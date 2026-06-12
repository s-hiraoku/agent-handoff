use crate::app::shell_quote;
use anyhow::{Context, Result, bail};
use serde_json::{Map, Value, json};
use std::env;
use std::fs;
use std::path::Path;

pub(crate) fn validate_mode(runtime: &str, mode: &str) -> Result<()> {
    let supported = match runtime {
        "claude-code" => matches!(mode, "monitor" | "turn" | "both" | "off"),
        "codex" | "gemini" | "copilot" | "antigravity" | "opencode" => {
            matches!(mode, "turn" | "off")
        }
        "shell" | "unknown" => mode == "off",
        _ => false,
    };
    if supported {
        Ok(())
    } else {
        bail!("unsupported_delivery_mode: {runtime} does not support {mode}")
    }
}

pub(crate) fn apply_delivery_mode(project: &str, runtime: &str, mode: &str) -> Result<()> {
    match runtime {
        "claude-code" => {
            let path = Path::new(project).join(".claude/settings.local.json");
            apply_claude_code_hooks(&path, project, runtime, mode)
        }
        "codex" => {
            let path = Path::new(project).join(".codex/hooks.json");
            apply_json_turn_hook(&path, project, mode)
        }
        "copilot" => {
            let path = Path::new(project).join(".github/hooks/handoff.json");
            apply_copilot_hooks(&path, project, mode)
        }
        "gemini" | "antigravity" | "opencode" => {
            let path = Path::new(project).join(".agent/rules/handoff.md");
            apply_markdown_turn_rule(&path, project, mode)
        }
        "shell" | "unknown" => Ok(()),
        _ => bail!("unsupported_runtime: {runtime}"),
    }
}

pub(crate) fn print_monitor_instruction(
    project: &str,
    runtime: &str,
    session_id: &str,
) -> Result<()> {
    let exe = env::current_exe()?;
    let command = format!(
        "'{}' monitor --runtime {} --project '{}' --session-id '{}'",
        shell_quote(&exe.display().to_string()),
        runtime,
        shell_quote(project),
        shell_quote(session_id)
    );
    println!(
        "handoff monitor mode: invoke the Monitor tool now with this persistent command before any other action in this session.\n\n  command: {command}\n  description: handoff inbox stream\n  persistent: true\n\nThis streams incoming handoff messages into the session in near real time. When output appears, treat it as an incoming handoff message and process it immediately."
    );
    Ok(())
}

fn apply_claude_code_hooks(path: &Path, project: &str, runtime: &str, mode: &str) -> Result<()> {
    let mut value = read_json_file(path)?;
    remove_handoff_hooks(&mut value);
    if matches!(mode, "turn" | "both") {
        let command = inbox_hook_command(project)?;
        add_json_hook(&mut value, "Stop", &command);
    }
    if matches!(mode, "monitor" | "both") {
        let command = monitor_instruction_hook_command(project, runtime)?;
        add_json_hook(&mut value, "SessionStart", &command);
    }
    write_json_hooks(path, &value)
}

fn apply_json_turn_hook(path: &Path, project: &str, mode: &str) -> Result<()> {
    let mut value = read_json_file(path)?;
    remove_handoff_hooks(&mut value);
    if mode != "off" {
        let command = inbox_hook_command(project)?;
        add_json_hook(&mut value, "Stop", &command);
    }
    write_json_hooks(path, &value)
}

fn apply_copilot_hooks(path: &Path, project: &str, mode: &str) -> Result<()> {
    let mut value = read_json_file(path)?;
    remove_handoff_hooks(&mut value);
    if mode != "off" {
        let command = inbox_hook_command(project)?;
        add_copilot_command_hook(&mut value, "Stop", &command);
    }
    write_copilot_hooks(path, &value)
}

fn write_json_hooks(path: &Path, value: &Value) -> Result<()> {
    let mut output = value.clone();
    remove_empty_hooks_object(&mut output);
    if value_has_no_content(&output) {
        if path.exists() {
            fs::remove_file(path)?;
        }
        return Ok(());
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, serde_json::to_string_pretty(&output)? + "\n")?;
    Ok(())
}

fn write_copilot_hooks(path: &Path, value: &Value) -> Result<()> {
    let mut output = value.clone();
    remove_empty_hooks_object(&mut output);
    if copilot_hooks_file_has_no_content(&output) {
        if path.exists() {
            fs::remove_file(path)?;
        }
        return Ok(());
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, serde_json::to_string_pretty(&output)? + "\n")?;
    Ok(())
}

