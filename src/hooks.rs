//! Harness hook registration and the dependency-free pre-tool dispatcher.
//!
//! Payload shims stay intentionally tiny: they hand stdin to
//! `shipmates hook gate --harness <target>`. JSON parsing, branch discovery,
//! run lookup, and native deny translation live here so every harness shares
//! one implementation and installed hooks do not require `jq`.

use anyhow::{Context, Result, bail};
use serde_json::{Value, json};
use std::fs;
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::cli::HookAction;
use crate::state::{self, GateDecision};

const HOOK_COMMANDS: &[(&str, &str)] = &[
    (
        "claude-code",
        "SHIPMATES_NATIVE_HOOK=1 bash \"${CLAUDE_PROJECT_DIR}/.claude/hooks/fsm-gate.sh\"",
    ),
    (
        "cursor",
        "SHIPMATES_NATIVE_HOOK=1 bash .cursor/hooks/fsm-gate.sh",
    ),
    (
        "codex",
        "SHIPMATES_NATIVE_HOOK=1 bash \"$(git rev-parse --show-toplevel)/.codex/hooks/fsm-gate.sh\"",
    ),
    (
        "antigravity",
        "SHIPMATES_NATIVE_HOOK=1 bash .agents/hooks/fsm-gate.sh",
    ),
    (
        "github-copilot",
        "SHIPMATES_NATIVE_HOOK=1 bash .github/hooks/fsm-gate.sh",
    ),
    (
        "windsurf",
        "SHIPMATES_NATIVE_HOOK=1 bash .windsurf/hooks/fsm-gate.sh",
    ),
];

/// Run a hook action and return its harness-native process status.
pub fn dispatch(action: &HookAction) -> i32 {
    match action {
        HookAction::Gate { harness } => dispatch_gate(harness),
        HookAction::Context { harness, event } => dispatch_context(harness, event),
        HookAction::Record { harness, event } => dispatch_record(harness, event),
        HookAction::Stop { harness } => dispatch_stop(harness),
    }
}

fn dispatch_gate(harness: &str) -> i32 {
    let mut input = String::new();
    if io::stdin().read_to_string(&mut input).is_err() {
        return 0;
    }
    let payload: Value = match serde_json::from_str(&input) {
        Ok(value) => value,
        Err(_) => return 0,
    };

    let Some(command) = hook_command(harness, &payload) else {
        return 0;
    };
    let Some(cwd) = hook_cwd(harness, &payload) else {
        return 0;
    };
    let cwd = command_worktree(&command, &cwd).unwrap_or(cwd);
    let Some(run) = discover_run(&cwd) else {
        return 0;
    };

    let run_path = cwd.join(".shipmates").join(format!("run-{run}.json"));
    if !run_path.is_file() {
        // A project that has not started a Shipmates run must remain unaffected.
        return 0;
    }

    let decision = match state::gate_for_hook(&cwd, run, &command) {
        Ok(decision) => decision,
        Err(error) => GateDecision::Error(error.reason().to_string()),
    };

    match decision {
        GateDecision::Allow => 0,
        GateDecision::Deny(reason) => emit_deny(harness, &reason),
        GateDecision::Error(reason) => {
            // Once a run is positively identified, corrupt state is not silently
            // ignored. Native hook failures are translated into a deny where the
            // harness has a decision channel, preserving normal sessions' allow-
            // on-no-run behavior without making active runs bypassable by damage.
            emit_deny(harness, &format!("shipmates hook error: {reason}"))
        }
    }
}

