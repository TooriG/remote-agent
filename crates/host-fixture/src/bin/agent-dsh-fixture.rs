//! DeepSeek Harness (DSH) as the Host runs it (`--profile acp --patch <file>`):
//! ACP v1 over stdio for its model catalog, without inference or conversations.
//! Its models are DSH's official Flash and, once the patch carries an OpenRouter
//! key's route, DeepSeek V4.1 Flash through OpenRouter.
use serde_json::{Value, json};
use std::io::{BufRead, Write};

const OFFICIAL: &str = r#"["deepseek-official","deepseek-flash"]"#;
const OPENROUTER: &str = r#"["openrouter","deepseek/deepseek-v4.1-flash"]"#;

fn main() {
    let arguments: Vec<String> = std::env::args().collect();
    let patch = arguments
        .iter()
        .position(|argument| argument == "--patch")
        .and_then(|index| arguments.get(index + 1))
        .expect("the Host passes --patch");
    let overlays: Value = serde_json::from_slice(&std::fs::read(patch).unwrap()).unwrap();
    let routed = overlays.as_array().into_iter().flatten().any(|overlay| {
        overlay["id"] == "llm-pi-ai" && overlay["config"]["providers"]["openrouter"].is_object()
    });
    let mut models = vec![(OFFICIAL, "DeepSeek V4 Flash")];
    if routed {
        models.push((OPENROUTER, "DeepSeek: DeepSeek V4.1 Flash"));
    }
    let mut created = 0;
    let mut output = std::io::stdout().lock();
    for line in std::io::stdin().lock().lines() {
        let Ok(message) = serde_json::from_str::<Value>(&line.unwrap()) else {
            break;
        };
        // Notifications need no reply.
        let Some(id) = message.get("id").cloned() else {
            continue;
        };
        let params = &message["params"];
        let result = match message["method"].as_str().unwrap_or_default() {
            "initialize" => Ok(json!({"protocolVersion": 1, "agentCapabilities": {}})),
            "session/new" => {
                created += 1;
                Ok(json!({
                    "sessionId": format!("dsh-fixture-{created}"),
                    "configOptions": options(&models, OFFICIAL),
                }))
            }
            "session/set_config_option" if params["configId"] == "model" => {
                let model = params["value"].as_str().unwrap_or_default();
                if models.iter().any(|(id, _)| *id == model) {
                    Ok(json!({"configOptions": options(&models, model)}))
                } else {
                    Err(format!("unknown model {model}"))
                }
            }
            "session/close" => Ok(json!({})),
            method => Err(format!("unsupported method {method}")),
        };
        let reply = match result {
            Ok(result) => json!({"jsonrpc": "2.0", "id": id, "result": result}),
            Err(message) => {
                json!({"jsonrpc": "2.0", "id": id, "error": {"code": -32601, "message": message}})
            }
        };
        writeln!(output, "{reply}").unwrap();
        output.flush().unwrap();
    }
}

/// The model and reasoning options with `current` chosen.
fn options(models: &[(&str, &str)], current: &str) -> Value {
    json!([
        {"id": "model", "currentValue": current, "options": models.iter().map(|(value, name)| json!({"value": value, "name": name})).collect::<Vec<_>>()},
        {"id": "reasoning_effort", "currentValue": "high", "options": [{"value": "high", "name": "High"}]},
    ])
}