fn apply_markdown_turn_rule(path: &Path, project: &str, mode: &str) -> Result<()> {
    if mode == "off" {
        if path.exists() {
            fs::remove_file(path)?;
        }
        return Ok(());
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let command = inbox_hook_command(project)?;
    fs::write(
        path,
        format!(
            "# handoff Integration Rule\n\nAfter each agent turn, check the handoff inbox. If the command reports messages, process them immediately and reply with `handoff reply <thread-id> <message>` when needed.\n\n```sh\n{command}\n```\n"
        ),
    )?;
    Ok(())
}

fn read_json_file(path: &Path) -> Result<Value> {
    if !path.exists() {
        return Ok(json!({}));
    }
    let content = fs::read_to_string(path)?;
    if content.trim().is_empty() {
        return Ok(json!({}));
    }
    serde_json::from_str(&content).with_context(|| format!("parse {}", path.display()))
}

fn remove_handoff_hooks(value: &mut Value) {
    let Some(hooks) = value.get_mut("hooks").and_then(Value::as_object_mut) else {
        return;
    };
    let keys = hooks.keys().cloned().collect::<Vec<_>>();
    for key in keys {
        let Some(entries) = hooks.get_mut(&key).and_then(Value::as_array_mut) else {
            continue;
        };
        entries.retain(|entry| !json_contains_handoff_command(entry));
    }
    hooks.retain(|_, entry| entry.as_array().is_none_or(|items| !items.is_empty()));
}

fn add_json_hook(value: &mut Value, event: &str, command: &str) {
    if !value.is_object() {
        *value = json!({});
    }
    let object = value.as_object_mut().expect("object initialized");
    let hooks = object
        .entry("hooks")
        .or_insert_with(|| Value::Object(Map::new()));
    if !hooks.is_object() {
        *hooks = Value::Object(Map::new());
    }
    let entries = hooks
        .as_object_mut()
        .expect("hooks object initialized")
        .entry(event)
        .or_insert_with(|| Value::Array(Vec::new()));
    if !entries.is_array() {
        *entries = Value::Array(Vec::new());
    }
    entries
        .as_array_mut()
        .expect("hook entries array initialized")
        .push(json!({
            "matcher": "",
            "hooks": [
                {
                    "type": "command",
                    "command": command
                }
            ]
        }));
}

fn add_copilot_command_hook(value: &mut Value, event: &str, command: &str) {
    if !value.is_object() {
        *value = json!({});
    }
    let object = value.as_object_mut().expect("object initialized");
    object.entry("version").or_insert(json!(1));
    let hooks = object
        .entry("hooks")
        .or_insert_with(|| Value::Object(Map::new()));
    if !hooks.is_object() {
        *hooks = Value::Object(Map::new());
    }
    let entries = hooks
        .as_object_mut()
        .expect("hooks object initialized")
        .entry(event)
        .or_insert_with(|| Value::Array(Vec::new()));
    if !entries.is_array() {
        *entries = Value::Array(Vec::new());
    }
    entries
        .as_array_mut()
        .expect("hook entries array initialized")
        .push(json!({
            "type": "command",
            "command": command,
            "timeoutSec": 30
        }));
}

fn copilot_hooks_file_has_no_content(value: &Value) -> bool {
    let Some(object) = value.as_object() else {
        return true;
    };
    object.is_empty()
        || (object.len() == 1
            && object
                .get("version")
                .is_some_and(|version| version.as_i64() == Some(1)))
}

fn value_has_no_hooks(value: &Value) -> bool {
    value
        .get("hooks")
        .and_then(Value::as_object)
        .is_none_or(|hooks| {
            hooks
                .values()
                .all(|entry| entry.as_array().is_none_or(Vec::is_empty))
        })
}

fn remove_empty_hooks_object(value: &mut Value) {
    let Some(object) = value.as_object_mut() else {
        return;
    };
    if value_has_no_hooks(&json!({"hooks": object.get("hooks").cloned().unwrap_or(Value::Null)})) {
        object.remove("hooks");
    }
}

fn value_has_no_content(value: &Value) -> bool {
    value.as_object().is_none_or(Map::is_empty)
}

fn json_contains_handoff_command(value: &Value) -> bool {
    match value {
        Value::String(text) => is_handoff_hook_command(text),
        Value::Array(items) => items.iter().any(json_contains_handoff_command),
        Value::Object(map) => map.values().any(json_contains_handoff_command),
        _ => false,
    }
}

fn is_handoff_hook_command(text: &str) -> bool {
    let inbox_hook = command_has_token(text, "inbox")
        && command_has_token(text, "--json")
        && command_has_token(text, "--project");
    let notify_hook = command_has_token(text, "notify")
        && command_has_token(text, "--json")
        && command_has_token(text, "--project");
    let monitor_hook = command_has_token(text, "monitor")
        && command_has_token(text, "--instruction")
        && command_has_token(text, "--runtime")
        && command_has_token(text, "--project");
    inbox_hook || notify_hook || monitor_hook
}

fn command_has_token(text: &str, token: &str) -> bool {
    text.split_whitespace().any(|part| part == token)
}

fn inbox_hook_command(project: &str) -> Result<String> {
    let exe = env::current_exe()?;
    Ok(format!(
        "'{}' notify --hook --json --project '{}'",
        shell_quote(&exe.display().to_string()),
        shell_quote(project)
    ))
}

fn monitor_instruction_hook_command(project: &str, runtime: &str) -> Result<String> {
    let exe = env::current_exe()?;
    Ok(format!(
        "'{}' monitor --instruction --runtime {} --project '{}'",
        shell_quote(&exe.display().to_string()),
        runtime,
        shell_quote(project)
    ))
}