fn dispatch_context(harness: &str, event: &str) -> i32 {
    let mut input = String::new();
    if io::stdin().read_to_string(&mut input).is_err() {
        return 0;
    }
    let payload: Value = match serde_json::from_str(&input) {
        Ok(value) => value,
        Err(_) => return 0,
    };
    let Some(cwd) = hook_cwd(harness, &payload) else {
        return 0;
    };
    let Some(run) = discover_run(&cwd) else {
        return 0;
    };
    if !cwd
        .join(".shipmates")
        .join(format!("run-{run}.json"))
        .is_file()
    {
        return 0;
    }
    let Ok(record) = state::status_for_hook(&cwd, run) else {
        return 0;
    };
    let rounds = if record.fix_rounds.is_empty() {
        "none".to_string()
    } else {
        record
            .fix_rounds
            .iter()
            .map(|(stage, count)| format!("{stage}:{count}"))
            .collect::<Vec<_>>()
            .join(", ")
    };
    let context = format!(
        "Shipmates run #{} ({}) is at phase `{}`. Fix rounds: {}. Continue from this phase; do not reset or skip state.",
        record.issue, record.command, record.phase, rounds
    );
    match harness {
        "claude-code" | "codex" => println!(
            "{}",
            json!({
                "hookSpecificOutput": {
                    "hookEventName": event,
                    "additionalContext": context
                }
            })
        ),
        _ => println!("{}", json!({"additionalContext": context})),
    }
    0
}

fn dispatch_record(harness: &str, event: &str) -> i32 {
    let mut input = String::new();
    if io::stdin().read_to_string(&mut input).is_err() {
        return 0;
    }
    let payload: Value = match serde_json::from_str(&input) {
        Ok(value) => value,
        Err(_) => return 0,
    };
    let Some(cwd) = hook_cwd(harness, &payload) else {
        return 0;
    };
    let Some(run) = discover_run(&cwd) else {
        return 0;
    };
    if !cwd
        .join(".shipmates")
        .join(format!("run-{run}.json"))
        .is_file()
    {
        return 0;
    }
    let tool = payload
        .get("tool_name")
        .or_else(|| payload.get("toolName"))
        .or_else(|| payload.get("tool"))
        .and_then(Value::as_str)
        .map(str::to_string);
    let _ = state::record_hook_event(&cwd, run, event, tool.as_deref());
    0
}

fn dispatch_stop(harness: &str) -> i32 {
    let mut input = String::new();
    if io::stdin().read_to_string(&mut input).is_err() {
        return 0;
    }
    let payload: Value = match serde_json::from_str(&input) {
        Ok(value) => value,
        Err(_) => return 0,
    };
    if payload
        .get("stop_hook_active")
        .or_else(|| payload.get("stopHookActive"))
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        return 0;
    }
    let Some(cwd) = hook_cwd(harness, &payload) else {
        return 0;
    };
    let Some(run) = discover_run(&cwd) else {
        return 0;
    };
    if !cwd
        .join(".shipmates")
        .join(format!("run-{run}.json"))
        .is_file()
    {
        return 0;
    }
    let record = match state::status_for_hook(&cwd, run) {
        Ok(record) => record,
        Err(error) => {
            println!(
                "{}",
                json!({
                    "decision": "block",
                    "reason": format!("Shipmates run state is invalid: {}", error.reason())
                })
            );
            return 0;
        }
    };
    let finished = record.phase == state::PHASE_COMPLETE
        || record.phase == state::PHASE_ESCALATED
        || (record.phase == "deliver"
            && matches!(
                state::gate_for_hook(&cwd, run, "gh pr merge"),
                Ok(GateDecision::Allow)
            ));
    if finished {
        return 0;
    }
    let reason = format!(
        "Shipmates run #{} is still at phase `{}`; continue the run or escalate it before stopping.",
        record.issue, record.phase
    );
    println!("{}", json!({"decision": "block", "reason": reason}));
    0
}

