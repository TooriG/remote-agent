//! Deterministic external Claude Code boundary. Conversation history lives in
//! native storage; tool side effects and input traces use the working directory.
use serde_json::{Value, json};
use std::{
    fs,
    io::{self, BufRead, Write},
    path::Path,
    sync::Mutex,
};

// Root partial-message frames have no message ID to multiplex two responses.
// Background notifications may interleave, but their answer must wait its turn.
static ROOT_RESPONSE: Mutex<()> = Mutex::new(());

fn option(args: &[String], name: &str) -> Option<String> {
    args.iter().enumerate().find_map(|(index, arg)| {
        if arg == name {
            args.get(index + 1).cloned()
        } else {
            arg.strip_prefix(&format!("{name}=")).map(str::to_owned)
        }
    })
}

fn write_json_line(writer: &mut impl Write, value: &Value) -> io::Result<()> {
    // Native history is read concurrently. Formatting directly into a File
    // exposes pieces of a JSON record before its terminating newline.
    let mut row = serde_json::to_vec(value).map_err(io::Error::other)?;
    row.push(b'\n');
    writer.write_all(&row)
}

/// Claude keeps a session beside others started in the same workspace.
fn project_directory(home: &Path) -> std::path::PathBuf {
    let cwd = std::env::current_dir().unwrap();
    let name: String = cwd
        .to_string_lossy()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();
    home.join("projects").join(name)
}

