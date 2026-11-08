//! Runs the Mac NSH app through `apps/nsh/acceptance/*.scenario`: each scenario
//! gets a fresh fixture Host in a temporary directory and a fresh app process
//! whose driver (`apps/nsh/src/acceptance`) takes the steps, so the user's own
//! Host and app state are never touched. The report and every scenario's
//! screenshots, trees and logs go to `target/nsh-acceptance/run-<time>/`.
use crate::{
    Result,
    supervision::{self, Child},
};
use serde::Deserialize;
use std::{
    fs::{self, File},
    path::{Path, PathBuf},
    time::{Duration, Instant, SystemTime},
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    process::Command,
};

const SCENARIOS: &str = "apps/nsh/acceptance";
/// The dev bex.app `scripts/build-macos.sh desktop dev` packages.
const APP: &str = "debug/bex.app/Contents/MacOS/bex";
const FIXTURES: &str = "aarch64-apple-darwin/debug";
const HOST_READY: Duration = Duration::from_secs(30);
/// The app's whole run, startup and every step's own timeout included.
const SCENARIO_LIMIT: Duration = Duration::from_secs(240);
/// How long a fixture turn keeps running before it completes.
const STREAM_DELAY_MS: &str = "2000";

pub async fn run(filters: &[String]) -> Result<()> {
    let cwd = std::env::current_dir()?;
    let target = std::env::var_os("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| cwd.join("target"));
    let tools = Tools {
        app: target.join(APP),
        fixture: target.join(FIXTURES).join("agent-ui-fixture"),
        codex: target.join(FIXTURES).join("agent-codex-fixture"),
        claude: target.join(FIXTURES).join("agent-claude-fixture"),
        deepseek: target.join(FIXTURES).join("agent-dsh-fixture"),
    };
    for path in [
        &tools.app,
        &tools.fixture,
        &tools.codex,
        &tools.claude,
        &tools.deepseek,
    ] {
        if !path.is_file() {
            return Err(format!(
                "{} is missing; `just nsh-acceptance` builds it",
                path.display()
            )
            .into());
        }
    }
    let mut scenarios = Vec::new();
    for entry in fs::read_dir(cwd.join(SCENARIOS))? {
        let path = entry?.path();
        if path
            .extension()
            .is_some_and(|extension| extension == "scenario")
        {
            scenarios.push(path);
        }
    }
    let scenarios = select(scenarios, filters)?;
    let seconds = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)?
        .as_secs();
    let records = target.join("nsh-acceptance").join(format!("run-{seconds}"));
    fs::create_dir_all(&records)?;
    let cancel = supervision::cancellation();
    let mut outcomes = Vec::new();
    for path in &scenarios {
        let name = name(path);
        println!("{name}: running");
        let output = records.join(&name);
        fs::create_dir_all(&output)?;
        let started = Instant::now();
        let outcome = match scenario(path, &output, &tools, &cancel).await {
            Ok(result) => Outcome {
                name,
                result,
                // A panic on a thread of its own leaves the app running: its
                // steps can pass.
                error: fs::read_to_string(output.join("app.log"))
                    .ok()
                    .and_then(|log| panic(&log).map(str::to_owned))
                    .map(|panic| format!("the app panicked: {panic} (see app.log)")),
                seconds: started.elapsed().as_secs_f64(),
            },
            Err(error) => Outcome {
                name,
                result: Report::default(),
                error: Some(error.to_string()),
                seconds: started.elapsed().as_secs_f64(),
            },
        };
        println!(
            "{}: {} in {:.1}s",
            outcome.name,
            if outcome.passed() { "passed" } else { "FAILED" },
            outcome.seconds
        );
        outcomes.push(outcome);
        if *cancel.borrow() {
            break;
        }
    }
    let report = records.join("report.md");
    fs::write(&report, render(&format!("run-{seconds}"), &outcomes))?;
    println!("report: {}", report.display());
    let failed = outcomes.iter().filter(|outcome| !outcome.passed()).count();
    if failed > 0 || outcomes.len() < scenarios.len() {
        return Err(format!(
            "{failed} of {} scenarios failed; {} not run",
            scenarios.len(),
            scenarios.len() - outcomes.len()
        )
        .into());
    }
    Ok(())
}