fn hook_command(harness: &str, payload: &Value) -> Option<String> {
    let tool = payload
        .get("tool_name")
        .or_else(|| payload.get("toolName"))
        .or_else(|| payload.get("tool"))
        .and_then(Value::as_str)
        .unwrap_or_default();

    let shell_only = matches!(harness, "cursor" | "windsurf");
    if !shell_only {
        let allowed = match harness {
            "claude-code" => tool == "Bash",
            "codex" => matches!(tool, "Bash" | "bash" | "shell" | "local_shell" | "exec"),
            "antigravity" => matches!(
                tool,
                "run_command" | "run_terminal_command" | "shell" | "bash"
            ),
            "github-copilot" => matches!(tool, "execute" | "shell" | "bash" | "Bash"),
            _ => false,
        };
        if !allowed {
            return None;
        }
    }

    let command = payload
        .get("command")
        .or_else(|| payload.get("tool_input").and_then(|v| v.get("command")))
        .or_else(|| payload.get("toolArgs").and_then(|v| v.get("command")))
        .or_else(|| payload.get("tool_info").and_then(|v| v.get("command_line")))
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())?;
    Some(command.to_string())
}

fn hook_cwd(harness: &str, payload: &Value) -> Option<PathBuf> {
    let raw_cwd = payload
        .get("cwd")
        .or_else(|| payload.get("tool_input").and_then(|v| v.get("cwd")))
        .or_else(|| payload.get("tool_info").and_then(|v| v.get("cwd")))
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from);

    let raw_cwd = if let Some(cwd) = raw_cwd {
        cwd
    } else if harness == "cursor" {
        payload
            .get("workspace_roots")
            .and_then(Value::as_array)
            .and_then(|roots| roots.first())
            .and_then(Value::as_str)
            .map(PathBuf::from)?
    } else {
        std::env::current_dir().ok()?
    };
    resolve_repo_root(&raw_cwd).or(Some(raw_cwd))
}

fn resolve_repo_root(cwd: &Path) -> Option<PathBuf> {
    let output = Command::new("git")
        .args(["-C", cwd.to_str()?, "rev-parse", "--show-toplevel"])
        .output()
        .ok()
        .filter(|output| output.status.success())?;
    let root = String::from_utf8_lossy(&output.stdout).trim().to_string();
    (!root.is_empty()).then(|| PathBuf::from(root))
}

fn command_worktree(command: &str, fallback: &Path) -> Option<PathBuf> {
    let rest = command
        .split_once("git -C ")
        .map(|(_, rest)| rest)
        .or_else(|| command.split_once("cd ").map(|(_, rest)| rest))?
        .trim_start();
    let (raw, _) = if let Some(rest) = rest.strip_prefix('"') {
        rest.split_once('"')?
    } else if let Some(rest) = rest.strip_prefix('\'') {
        rest.split_once('\'')?
    } else {
        (rest.split_whitespace().next()?, "")
    };
    if raw.starts_with('$') || raw.is_empty() {
        return None;
    }
    let path = PathBuf::from(raw);
    let path = if path.is_absolute() {
        path
    } else {
        fallback.join(path)
    };
    resolve_repo_root(&path).or(Some(path))
}

fn discover_run(cwd: &Path) -> Option<u64> {
    let branch = Command::new("git")
        .args(["-C", cwd.to_str()?, "rev-parse", "--abbrev-ref", "HEAD"])
        .output()
        .ok()
        .filter(|output| output.status.success())
        .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_string())?;

    for prefix in ["feat/issue-", "feat/bundle-"] {
        let Some(rest) = branch.strip_prefix(prefix) else {
            continue;
        };
        let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
        if digits.is_empty() {
            continue;
        }
        let suffix = &rest[digits.len()..];
        if suffix.is_empty() || suffix.starts_with('-') {
            return digits.parse().ok();
        }
    }
    None
}

fn emit_deny(harness: &str, reason: &str) -> i32 {
    match harness {
        "claude-code" => {
            println!(
                "{}",
                json!({
                    "hookSpecificOutput": {
                        "hookEventName": "PreToolUse",
                        "permissionDecision": "deny",
                        "permissionDecisionReason": reason
                    }
                })
            );
            0
        }
        "codex" => {
            println!(
                "{}",
                json!({
                    "hookSpecificOutput": {
                        "hookEventName": "PreToolUse",
                        "permissionDecision": "deny",
                        "permissionDecisionReason": reason
                    }
                })
            );
            0
        }
        "github-copilot" => {
            println!(
                "{}",
                json!({
                    "permissionDecision": "deny",
                    "permissionDecisionReason": reason
                })
            );
            0
        }
        "antigravity" => {
            println!("{}", json!({"decision": "deny", "reason": reason}));
            0
        }
        "cursor" => {
            println!("{{\"permission\":\"deny\"}}");
            eprintln!("shipmates hook denied: {reason}");
            2
        }
        "windsurf" => {
            eprintln!("shipmates hook denied: {reason}");
            2
        }
        _ => 0,
    }
}