fn native(session: &str, mut value: Value) {
    if value["type"] == "system" && value["subtype"] == "task_notification" {
        let Some(tool) = value["tool_use_id"].as_str() else {
            return;
        };
        let prompt = format!(
            "<task-notification><task-id>{}</task-id><tool-use-id>{tool}</tool-use-id><output-file>{}</output-file><status>{}</status><summary>{}</summary></task-notification>",
            value["task_id"].as_str().unwrap(),
            value["output_file"].as_str().unwrap_or_default(),
            value["status"].as_str().unwrap(),
            value["summary"].as_str().unwrap()
        );
        value = json!({"type":"user","origin":{"kind":"task-notification"},"promptSource":"system","turnOrigin":"task_notification","queueSkipAttachments":true,"message":{"role":"user","content":prompt}});
    }
    if !matches!(
        value["type"].as_str(),
        Some("assistant" | "user" | "attachment")
    ) {
        return;
    }
    let Some(home) = std::env::var_os("CLAUDE_CONFIG_DIR") else {
        return;
    };
    if let Some(result) = value.as_object_mut().unwrap().remove("tool_use_result") {
        value["toolUseResult"] = result;
    }
    let directory = project_directory(Path::new(&home));
    fs::create_dir_all(&directory).unwrap();
    let path = directory.join(format!("{session}.jsonl"));
    let previous: Option<Value> = fs::read_to_string(&path).ok().and_then(|text| {
        text.lines()
            .last()
            .and_then(|line| serde_json::from_str(line).ok())
    });
    value["parentUuid"] = previous
        .map(|value| value["uuid"].clone())
        .unwrap_or(Value::Null);
    value["uuid"] = value.get("uuid").cloned().unwrap_or_else(|| {
        format!(
            "fixture-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        )
        .into()
    });
    value["sessionId"] = session.into();
    value["cwd"] = std::env::current_dir()
        .unwrap()
        .to_string_lossy()
        .to_string()
        .into();
    value["version"] = "2.1.266".into();
    let mut file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .unwrap();
    write_json_line(&mut file, &value).unwrap();
}

fn session_state(session: &str, state: &str) -> Value {
    json!({"type":"system","subtype":"session_state_changed","state":state,"session_id":session,"sdk_host_only":true})
}

fn emit(session: &str, value: Value) {
    let idle = (value["type"] == "result")
        .then(|| session_state(value["session_id"].as_str().unwrap(), "idle"));
    native(session, value.clone());
    output(value);
    if let Some(idle) = idle {
        output(idle);
    }
}

fn output(value: Value) {
    let mut out = io::stdout().lock();
    writeln!(out, "{value}").unwrap();
    out.flush().unwrap();
}

/// Claude saves each block with its message stop reason, but live output sends
/// the block before that reason arrives in `message_delta`.
fn block(
    session: &str,
    message: &str,
    index: usize,
    kind: &str,
    text: &str,
    stop_reason: Option<&str>,
) {
    let (field, delta) = if kind == "thinking" {
        ("thinking", "thinking_delta")
    } else {
        ("text", "text_delta")
    };
    emit(
        session,
        json!({"type":"stream_event","session_id":session,"event":{"type":"content_block_start","index":index,"content_block":{"type":kind,field:""}}}),
    );
    emit(
        session,
        json!({"type":"stream_event","session_id":session,"event":{"type":"content_block_delta","index":index,"delta":{"type":delta,field:text}}}),
    );
    let envelope = json!({"type":"assistant","apiBlockIndex":index,"uuid":format!("envelope-{message}-{index}"),"session_id":session,"message":{"id":message,"role":"assistant","stop_reason":null,"content":[{"type":kind,field:text}]}});
    let mut saved = envelope.clone();
    saved["message"]["stop_reason"] = stop_reason.into();
    native(session, saved);
    output(envelope);
    emit(
        session,
        json!({"type":"stream_event","session_id":session,"event":{"type":"content_block_stop","index":index}}),
    );
}

fn end_turn(session: &str) {
    emit(
        session,
        json!({"type":"stream_event","session_id":session,"event":{"type":"message_delta","delta":{"stop_reason":"end_turn"}}}),
    );
}

fn emit_reply(session: &str, message: &str, text: &str, idle_before_result: bool) {
    emit(
        session,
        json!({"type":"stream_event","session_id":session,"event":{"type":"message_start","message":{"id":message}}}),
    );
    block(
        session,
        message,
        0,
        "thinking",
        "Fixture reasoning",
        Some("end_turn"),
    );
    block(session, message, 1, "text", text, Some("end_turn"));
    end_turn(session);
    let result = json!({"type":"result","subtype":"success","session_id":session,"is_error":false,"result":text});
    if idle_before_result {
        output(session_state(session, "idle"));
        output(result);
    } else {
        emit(session, result);
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ControlRequest {
    Unknown,
    Dialog,
    Elicitation,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Waiting {
    Control(ControlRequest),
    Permission,
    Question,
    Background,
    BackgroundSettled,
    BackgroundShell,
    Wait,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum PermissionScenario {
    Command,
    Question,
    Background,
    BackgroundSettled,
}
impl PermissionScenario {
    fn tool(self) -> (&'static str, Value) {
        match self {
            Self::Question => (
                "AskUserQuestion",
                json!({"questions":[{"question":"Which color?","header":"Color","multiSelect":false,"options":[{"label":"Blue","description":"Use blue"},{"label":"Red","description":"Use red"}]}]}),
            ),
            _ => ("Bash", json!({"command":"printf approved > approved.txt"})),
        }
    }
    fn waiting(self) -> Waiting {
        match self {
            Self::Command => Waiting::Permission,
            Self::Question => Waiting::Question,
            Self::Background => Waiting::Background,
            Self::BackgroundSettled => Waiting::BackgroundSettled,
        }
    }
}

enum InputScenario {
    Crash,
    Error,
    BackgroundShell,
    Wait,
    Control(ControlRequest),
    Permission(PermissionScenario),
    Reply,
}
impl InputScenario {
    fn parse(text: &str, result_error: bool) -> Self {
        match text {
            "crash" => Self::Crash,
            _ if result_error => Self::Error,
            "background-shell" => Self::BackgroundShell,
            "wait" => Self::Wait,
            "unknown_control" => Self::Control(ControlRequest::Unknown),
            "dialog" => Self::Control(ControlRequest::Dialog),
            "elicitation" => Self::Control(ControlRequest::Elicitation),
            "permission" => Self::Permission(PermissionScenario::Command),
            "question" => Self::Permission(PermissionScenario::Question),
            "background" => Self::Permission(PermissionScenario::Background),
            "background-settled" => Self::Permission(PermissionScenario::BackgroundSettled),
            _ => Self::Reply,
        }
    }
}

struct Fixture {
    model: Option<String>,
    effort: Option<String>,
    config: Value,
    session: String,
    inputs: Vec<Value>,
    process: String,
    history: std::path::PathBuf,
    trace: String,
    waiting: Option<Waiting>,
}

impl Fixture {
    fn interrupt(&mut self, value: &Value) {
        emit(
            &self.session,
            json!({"type":"control_response","response":{"subtype":"success","request_id":value["request_id"],"response":{}}}),
        );
        // A settled response has no result to end; its shell keeps running.
        if self.waiting != Some(Waiting::BackgroundShell) {
            emit(
                &self.session,
                json!({"type":"result","session_id":self.session,"is_error":false,"result":"interrupted"}),
            );
        }
        self.waiting = None;
    }

    fn message_id(&self) -> String {
        format!("message-{}", self.inputs.len())
    }

    fn reply(&self, text: &str) {
        emit_reply(
            &self.session,
            &self.message_id(),
            text,
            self.config["idleBeforeResult"] == true,
        );
    }

    fn initialize(&self, value: &Value) {
        let home = std::env::var_os("CLAUDE_SECURESTORAGE_CONFIG_DIR").unwrap();
        let home = Path::new(&home);
        let account = if self.config["unauthenticated"] == true {
            json!({})
        } else if let Ok(bytes) = fs::read(home.join("fixture-auth.json")) {
            serde_json::from_slice(&bytes).unwrap()
        } else if self.session == "catalog" {
            json!({"tokenSource":"none","apiKeySource":"none"})
        } else {
            json!({"subscriptionType":"Claude Max","tokenSource":"oauth","apiKeySource":"none"})
        };
        if self.session == "catalog" {
            let home = std::env::var_os("CLAUDE_CONFIG_DIR").unwrap();
            let home = Path::new(&home);
            if let Ok(mode) = fs::read_to_string(home.join("model-list-mode")) {
                fs::write(home.join("model-list-requested"), []).unwrap();
                if mode == "silent" {
                    return;
                }
            }
        }
        if self.config["initializeError"] == true {
            emit(
                &self.session,
                json!({"type":"control_response","response":{"subtype":"error","request_id":value["request_id"],"error":"fixture initialization failed"}}),
            );
        } else {
            emit(
                &self.session,
                json!({"type":"control_response","response":{"subtype":"success","request_id":value["request_id"],"response":{
                    "models":fs::read(Path::new(&std::env::var_os("CLAUDE_CONFIG_DIR").unwrap()).join("fixture-models.json"))
                        .ok()
                        .map(|bytes| serde_json::from_slice::<Value>(&bytes).unwrap())
                        .unwrap_or_else(|| json!([
                        {"value":"default","displayName":"Default (recommended)","description":"Opus 5 with 1M context · Best for everyday, complex tasks","supportedEffortLevels":["low","high"]},
                        {"value":"opus[1m]","displayName":"Opus (1M context)","description":"Opus 5 with 1M context · Best for everyday, complex tasks"},
                        {"value":"claude-fable-5-1[1m]","displayName":"Fable","description":"Fable 5.1 · Most capable for your hardest and longest-running tasks"},
                        {"value":"sonnet","displayName":"Sonnet","description":"Sonnet 5 · Efficient for routine tasks"},
                        {"value":"haiku","displayName":"Haiku","description":"Haiku 4.5 · Fastest for quick answers"},
                        {"value":"custom","displayName":"Custom model"}
                    ])),
                    "commands":[{"name":"fixture-skill","description":"Fixture skill"}],
                    "account":account
                }}}),
            );
        }
    }

    fn user(&mut self, value: &Value) {
        assert_ne!(self.session, "catalog");
        assert_eq!(value["session_id"], self.session);
        assert!(
            self.config["unauthenticated"] != true,
            "unauthenticated input must never reach Claude"
        );
        assert_eq!(
            self.model.as_deref(),
            Some(self.config["expectedModel"].as_str().unwrap_or("default"))
        );
        if self.waiting.take() == Some(Waiting::Wait) {
            // The previous input can finish before the queued input is consumed.
            emit(
                &self.session,
                json!({"type":"result","session_id":self.session,"is_error":false,"result":"finished waiting"}),
            );
            native(
                &self.session,
                json!({"type":"attachment","attachment":{
                    "type":"queued_command","commandMode":"prompt","origin":{"kind":"human"},"source_uuid":value["uuid"],
                    "prompt":value["message"]["content"]
                }}),
            );
            output(value.clone());
        } else {
            emit(&self.session, value.clone());
        }
        let content = value["message"]["content"].clone();
        self.inputs.push(
            json!({"content":content,"model":self.model,"effort":self.effort,"process":self.process}),
        );
        let bytes = serde_json::to_vec(&self.inputs).unwrap();
        fs::create_dir_all(self.history.parent().unwrap()).unwrap();
        fs::write(&self.history, &bytes).unwrap();
        // Append: Windows cannot truncate a file that Git's workspace review maps.
        let mut trace = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.trace)
            .unwrap();
        let written = trace.metadata().unwrap().len() > 0;
        for input in &self.inputs[if written { self.inputs.len() - 1 } else { 0 }..] {
            write_json_line(&mut trace, input).unwrap();
        }
        emit(
            &self.session,
            json!({"type":"system","subtype":"init","session_id":self.session}),
        );
        output(session_state(&self.session, "running"));
        let text = content
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|block| block["text"].as_str())
            .collect::<Vec<_>>()
            .join("\n");
        self.waiting = self.respond_to_input(&text);
    }

    fn control_response(&mut self, value: &Value) {
        match self.waiting {
            Some(Waiting::Control(control)) => {
                assert_eq!(value["response"]["request_id"], "control-1");
                match control {
                    ControlRequest::Unknown => assert_eq!(value["response"]["subtype"], "error"),
                    ControlRequest::Dialog => {
                        assert_eq!(value["response"]["response"]["behavior"], "cancelled")
                    }
                    ControlRequest::Elicitation => assert_eq!(
                        value["response"]["response"],
                        json!({"action":"accept","content":{"name":"BEX"}})
                    ),
                }
                self.reply("control resolved");
            }
            _ => {
                assert_eq!(value["response"]["request_id"], "permission-1");
                let response = &value["response"]["response"];
                let allowed = response["behavior"] == "allow";
                let text = if self.waiting == Some(Waiting::Question) && allowed {
                    response["updatedInput"]["answers"]["Which color?"]
                        .as_str()
                        .unwrap()
                        .to_owned()
                } else if allowed {
                    fs::write("approved.txt", "approved").unwrap();
                    "approved".into()
                } else {
                    "denied".into()
                };
                emit(
                    &self.session,
                    json!({"type":"user","uuid":format!("tool-result-{}", self.inputs.len()),
                        "message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"tool-1","content":text,"is_error":!allowed}]},
                        "tool_use_result":if allowed && self.waiting == Some(Waiting::Background) { json!({"backgroundTaskId":"background-1"}) }
                            else if allowed && self.waiting != Some(Waiting::Question) { json!({"exitCode":0}) } else { json!({}) }}),
                );
                if self.waiting == Some(Waiting::Background) {
                    let output_file = std::env::current_dir().unwrap().join("background.output");
                    fs::write(&output_file, "background stdout\n").unwrap();
                    emit(
                        &self.session,
                        json!({"type":"system","subtype":"task_notification","task_id":"background-1","tool_use_id":"tool-1","output_file":output_file,"status":"completed","summary":"background done","session_id":self.session}),
                    );
                    while Path::new("background-paused").exists() {
                        std::thread::sleep(std::time::Duration::from_millis(10));
                    }
                }
                self.reply(&text);
            }
        }
        self.waiting = None;
    }

    fn respond_to_input(&self, text: &str) -> Option<Waiting> {
        match InputScenario::parse(text, self.config["resultError"] == true) {
            InputScenario::Crash => {
                std::process::exit(17);
            }
            InputScenario::Error => {
                emit(
                    &self.session,
                    json!({"type":"result","session_id":self.session,"is_error":true,"errors":["fixture inference failed"]}),
                );
                None
            }
            InputScenario::BackgroundShell => {
                // Claude settles the response while its shell runs, then
                // answers the finished shell without another input.
                emit(
                    &self.session,
                    json!({"type":"system","subtype":"background_tasks_changed","session_id":self.session,"tasks":[
                        {"task_id":"shell-1","task_type":"local_bash","description":"background shell"},
                        {"task_id":"watcher-1","task_type":"monitor","description":"watcher","ambient":true}
                    ]}),
                );
                let message = self.message_id();
                emit_reply(&self.session, &message, "あとで結果を報告します", false);
                let session = self.session.clone();
                std::thread::spawn(move || {
                    while Path::new("background-shell-paused").exists() {
                        std::thread::sleep(std::time::Duration::from_millis(10));
                    }
                    emit(
                        &session,
                        json!({"type":"system","subtype":"background_tasks_changed","session_id":session,"tasks":[
                            {"task_id":"watcher-1","task_type":"monitor","description":"watcher","ambient":true}
                        ]}),
                    );
                    emit(
                        &session,
                        json!({"type":"system","subtype":"task_notification","task_id":"shell-1","status":"completed","output_file":"","summary":"background shell done","session_id":session}),
                    );
                    let _response = ROOT_RESPONSE.lock().unwrap();
                    emit(
                        &session,
                        json!({"type":"system","subtype":"init","session_id":session}),
                    );
                    output(session_state(&session, "running"));
                    emit_reply(
                        &session,
                        &format!("{message}-report"),
                        "結果を報告します",
                        false,
                    );
                });
                Some(Waiting::BackgroundShell)
            }
            InputScenario::Wait => {
                let message = format!("waiting-{}", self.inputs.len());
                emit(
                    &self.session,
                    json!({"type":"stream_event","session_id":self.session,"event":{"type":"message_start","message":{"id":message}}}),
                );
                block(
                    &self.session,
                    &message,
                    0,
                    "text",
                    "Waiting for interruption",
                    None,
                );
                Some(Waiting::Wait)
            }
            InputScenario::Control(control) => {
                let request = match control {
                    ControlRequest::Dialog => {
                        json!({"subtype":"request_user_dialog","dialog_kind":"future"})
                    }
                    ControlRequest::Elicitation => {
                        json!({"subtype":"elicitation","mcp_server_name":"fixture","message":"Name?","requested_schema":{"type":"object","required":["name"],"properties":{"name":{"type":"string","minLength":1}}}})
                    }
                    ControlRequest::Unknown => json!({"subtype":"future_control"}),
                };
                emit(
                    &self.session,
                    json!({"type":"control_request","request_id":"control-1","request":request}),
                );
                Some(Waiting::Control(control))
            }
            InputScenario::Permission(permission) => {
                if matches!(
                    permission,
                    PermissionScenario::Background | PermissionScenario::BackgroundSettled
                ) {
                    emit(
                        &self.session,
                        json!({"type":"system","subtype":"task_started","task_id":"background-1","task_type":"local_agent","description":"background work","session_id":self.session}),
                    );
                    let message = format!("background-{}", self.inputs.len());
                    emit(
                        &self.session,
                        json!({"type":"stream_event","session_id":self.session,"event":{"type":"message_start","message":{"id":message}}}),
                    );
                    block(
                        &self.session,
                        &message,
                        0,
                        "text",
                        "あとで報告します",
                        Some("end_turn"),
                    );
                    end_turn(&self.session);
                    if permission == PermissionScenario::BackgroundSettled {
                        emit(
                            &self.session,
                            json!({"type":"system","subtype":"task_notification","task_id":"background-1","status":"completed","summary":"background done","session_id":self.session}),
                        );
                    }
                    // The parent still owes a follow-up after this result.
                    output(
                        json!({"type":"result","session_id":self.session,"is_error":false,"result":"あとで報告します"}),
                    );
                }
                let (tool, input) = permission.tool();
                emit(
                    &self.session,
                    json!({"type":"assistant","uuid":format!("tool-message-{}", self.inputs.len()),"message":{"id":"tool-message","role":"assistant","content":[{"type":"tool_use","id":"tool-1","name":tool,"input":input}]}}),
                );
                emit(
                    &self.session,
                    json!({"type":"control_request","request_id":"permission-1","request":{"subtype":"can_use_tool","tool_name":tool,"tool_use_id":"tool-1","input":input}}),
                );
                Some(permission.waiting())
            }
            InputScenario::Reply => {
                self.reply(&format!("reply {}: {text}", self.inputs.len()));
                None
            }
        }
    }
}

fn usage(session: &str, value: &Value) {
    assert_eq!(value["request"]["skip_behaviors"], true);
    let home = std::env::var_os("CLAUDE_SECURESTORAGE_CONFIG_DIR").unwrap();
    let home = Path::new(&home);
    if fs::read_to_string(home.join("usage-paused"))
        .is_ok_and(|cwd| Path::new(&cwd) == std::env::current_dir().unwrap())
    {
        fs::write(home.join("usage-requested"), "").unwrap();
        while home.join("usage-paused").exists() {
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
    }
    emit(
        session,
        json!({"type":"control_response","response":{"subtype":"success","request_id":value["request_id"],"response":{"rate_limits":{"limits":[
            {"kind":"session","group":"session","percent":72,"resets_at":"2033-05-18T03:33:20Z","scope":null},
            {"kind":"weekly_all","group":"weekly","percent":39,"resets_at":"2033-05-24T03:33:20Z","scope":null},
            {"kind":"weekly_scoped","group":"weekly","percent":34,"resets_at":"2033-05-24T03:33:20Z","scope":{"model":{"display_name":"Fable"}}}
        ]}}}}),
    );
}

fn main() {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.first().map(String::as_str) == Some("auth") {
        auth(&args);
        return;
    }

    // Process IDs can be reused after a restart; this names one process run.
    let process = format!(
        "{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    );
    assert_eq!(
        option(&args, "--input-format").as_deref(),
        Some("stream-json")
    );
    assert_eq!(
        option(&args, "--output-format").as_deref(),
        Some("stream-json")
    );
    assert_eq!(
        option(&args, "--permission-prompt-tool").as_deref(),
        Some("stdio")
    );
    assert_eq!(
        std::env::var("CLAUDE_CODE_SDK_READS_SESSION_STATE").as_deref(),
        Ok("1")
    );
    assert!(
        !args
            .iter()
            .any(|arg| arg.contains("skip-permissions") || arg == "--bare")
    );
    let config: Value = fs::read("claude-fixture.json")
        .ok()
        .map(|bytes| serde_json::from_slice(&bytes).unwrap())
        .unwrap_or(json!({}));
    let session = option(&args, "--resume")
        .or_else(|| option(&args, "--session-id"))
        .unwrap_or_else(|| "catalog".into());
    if session != "catalog" {
        let mut trace = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open("claude-auth-homes.jsonl")
            .unwrap();
        let config_home = std::env::var("CLAUDE_CONFIG_DIR").unwrap();
        let credentials_home = std::env::var("CLAUDE_SECURESTORAGE_CONFIG_DIR").unwrap();
        write_json_line(&mut trace, &json!({
            "configHome": config_home,
            "credentialsHome": credentials_home,
            "skill": fs::read_to_string(Path::new(&config_home).join("skills/account-test/SKILL.md")).ok(),
            "settings": fs::read_to_string(Path::new(&config_home).join("settings.json")).ok(),
            "account": fs::read(Path::new(&credentials_home).join("fixture-auth.json")).ok()
                .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok())
                .and_then(|value| value["email"].as_str().map(str::to_owned)),
        })).unwrap();
    }
    let path = format!("claude-session-{session}.jsonl");
    let history = project_directory(Path::new(&std::env::var_os("CLAUDE_CONFIG_DIR").unwrap()))
        .join(format!("{session}.inputs.json"));
    let inputs: Vec<Value> = if option(&args, "--resume").is_some() {
        serde_json::from_slice(
            &fs::read(&history).expect("resume must find the original session in native storage"),
        )
        .unwrap()
    } else {
        assert!(
            !history.exists(),
            "a new session cannot overwrite a previous session"
        );
        Vec::new()
    };
    assert!(args.iter().any(|arg| arg == "--replay-user-messages"));
    let mut fixture = Fixture {
        model: option(&args, "--model"),
        effort: option(&args, "--effort"),
        config,
        session,
        inputs,
        process,
        history,
        trace: path,
        waiting: None,
    };

    for line in io::stdin().lock().lines() {
        let value: Value = serde_json::from_str(&line.unwrap()).unwrap();
        let _response = ROOT_RESPONSE.lock().unwrap();
        match value["type"].as_str().unwrap() {
            "control_request" if value["request"]["subtype"] == "initialize" => {
                fixture.initialize(&value)
            }
            "control_request" if value["request"]["subtype"] == "get_usage" => {
                usage(&fixture.session, &value)
            }
            "user" => fixture.user(&value),
            "control_response" => fixture.control_response(&value),
            "control_request" if value["request"]["subtype"] == "interrupt" => {
                fixture.interrupt(&value)
            }
            _ => panic!("unexpected fixture input type"),
        }
    }
}

fn auth(args: &[String]) {
    let home = std::path::PathBuf::from(
        std::env::var_os("CLAUDE_CONFIG_DIR").expect("isolated auth home"),
    );
    fs::create_dir_all(&home).unwrap();
    let file = home.join("fixture-auth.json");
    match args.get(1).map(String::as_str) {
        Some("login") => {
            assert!(args.iter().any(|arg| arg == "--claudeai"));
            println!(
                "If the browser didn't open, visit: https://claude.com/cai/oauth/authorize?state=fixture-only"
            );
            print!("Paste code here if prompted > ");
            io::stdout().flush().unwrap();
            let mut code = String::new();
            io::stdin().read_line(&mut code).unwrap();
            if code.trim() != "fixture-code" {
                std::process::exit(1);
            }
            fs::write(file, serde_json::to_vec(&json!({"tokenSource":"oauth","apiKeySource":"none","email":"claude@example.invalid","subscriptionType":"max"})).unwrap()).unwrap();
        }
        Some("logout") => {
            if home.join("fail-auth-logout").exists() {
                std::process::exit(1);
            }
            let _ = fs::remove_file(file);
        }
        _ => panic!("unexpected auth command"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Default)]
    struct ObservedWrites(Vec<Vec<u8>>);

    impl Write for ObservedWrites {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            self.0.push(bytes.to_vec());
            Ok(bytes.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn json_line_readers_only_observe_complete_records() {
        for value in [
            json!({"text": "quoted \"text\"\n日本語"}),
            json!(null),
            json!([true, 12]),
        ] {
            let mut writer = ObservedWrites::default();
            write_json_line(&mut writer, &value).unwrap();
            assert_eq!(
                writer.0.len(),
                1,
                "a native-history reader can run between file writes"
            );
            assert!(writer.0[0].ends_with(b"\n"));
            assert_eq!(
                serde_json::from_slice::<Value>(&writer.0[0]).unwrap(),
                value
            );
        }
    }

    #[test]
    fn json_line_write_errors_are_not_reported_as_persisted() {
        struct Unavailable;
        impl Write for Unavailable {
            fn write(&mut self, _: &[u8]) -> io::Result<usize> {
                Err(io::Error::from(io::ErrorKind::BrokenPipe))
            }
            fn flush(&mut self) -> io::Result<()> {
                Ok(())
            }
        }
        let error = write_json_line(&mut Unavailable, &json!({"text": "hello"})).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::BrokenPipe);
    }
}