struct Tools {
    app: PathBuf,
    fixture: PathBuf,
    codex: PathBuf,
    claude: PathBuf,
    deepseek: PathBuf,
}

/// What the app's driver wrote to `result.json`.
#[derive(Default, Deserialize)]
struct Report {
    passed: Option<bool>,
    steps: Vec<StepResult>,
    evidence: Vec<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct StepResult {
    line: u64,
    source: String,
    status: String,
    message: String,
    elapsed_ms: u64,
    artifact: Option<String>,
}

struct Outcome {
    name: String,
    result: Report,
    /// Why the scenario could not run to its end.
    error: Option<String>,
    seconds: f64,
}

impl Outcome {
    fn passed(&self) -> bool {
        self.error.is_none() && self.result.passed == Some(true)
    }
}

fn name(path: &Path) -> String {
    path.file_stem()
        .map(|stem| stem.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// The scenarios whose names contain one of `filters` (all without any), by name.
fn select(mut scenarios: Vec<PathBuf>, filters: &[String]) -> Result<Vec<PathBuf>> {
    scenarios.sort();
    let all: Vec<String> = scenarios.iter().map(|path| name(path)).collect();
    scenarios.retain(|path| {
        filters.is_empty()
            || filters
                .iter()
                .any(|filter| name(path).contains(filter.as_str()))
    });
    if scenarios.is_empty() {
        return Err(format!("no scenario matches; available: {}", all.join(", ")).into());
    }
    Ok(scenarios)
}

/// A scenario's leading lines, which prepare the run before the app starts.
fn preparation(script: &str) -> impl Iterator<Item = &str> {
    script
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .take_while(|line| line.starts_with("fixture ") || *line == FIRST_LAUNCH)
}

/// The control paths a scenario's leading `fixture` lines post before the app
/// starts; the app's driver posts the later ones.
fn fixture_paths(script: &str) -> Vec<String> {
    preparation(script)
        .filter_map(|line| line.strip_prefix("fixture "))
        .map(|path| path.trim().trim_matches('"').to_owned())
        .collect()
}

/// The step that keeps the app's first-run setup; without it the app starts
/// as a returning user's, whose setup ended.
const FIRST_LAUNCH: &str = "first-launch";
/// What the app saves in its data once its setup ends
/// (`apps/nsh/src/application/onboarding.rs`).
const SETUP_COMPLETED: &str = "nsh-client/onboarding.completed";

async fn scenario(
    path: &Path,
    output: &Path,
    tools: &Tools,
    cancel: &tokio::sync::watch::Receiver<bool>,
) -> Result<Report> {
    let script = fs::read_to_string(path)?;
    let temporary = tempfile::Builder::new().prefix("bex-nsh-").tempdir()?;
    let host = temporary.path().join("host");
    let port = temporary.path().join("pairing.port");
    let log = File::create(output.join("host.log"))?;
    let mut command = Command::new(&tools.fixture);
    command
        .arg(&host)
        .arg(&tools.codex)
        .arg(&port)
        .arg(STREAM_DELAY_MS)
        .arg(&tools.claude)
        .arg("--deepseek")
        .arg(&tools.deepseek)
        // Publishes the Host where a client with the same isolated state finds it.
        .env("AGENT_FIXTURE_LOCAL_HOST", "1")
        // DeepSeek counts a key in the environment or DSH's home as configured;
        // the user's own stay out of the fixture's.
        .env("DSH_HOME", temporary.path().join("dsh"))
        .env_remove("DEEPSEEK_API_KEY")
        .stdin(std::process::Stdio::null())
        .stdout(log.try_clone()?)
        .stderr(log);
    let mut fixture = Child::spawn(command)?;
    let result: Result<Report> = async {
        let deadline = Instant::now() + HOST_READY;
        while !port.is_file() {
            if fixture.try_wait()?.is_some() {
                return Err("the fixture Host exited during startup; see host.log".into());
            }
            if Instant::now() >= deadline || *cancel.borrow() {
                return Err("the fixture Host did not become ready".into());
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        let port: u16 = fs::read_to_string(&port)?.trim().parse()?;
        for path in fixture_paths(&script) {
            post(port, &path).await?;
        }
        if !preparation(&script).any(|line| line == FIRST_LAUNCH) {
            let completed = host.join(SETUP_COMPLETED);
            fs::create_dir_all(completed.parent().expect("the marker has a directory"))?;
            fs::write(completed, "completed\n")?;
        }
        let log = File::create(output.join("app.log"))?;
        let mut command = Command::new(&tools.app);
        command
            .env("AGENT_STATE_DIR", &host)
            .env("AGENT_ISOLATED_HOST", "1")
            .env("BEX_ACCEPTANCE", path)
            .env("BEX_ACCEPTANCE_OUTPUT", output)
            .env("BEX_ACCEPTANCE_FIXTURE", port.to_string())
            .stdin(std::process::Stdio::null())
            .stdout(log.try_clone()?)
            .stderr(log);
        let ended = Child::spawn(command)?
            .output(cancel, SCENARIO_LIMIT, Duration::from_secs(10))
            .await;
        match fs::read(output.join("result.json")) {
            Ok(bytes) => {
                let report: Report = serde_json::from_slice(&bytes)?;
                match (report.passed, ended) {
                    (Some(_), _) => Ok(report),
                    (None, Err(error)) => {
                        Err(format!("the app stopped mid-scenario: {error}").into())
                    }
                    (None, Ok(output)) => {
                        Err(format!("the app exited mid-scenario with {}", output.status).into())
                    }
                }
            }
            Err(_) => Err(match ended {
                Ok(output) => format!(
                    "the app exited with {} before any step; see app.log",
                    output.status
                ),
                Err(error) => format!("the app wrote no result: {error}"),
            }
            .into()),
        }
    }
    .await;
    let stopped = fixture.stop(true, Duration::from_secs(10)).await;
    let diagnostics = host.join("logs/host.jsonl");
    if diagnostics.is_file() {
        fs::copy(diagnostics, output.join("host-diagnostics.jsonl"))?;
    }
    let report = result?;
    stopped?;
    Ok(report)
}

/// The first panic an app log reports.
fn panic(log: &str) -> Option<&str> {
    log.lines()
        .find(|line| line.contains("panicked at"))
        .map(str::trim)
}

/// Posts to the fixture Host's loopback control server.
async fn post(port: u16, path: &str) -> Result<()> {
    let mut stream = tokio::net::TcpStream::connect((std::net::Ipv4Addr::LOCALHOST, port)).await?;
    stream
        .write_all(
            format!(
                "POST {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
            )
            .as_bytes(),
        )
        .await?;
    let mut response = String::new();
    stream.read_to_string(&mut response).await?;
    let status = response.split_whitespace().nth(1).unwrap_or_default();
    if !status.starts_with('2') {
        return Err(format!("fixture {path} answered {status:?}").into());
    }
    Ok(())
}

fn cell(text: &str) -> String {
    text.replace('|', "\\|").replace('\n', " ")
}

fn render(run: &str, outcomes: &[Outcome]) -> String {
    let passed = outcomes.iter().filter(|outcome| outcome.passed()).count();
    let mut report = format!(
        "# NSH acceptance {run}\n\n{passed} of {} scenarios passed.\n\n| Scenario | Result | Failed step | Reason |\n|---|---|---|---|\n",
        outcomes.len()
    );
    for outcome in outcomes {
        let failed = outcome
            .result
            .steps
            .iter()
            .find(|step| step.status == "failed");
        let (step, reason) = match (&outcome.error, failed) {
            (Some(error), _) => (String::new(), error.clone()),
            (None, Some(step)) => (
                format!("line {}: `{}`", step.line, step.source),
                step.message.clone(),
            ),
            (None, None) => (String::new(), String::new()),
        };
        report.push_str(&format!(
            "| [{name}](#{name}) | {} | {} | {} |\n",
            if outcome.passed() { "passed" } else { "failed" },
            cell(&step),
            cell(&reason),
            name = outcome.name,
        ));
    }
    for outcome in outcomes {
        let name = &outcome.name;
        report.push_str(&format!(
            "\n## {name}\n\n{} in {:.1} s. Logs: [app]({name}/app.log), [Host]({name}/host.log).\n",
            if outcome.passed() { "Passed" } else { "Failed" },
            outcome.seconds
        ));
        if let Some(error) = &outcome.error {
            report.push_str(&format!("\n{error}\n"));
        }
        if !outcome.result.steps.is_empty() {
            report.push_str("\n| Line | Step | Result | ms | Message |\n|---|---|---|---|---|\n");
        }
        for step in &outcome.result.steps {
            report.push_str(&format!(
                "| {} | `{}` | {} | {} | {} |\n",
                step.line,
                cell(&step.source),
                step.status,
                step.elapsed_ms,
                cell(&step.message)
            ));
        }
        let images = outcome
            .result
            .steps
            .iter()
            .filter_map(|step| step.artifact.as_ref())
            .chain(&outcome.result.evidence);
        for file in images {
            report.push_str(&if file.ends_with(".png") {
                format!("\n![{file}]({name}/{file})\n")
            } else {
                format!("\n[{file}]({name}/{file})\n")
            });
        }
    }
    report
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filters_select_scenarios_by_name_and_reject_unknown_ones() {
        let all = vec![
            PathBuf::from("a/send-message.scenario"),
            PathBuf::from("a/approval.scenario"),
            PathBuf::from("a/settings.scenario"),
        ];
        assert_eq!(
            select(all.clone(), &[]).unwrap(),
            [&all[1], &all[0], &all[2]].map(Clone::clone)
        );
        assert_eq!(
            select(all.clone(), &["sett".into(), "send".into()]).unwrap(),
            [&all[0], &all[2]].map(Clone::clone)
        );
        assert_eq!(
            select(all, &["missing".into()]).unwrap_err().to_string(),
            "no scenario matches; available: approval, send-message, settings"
        );
    }

    #[test]
    fn only_leading_fixture_lines_are_posted_before_the_app_starts() {
        assert_eq!(
            fixture_paths(
                "# fixture /comment\nfixture /external-conversation\nfirst-launch\n\n  fixture \"/list-fixture\"\nexpect fixture\nfixture /release-inputs\n"
            ),
            ["/external-conversation", "/list-fixture"]
        );
    }

    #[test]
    fn only_a_leading_first_launch_keeps_the_setup() {
        let first = |script| preparation(script).any(|line| line == FIRST_LAUNCH);
        assert!(first("fixture /list-fixture\nfirst-launch\nexpect x\n"));
        assert!(!first("fixture /list-fixture\nexpect x\n"));
        assert!(!first("expect x\nfirst-launch\n"));
    }

    #[test]
    fn a_panic_in_the_app_log_is_found() {
        assert_eq!(
            panic(
                "started\nthread 'nsh-engine' (1) panicked at apps/nsh/src/terminal.rs:71:20:\nboom\n"
            ),
            Some("thread 'nsh-engine' (1) panicked at apps/nsh/src/terminal.rs:71:20:")
        );
        assert_eq!(panic("started\nquit\n"), None);
    }

    #[test]
    fn the_report_leads_with_each_failure_and_links_its_evidence() {
        let report: Report = serde_json::from_value(serde_json::json!({
            "scenario": "approval.scenario",
            "passed": false,
            "steps": [
                {"line": 1, "source": "screenshot start", "status": "passed", "message": "wrote start.png", "elapsedMs": 3, "artifact": "start.png"},
                {"line": 2, "source": "press \"承認|する\"", "status": "failed", "message": "no element is labelled", "elapsedMs": 10000, "artifact": null},
                {"line": 3, "source": "expect done", "status": "skipped", "message": "", "elapsedMs": 0, "artifact": null}
            ],
            "evidence": ["failure.png", "failure-tree.txt"]
        }))
        .unwrap();
        let outcomes = [
            Outcome {
                name: "approval".into(),
                result: report,
                error: None,
                seconds: 12.0,
            },
            Outcome {
                name: "startup".into(),
                result: Report::default(),
                error: Some("the fixture Host did not become ready".into()),
                seconds: 30.0,
            },
        ];
        let text = render("run-1", &outcomes);
        assert!(text.contains("0 of 2 scenarios passed."));
        assert!(text.contains(
            "| [approval](#approval) | failed | line 2: `press \"承認\\|する\"` | no element is labelled |"
        ));
        assert!(text.contains(
            "| [startup](#startup) | failed |  | the fixture Host did not become ready |"
        ));
        for link in [
            "![start.png](approval/start.png)",
            "![failure.png](approval/failure.png)",
            "[failure-tree.txt](approval/failure-tree.txt)",
            "[app](approval/app.log)",
        ] {
            assert!(text.contains(link), "{link}");
        }
    }
}