/// Register a harness's pre-tool shim without overwriting user hooks.
pub fn register(target_dir: &Path, harness: &str) -> Result<Vec<PathBuf>> {
    match harness {
        "claude-code" => {
            let path = target_dir.join(".claude/settings.json");
            let mut paths = register_group_json(
                &path,
                "PreToolUse",
                json!({
                    "matcher": "Bash",
                    "hooks": [{"type": "command", "command": HOOK_COMMANDS[0].1, "timeout": 30}]
                }),
            )?;
            for event in ["SessionStart", "SubagentStart"] {
                paths.extend(register_group_json(
                    &path,
                    event,
                    json!({
                        "hooks": [{
                            "type": "command",
                            "command": format!("shipmates hook context --harness claude-code --event {event}"),
                            "timeout": 10
                        }]
                    }),
                )?);
            }
            paths.extend(register_group_json(
                &path,
                "PreCompact",
                json!({
                    "hooks": [{
                        "type": "command",
                        "command": "shipmates hook record --harness claude-code --event PreCompact",
                        "timeout": 10
                    }]
                }),
            )?);
            for (event, matcher) in [("PostToolUse", "Bash|Edit|Write"), ("SubagentStop", "")] {
                paths.extend(register_group_json(
                    &path,
                    event,
                    json!({
                        "matcher": matcher,
                        "hooks": [{
                            "type": "command",
                            "command": format!("shipmates hook record --harness claude-code --event {event}"),
                            "timeout": 10
                        }]
                    }),
                )?);
            }
            paths.extend(register_group_json(
                &path,
                "Stop",
                json!({
                    "hooks": [{"type": "command", "command": "shipmates hook stop --harness claude-code", "timeout": 10}]
                }),
            )?);
            Ok(paths)
        }
        "cursor" => register_direct_json(
            &target_dir.join(".cursor/hooks.json"),
            "beforeShellExecution",
            json!({"command": HOOK_COMMANDS[1].1, "failClosed": true, "timeout": 30}),
            true,
        ),
        "codex" => {
            let mut paths = register_group_json(
                &target_dir.join(".codex/hooks.json"),
                "PreToolUse",
                json!({
                    "matcher": "Bash",
                    "hooks": [{"type": "command", "command": HOOK_COMMANDS[2].1, "timeout": 30}]
                }),
            )?;
            let config = target_dir.join(".codex/hooks.json");
            for event in ["SessionStart", "SubagentStart"] {
                paths.extend(register_group_json(
                    &config,
                    event,
                    json!({
                        "hooks": [{
                            "type": "command",
                            "command": format!("shipmates hook context --harness codex --event {event}"),
                            "timeout": 10
                        }]
                    }),
                )?);
            }
            paths.extend(register_group_json(
                &config,
                "PreCompact",
                json!({
                    "hooks": [{
                        "type": "command",
                        "command": "shipmates hook record --harness codex --event PreCompact",
                        "timeout": 10
                    }]
                }),
            )?);
            for event in ["PostToolUse", "SubagentStop"] {
                paths.extend(register_group_json(
                    &config,
                    event,
                    json!({
                        "hooks": [{
                            "type": "command",
                            "command": format!("shipmates hook record --harness codex --event {event}"),
                            "timeout": 10
                        }]
                    }),
                )?);
            }
            paths.extend(register_group_json(
                &config,
                "Stop",
                json!({
                    "hooks": [{"type": "command", "command": "shipmates hook stop --harness codex", "timeout": 10}]
                }),
            )?);
            let config = target_dir.join(".codex/config.toml");
            if enable_codex_hooks(&config)? {
                paths.push(config);
            }
            Ok(paths)
        }
        "antigravity" => register_group_json(
            &target_dir.join(".agents/hooks.json"),
            "PreToolUse",
            json!({
                "matcher": "run_command",
                "hooks": [{"type": "command", "command": HOOK_COMMANDS[3].1, "timeout": 30}]
            }),
        ),
        "github-copilot" => register_direct_json(
            &target_dir.join(".github/hooks/shipmates-fsm-gate.json"),
            "preToolUse",
            json!({"type": "command", "bash": HOOK_COMMANDS[4].1, "timeoutSec": 30}),
            true,
        ),
        "windsurf" => register_direct_json(
            &target_dir.join(".windsurf/hooks.json"),
            "pre_run_command",
            json!({"command": HOOK_COMMANDS[5].1, "show_output": false}),
            false,
        ),
        // Local opencode plugins are discovered automatically from the plural
        // `.opencode/plugins/` directory; no config mutation is required.
        "opencode" => Ok(Vec::new()),
        other => bail!("unsupported hook target: {other}"),
    }
}

/// Return whether install-time registration is present and points at Shipmates.
pub fn is_registered(target_dir: &Path, harness: &str) -> Result<bool> {
    match harness {
        "opencode" => Ok(target_dir.join(".opencode/plugins/fsm-gate.ts").is_file()),
        "codex" => Ok(shell_registered(
            target_dir,
            ".codex/hooks.json",
            HOOK_COMMANDS[2].1,
            ".codex/hooks/fsm-gate.sh",
        )? && codex_hooks_enabled(&target_dir.join(".codex/config.toml"))?),
        "claude-code" => shell_registered(
            target_dir,
            ".claude/settings.json",
            HOOK_COMMANDS[0].1,
            ".claude/hooks/fsm-gate.sh",
        ),
        "cursor" => shell_registered(
            target_dir,
            ".cursor/hooks.json",
            HOOK_COMMANDS[1].1,
            ".cursor/hooks/fsm-gate.sh",
        ),
        "antigravity" => shell_registered(
            target_dir,
            ".agents/hooks.json",
            HOOK_COMMANDS[3].1,
            ".agents/hooks/fsm-gate.sh",
        ),
        "github-copilot" => shell_registered(
            target_dir,
            ".github/hooks/shipmates-fsm-gate.json",
            HOOK_COMMANDS[4].1,
            ".github/hooks/fsm-gate.sh",
        ),
        "windsurf" => shell_registered(
            target_dir,
            ".windsurf/hooks.json",
            HOOK_COMMANDS[5].1,
            ".windsurf/hooks/fsm-gate.sh",
        ),
        other => bail!("unsupported hook target: {other}"),
    }
}

fn register_group_json(path: &Path, event: &str, group: Value) -> Result<Vec<PathBuf>> {
    let mut root = read_json_object(path)?;
    let hooks = ensure_object(&mut root, "hooks", path)?;
    let events = ensure_array(hooks, event, path)?;
    if !array_contains_command(events, &group) {
        events.push(group);
        write_json(path, &root)?;
        return Ok(vec![path.to_path_buf()]);
    }
    Ok(Vec::new())
}

fn register_direct_json(
    path: &Path,
    event: &str,
    entry: Value,
    include_version: bool,
) -> Result<Vec<PathBuf>> {
    let mut root = read_json_object(path)?;
    if include_version && !root.get("version").is_some() {
        root["version"] = json!(1);
    }
    let hooks = ensure_object(&mut root, "hooks", path)?;
    let events = ensure_array(hooks, event, path)?;
    if !array_contains_command(events, &entry) {
        events.push(entry);
        write_json(path, &root)?;
        return Ok(vec![path.to_path_buf()]);
    }
    Ok(Vec::new())
}

fn read_json_object(path: &Path) -> Result<Value> {
    if !path.exists() {
        return Ok(json!({}));
    }
    let text = fs::read_to_string(path)
        .with_context(|| format!("reading hook config {}", path.display()))?;
    let value: Value = serde_json::from_str(&text)
        .with_context(|| format!("parsing hook config {}", path.display()))?;
    if !value.is_object() {
        bail!("hook config {} must contain a JSON object", path.display());
    }
    Ok(value)
}

fn ensure_object<'a>(
    root: &'a mut Value,
    key: &str,
    path: &Path,
) -> Result<&'a mut serde_json::Map<String, Value>> {
    let root_object = root
        .as_object_mut()
        .ok_or_else(|| anyhow::anyhow!("hook config {} must be an object", path.display()))?;
    if !root_object.contains_key(key) {
        root_object.insert(key.to_string(), json!({}));
    }
    root_object[key].as_object_mut().ok_or_else(|| {
        anyhow::anyhow!(
            "hook config {} field {key:?} must be an object",
            path.display()
        )
    })
}

fn ensure_array<'a>(
    object: &'a mut serde_json::Map<String, Value>,
    key: &str,
    path: &Path,
) -> Result<&'a mut Vec<Value>> {
    if !object.contains_key(key) {
        object.insert(key.to_string(), json!([]));
    }
    object[key].as_array_mut().ok_or_else(|| {
        anyhow::anyhow!(
            "hook config {} event {key:?} must be an array",
            path.display()
        )
    })
}

fn array_contains_command(entries: &[Value], wanted: &Value) -> bool {
    let wanted_command = command_from_entry(wanted);
    let Some(wanted_command) = wanted_command else {
        return false;
    };
    entries.iter().any(|entry| {
        if command_from_entry(entry) == Some(wanted_command) {
            return true;
        }
        false
    })
}

fn command_from_entry(entry: &Value) -> Option<&str> {
    entry
        .get("command")
        .and_then(Value::as_str)
        .or_else(|| entry.get("bash").and_then(Value::as_str))
        .or_else(|| {
            entry
                .get("hooks")
                .and_then(Value::as_array)
                .and_then(|nested| nested.iter().find_map(command_from_entry))
        })
}

fn write_json(path: &Path, value: &Value) -> Result<()> {
    let mut text = serde_json::to_string_pretty(value)?;
    text.push('\n');
    crate::installer::atomic_write(path, &text)?;
    Ok(())
}

fn has_command(path: &Path, command: &str) -> Result<bool> {
    if !path.exists() {
        return Ok(false);
    }
    let root = read_json_object(path)?;
    Ok(root
        .get("hooks")
        .and_then(Value::as_object)
        .is_some_and(|events| {
            events.values().any(|entries| {
                entries.as_array().is_some_and(|entries| {
                    entries.iter().any(|entry| {
                        entry.get("command").and_then(Value::as_str) == Some(command)
                            || entry.get("bash").and_then(Value::as_str) == Some(command)
                            || entry
                                .get("hooks")
                                .and_then(Value::as_array)
                                .is_some_and(|nested| {
                                    nested.iter().any(|hook| {
                                        hook.get("command").and_then(Value::as_str) == Some(command)
                                    })
                                })
                    })
                })
            })
        }))
}

fn shell_registered(target_dir: &Path, config: &str, command: &str, shim: &str) -> Result<bool> {
    Ok(has_command(&target_dir.join(config), command)? && target_dir.join(shim).is_file())
}

fn enable_codex_hooks(path: &Path) -> Result<bool> {
    let old = if path.exists() {
        fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?
    } else {
        String::new()
    };
    let mut lines: Vec<String> = old.lines().map(str::to_string).collect();
    let feature_start = lines.iter().position(|line| line.trim() == "[features]");
    let mut found = false;
    if let Some(start) = feature_start {
        let end = lines
            .iter()
            .enumerate()
            .skip(start + 1)
            .find(|(_, line)| line.trim().starts_with('['))
            .map(|(index, _)| index)
            .unwrap_or(lines.len());
        for line in &mut lines[start + 1..end] {
            let trimmed = line.trim();
            let key = trimmed.split_once('=').map(|(key, _)| key.trim());
            if matches!(key, Some("hooks") | Some("codex_hooks")) {
                *line = "hooks = true".to_string();
                found = true;
            }
        }
        if !found {
            lines.insert(end, "hooks = true".to_string());
        }
    } else {
        if !lines.is_empty() && lines.last().is_some_and(|line| !line.is_empty()) {
            lines.push(String::new());
        }
        lines.push("[features]".to_string());
        lines.push("hooks = true".to_string());
    }
    let mut next = lines.join("\n");
    next.push('\n');
    if next == old {
        return Ok(false);
    }
    crate::installer::atomic_write(path, &next)?;
    Ok(true)
}

fn codex_hooks_enabled(path: &Path) -> Result<bool> {
    if !path.exists() {
        return Ok(false);
    }
    let text = fs::read_to_string(path)?;
    let mut in_features = false;
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') {
            in_features = trimmed == "[features]";
        } else if in_features && trimmed == "hooks = true" {
            return Ok(true);
        }
    }
    Ok(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::installer::atomic_write;
    use tempfile::tempdir;

    #[test]
    fn test_registration_preserves_existing_claude_hooks_and_is_idempotent() {
        let dir = tempdir().unwrap();
        let path = dir.path().join(".claude/settings.json");
        atomic_write(
            &path,
            r#"{"hooks":{"PostToolUse":[{"hooks":[]}]},"custom":true}"#,
        )
        .unwrap();

        let first = register(dir.path(), "claude-code").unwrap();
        let second = register(dir.path(), "claude-code").unwrap();
        let root: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(first.len(), 7);
        assert!(first.iter().all(|registered| registered == &path));
        assert!(second.is_empty());
        assert_eq!(root["custom"], true);
        assert_eq!(root["hooks"]["PreToolUse"].as_array().unwrap().len(), 1);
        atomic_write(&dir.path().join(".claude/hooks/fsm-gate.sh"), "#!/bin/sh\n").unwrap();
        assert!(is_registered(dir.path(), "claude-code").unwrap());
    }

    #[test]
    fn test_codex_registration_enables_hooks_without_duplicate_features() {
        let dir = tempdir().unwrap();
        let config = dir.path().join(".codex/config.toml");
        atomic_write(&config, "[features]\nother = true\n").unwrap();
        register(dir.path(), "codex").unwrap();
        register(dir.path(), "codex").unwrap();
        let text = fs::read_to_string(config).unwrap();
        assert_eq!(text.matches("[features]").count(), 1);
        assert_eq!(text.matches("hooks = true").count(), 1);
        atomic_write(&dir.path().join(".codex/hooks/fsm-gate.sh"), "#!/bin/sh\n").unwrap();
        assert!(is_registered(dir.path(), "codex").unwrap());

        atomic_write(
            &dir.path().join(".codex/config.toml"),
            "[features]\nhooks=true\n",
        )
        .unwrap();
        register(dir.path(), "codex").unwrap();
        let text = fs::read_to_string(dir.path().join(".codex/config.toml")).unwrap();
        assert_eq!(text.matches("hooks = true").count(), 1);
    }

    #[test]
    fn test_run_discovery_accepts_single_and_bundle_branches_only() {
        assert!(discover_run_from_branch("feat/issue-42-slug") == Some(42));
        assert!(discover_run_from_branch("feat/bundle-42-many") == Some(42));
        assert!(discover_run_from_branch("feat/issue-42x") == None);
        assert!(discover_run_from_branch("main") == None);
    }

    fn discover_run_from_branch(branch: &str) -> Option<u64> {
        for prefix in ["feat/issue-", "feat/bundle-"] {
            let Some(rest) = branch.strip_prefix(prefix) else {
                continue;
            };
            let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
            if digits.is_empty() {
                continue;
            }
            let suffix = &rest[digits.len()..];
            if suffix.is_empty() || suffix.starts_with('-') {
                return digits.parse().ok();
            }
        }
        None
    }
}
