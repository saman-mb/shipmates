use shipmates::adapters::Adapter;
use shipmates::adapters::antigravity::AntigravityAdapter;
use shipmates::adapters::claude_code::ClaudeCodeAdapter;
use shipmates::adapters::codex::CodexAdapter;
use shipmates::adapters::opencode::OpencodeAdapter;
use shipmates::catalog::{reject_positional, CanonicalCommand, CanonicalRole};
use shipmates::digest;
use shipmates::hooks;
use std::path::PathBuf;

#[test]
fn test_claude_code_payload_digest() {
    let role = CanonicalRole {
        name: "test-role".into(),
        description: "A test role".into(),
        capabilities: vec![],
        writes: false,
        web_scopes: vec![],
        read_scopes: vec![],
        tool_order: vec![],
        effort: None,
        source: PathBuf::from("test.md"),
        body: "body content".into(),
    };
    let files = ClaudeCodeAdapter.build(&[role], &[]).unwrap();
    let content = files.get("harnesses/claude-code/.claude/agents/test-role.md").unwrap();
    let hashed = digest::hash(content);
    assert_eq!(hashed, "491b209dc45c12fd8b89e113ba775ca5c6c03b0b977c868427cdbf22e0705209");
}

#[test]
fn test_opencode_payload_digest() {
    let command = CanonicalCommand {
        name: "test-cmd".into(),
        description: "Test cmd".into(),
        argument_hint: "".into(),
        allowed_tools: "".into(),
        disable_model_invocation: true,
        arguments: vec![],
        loop_max: 1,
        stages: vec![],
        tool_gates: vec![],
        narrative: "narrative".into(),
        invocation: "invoke".into(),
        board: "board".into(),
        source: PathBuf::from("cmd.md"),
    };
    let files = OpencodeAdapter.build(&[], &[command]).unwrap();
    let content = files.get("harnesses/opencode/.opencode/commands/test-cmd.md").unwrap();
    let hashed = digest::hash(content);
    assert_eq!(hashed, "d7f5ef7b388b4472f7005bd7788b93ff8a637cc2e889528f8a505af60d3fbe5f");
}

#[test]
fn test_opencode_permissions_deny_first() {
    let role = CanonicalRole {
        name: "test-role".into(),
        description: "A test role".into(),
        capabilities: vec!["read".into()],
        writes: false,
        web_scopes: vec![],
        read_scopes: vec![],
        tool_order: vec![],
        effort: None,
        source: PathBuf::from("test.md"),
        body: "body content".into(),
    };
    let files = OpencodeAdapter.build(&[role], &[]).unwrap();
    let content = files.get("harnesses/opencode/.opencode/agents/test-role.md").unwrap();
    assert!(content.starts_with("---\n"));
    assert!(content.contains("  \"*\": deny\n"));
    assert!(content.contains("  read: allow\n"));
}

#[test]
fn test_positional_args_rejected() {
    let result = reject_positional("test", "some text with $1 here");
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(err.contains("a command has no positional arguments"));

    let ok_result = reject_positional("test", "some text with \\$1 here");
    assert!(ok_result.is_ok());
}

#[test]
fn test_cli_targets() {
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_shipmates"))
        .arg("targets")
        .output()
        .expect("failed to execute shipmates CLI binary");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    for target in [
        "claude-code",
        "opencode",
        "antigravity",
        "codex",
        "cursor",
        "github-copilot",
        "windsurf",
    ] {
        assert!(stdout.contains(target), "targets output missing {target}");
    }
}

#[test]
fn test_non_claude_targets_build_via_cli() {
    let temp_dir = tempfile::tempdir().unwrap();
    for target in ["codex", "cursor", "github-copilot", "windsurf"] {
        let output = std::process::Command::new(env!("CARGO_BIN_EXE_shipmates"))
            .args(["build", "--target", target, "--out", temp_dir.path().to_str().unwrap()])
            .output()
            .expect("failed to execute shipmates build");
        assert!(output.status.success(), "{target} build failed: {}", String::from_utf8_lossy(&output.stderr));
    }
    // Every harness that reads the open Agent Skills tree ships its skills to the
    // shared `.agents/skills/` location, not a harness-private one.
    let codex_skill = temp_dir.path().join("harnesses/codex/.agents/skills/ship-issue/SKILL.md");
    assert!(codex_skill.is_file(), "codex ship-issue skill not emitted");
    let copilot_skill = temp_dir.path().join("harnesses/github-copilot/.agents/skills/ship-issue/SKILL.md");
    assert!(copilot_skill.is_file(), "copilot ship-issue skill not emitted");
    // ...and the shared rendering is byte-identical across those harnesses.
    let codex_bytes = std::fs::read(&codex_skill).unwrap();
    let copilot_bytes = std::fs::read(&copilot_skill).unwrap();
    assert_eq!(codex_bytes, copilot_bytes, "shared skill must be identical across harnesses");
}

#[test]
fn test_codex_adapter_renders_dialect() {
    let command = CanonicalCommand {
        name: "onboard".into(),
        description: "Onboard".into(),
        argument_hint: "".into(),
        allowed_tools: "".into(),
        disable_model_invocation: true,
        arguments: vec![],
        loop_max: 1,
        stages: vec![],
        tool_gates: vec![],
        narrative: "Write `TARGET.md` if one exists, else `AGENTS.md`; resolve via `agent-files/*.md`; use {{repo}}."
            .into(),
        invocation: "invoke".into(),
        board: "board".into(),
        source: PathBuf::from("cmd.md"),
    };
    let files = CodexAdapter.build(&[], &[command]).unwrap();
    let content = files.get("harnesses/codex/.agents/skills/onboard/SKILL.md").unwrap();
    assert!(content.contains("`AGENTS.md` if one exists, else `CLAUDE.md`"));
    // Shared neutral dialect: crew glob is the open `.agents/agents`, not `.codex/`.
    assert!(content.contains(".agents/agents/*.md"));
    assert!(!content.contains(".codex/agents/*.md"));
    assert!(content.contains("$ARGUMENTS"));
    assert!(!content.contains("TARGET.md"));
    assert!(!content.contains("agent-files/"));
    assert!(!content.contains("{{repo}}"));
}

#[test]
fn test_cli_build_and_install() {
    let temp_dir = tempfile::tempdir().unwrap();
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_shipmates"))
        .args(["install", "--harness", "claude-code", "--dir", temp_dir.path().to_str().unwrap()])
        .output()
        .expect("failed to execute shipmates install");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Installed harness: claude-code"));
}

#[test]
fn test_hook_registration_events_are_idempotent_and_preserve_user_config() {
    let temp_dir = tempfile::tempdir().unwrap();
    let settings = temp_dir.path().join(".claude/settings.json");
    shipmates::installer::atomic_write(
        &settings,
        r#"{"custom":true,"hooks":{"PostToolUse":[{"hooks":[]}]}}"#,
    )
    .unwrap();

    hooks::register(temp_dir.path(), "claude-code").unwrap();
    hooks::register(temp_dir.path(), "claude-code").unwrap();
    let config: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(settings).unwrap()).unwrap();
    assert_eq!(config["custom"], true);
    for event in [
        "PreToolUse",
        "SessionStart",
        "PreCompact",
        "SubagentStart",
        "PostToolUse",
        "SubagentStop",
        "Stop",
    ] {
        let expected = if event == "PostToolUse" { 2 } else { 1 };
        assert_eq!(config["hooks"][event].as_array().unwrap().len(), expected, "{event}");
    }
}

/// No adapter may stamp a model into a crew agent file — a model is a runtime
/// decision the orchestrator makes at spawn (#205), so an install-time value
/// would be wrong across harnesses and user access tiers. Effort (#204) IS
/// emitted, so the guard uses line-PREFIX checks per dialect: YAML/MD targets
/// must have no line starting `model:` (so `reasoningEffort:`/`effort:` don't
/// trip it), and the codex TOML no line starting `model =` (so
/// `model_reasoning_effort =` doesn't trip it).
#[test]
fn test_no_adapter_emits_a_model_line() {
    let role = || CanonicalRole {
        name: "architect".into(),
        description: "A test role".into(),
        capabilities: vec!["read".into(), "bash".into()],
        writes: false,
        web_scopes: vec![],
        read_scopes: vec![],
        tool_order: vec![],
        effort: Some("high".into()),
        source: PathBuf::from("architect.md"),
        body: "system prompt body".into(),
    };

    // Iterate every shipped target rather than a hardcoded list, so a future
    // crew-bearing adapter (cursor, #34) is auto-covered. Skills-only targets
    // emit no agent files and are skipped. The two prefix checks span both
    // dialects: `model:` (YAML/MD frontmatter) and `model = ` (codex TOML) —
    // neither trips on `reasoningEffort:`/`effort:` or `model_reasoning_effort =`.
    for target in shipmates::adapters::targets() {
        let files = shipmates::adapters::select(target).unwrap().build(&[role()], &[]).unwrap();
        for (path, content) in &files {
            if !path.contains("/agents/") {
                continue;
            }
            assert!(
                !content.lines().any(|l| l.trim_start().starts_with("model:")),
                "{target} agent file {path} emitted a model line:\n{content}"
            );
            assert!(
                !content.lines().any(|l| l.trim_start().starts_with("model = ")),
                "{target} agent file {path} emitted a model line:\n{content}"
            );
        }
    }
}

#[test]
fn test_antigravity_adapter_integration() {
    let role = CanonicalRole {
        name: "architect".into(),
        description: "Architect role".into(),
        capabilities: vec!["read".into()],
        writes: false,
        web_scopes: vec![],
        read_scopes: vec![],
        tool_order: vec![],
        effort: None,
        source: PathBuf::from("architect.md"),
        body: "system prompt body".into(),
    };
    let files = AntigravityAdapter.build(&[role], &[]).unwrap();
    let content = files.get("harnesses/antigravity/.agents/agents/architect.md").unwrap();
    assert!(content.contains("name: architect"));
    assert!(content.contains("subagent: true"));
    assert!(content.contains("system prompt body"));
}

/// Every harness's `agents` flag must match what its adapter actually emits.
///
/// This is the gate the change that added it was fixing the absence of: five
/// entries sat at `agents: false` for months, three of them wrong, because
/// nothing compared the claim to the payload. Prose in `agents_notes` makes the
/// next audit easier but cannot prevent the drift — only this can.
///
/// It also separates the two states that got conflated: "this target has no
/// crew mechanism" and "this adapter forgot to emit crew" look identical from
/// the outside, and one of them is a bug.
#[test]
fn test_matrix_agents_flag_matches_adapter_output() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let matrix: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(root.join("tools/harness_matrix.json")).unwrap()).unwrap();
    let harnesses = matrix["harnesses"].as_object().expect("harness_matrix.json has no harnesses map");

    let declared: std::collections::BTreeSet<&str> = harnesses.keys().map(|k| k.as_str()).collect();
    let shipped: std::collections::BTreeSet<&str> = shipmates::adapters::targets().into_iter().collect();
    assert_eq!(declared, shipped, "harness_matrix.json and adapters::targets() disagree");

    let temp_dir = tempfile::tempdir().unwrap();
    for (name, entry) in harnesses {
        let claims_agents = entry["agents"].as_bool().unwrap_or_else(|| panic!("{name}: no `agents` boolean"));
        assert!(
            entry["agents_notes"].as_str().is_some_and(|s| !s.trim().is_empty()),
            "{name}: `agents` must carry `agents_notes` recording the evidence — a bare flag is how              three harnesses stayed wrong",
        );

        let out = temp_dir.path().join(name);
        let status = std::process::Command::new(env!("CARGO_BIN_EXE_shipmates"))
            .args(["build", "--target", name, "--out", out.to_str().unwrap()])
            .status()
            .expect("failed to execute shipmates build");
        assert!(status.success(), "{name}: build failed");

        let emits_agents = walk(&out).iter().any(|p| {
            p.components().any(|c| c.as_os_str() == "agents") && p.file_name().is_some_and(|f| f != "AGENTS.md")
        });
        assert_eq!(
            claims_agents, emits_agents,
            "{name}: harness_matrix.json says agents={claims_agents} but the adapter emits agents={emits_agents}",
        );
    }
}

/// Every harness's `effort` flag must match what its adapter actually emits —
/// the same drift guard the `agents` flag gets, so the new #204 feature-support
/// claim can't rot into pure documentation. A `true` flag ⇒ at least one crew
/// agent carries a reasoning-effort key; `false` ⇒ none do. This is also the
/// negative test for antigravity/github-copilot/cursor/windsurf: their
/// `false` is now enforced against emission, not just asserted in prose.
#[test]
fn test_matrix_effort_flag_matches_adapter_output() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let matrix: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(root.join("tools/harness_matrix.json")).unwrap()).unwrap();
    let harnesses = matrix["harnesses"].as_object().expect("harness_matrix.json has no harnesses map");

    // Detect a reasoning-effort key across every dialect: claude-code's
    // `effort:` line, codex's `model_reasoning_effort` TOML key, opencode's
    // top-level `reasoningEffort`.
    fn carries_effort(content: &str) -> bool {
        content.lines().any(|l| l.trim_start().starts_with("effort:"))
            || content.contains("model_reasoning_effort")
            || content.contains("reasoningEffort")
    }

    let temp_dir = tempfile::tempdir().unwrap();
    for name in shipmates::adapters::targets() {
        let entry = &harnesses[name];
        let claims_effort = entry["effort"].as_bool().unwrap_or_else(|| panic!("{name}: no `effort` boolean"));
        assert!(
            entry["effort_notes"].as_str().is_some_and(|s| !s.trim().is_empty()),
            "{name}: `effort` must carry `effort_notes` recording the evidence — a bare flag is how three harnesses' `agents` claims stayed wrong",
        );

        let out = temp_dir.path().join(name);
        let status = std::process::Command::new(env!("CARGO_BIN_EXE_shipmates"))
            .args(["build", "--target", name, "--out", out.to_str().unwrap()])
            .status()
            .expect("failed to execute shipmates build");
        assert!(status.success(), "{name}: build failed");

        let emits_effort = walk(&out).iter().any(|p| {
            let is_agent = p.components().any(|c| c.as_os_str() == "agents")
                && p.file_name().is_some_and(|f| f != "AGENTS.md");
            is_agent && std::fs::read_to_string(p).is_ok_and(|c| carries_effort(&c))
        });
        assert_eq!(
            claims_effort, emits_effort,
            "{name}: harness_matrix.json says effort={claims_effort} but the adapter emits effort={emits_effort}",
        );
    }
}

/// The `lifecycle_events` matrix must stay internally consistent and honest —
/// the same drift/evidence discipline the `agents`/`effort` flags get, applied
/// to the per-event × per-harness hook capability grid (#239). This is the
/// foundation a feature-aware adapter (epic #113) will consult to emit a hook
/// only where the target actually supports it.
///
/// It asserts the schema the JSON's `_schema` note documents: every event over
/// every shipped target, `supported:true` cells carry a channel + blocking
/// boolean + notes, `supported:false` cells carry a policy + notes, and EVERY
/// cell carries `verified` (a date string or literal `false`) so "mark
/// unverified, don't guess" is operationalized rather than trusted.
///
/// Only the `PreToolUse` row is cross-checked against a real adapter emission:
/// `emit_hook_shim` is the sole event with a shipped hook today, so its channel
/// per harness is pinned here. The other ten events have no emission yet
/// (epic #113), so there is nothing to cross-check them against — the matrix
/// records their capability, not shipped behaviour.
#[test]
fn test_matrix_lifecycle_events_matrix_is_consistent() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let matrix: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(root.join("tools/harness_matrix.json")).unwrap()).unwrap();

    let events = matrix["lifecycle_events"]
        .as_object()
        .expect("harness_matrix.json has no lifecycle_events map");

    let shipped: std::collections::BTreeSet<&str> = shipmates::adapters::targets().into_iter().collect();

    fn is_verified_date(value: &serde_json::Value) -> bool {
        let Some(s) = value.as_str() else { return false };
        let bytes = s.as_bytes();
        bytes.len() == 10
            && bytes[4] == b'-'
            && bytes[7] == b'-'
            && bytes
                .iter()
                .enumerate()
                .all(|(i, b)| i == 4 || i == 7 || b.is_ascii_digit())
    }

    // Every lifecycle event we claim to have researched must be present.
    let expected_events = [
        "PreToolUse",
        "SessionStart",
        "SessionEnd",
        "PostToolUse",
        "PostToolUseFailure",
        "Stop",
        "SubagentStart",
        "SubagentStop",
        "PreCompact",
        "Notification",
        "UserPromptSubmit",
    ];
    for event in expected_events {
        assert!(events.contains_key(event), "lifecycle_events missing event `{event}`");

        let harnesses = events[event]["harnesses"]
            .as_object()
            .unwrap_or_else(|| panic!("{event}: no `harnesses` map"));
        let declared: std::collections::BTreeSet<&str> = harnesses.keys().map(|k| k.as_str()).collect();
        assert_eq!(
            declared, shipped,
            "{event}: lifecycle_events harness set disagrees with adapters::targets()",
        );

        for (harness, cell) in harnesses {
            let supported = cell["supported"]
                .as_bool()
                .unwrap_or_else(|| panic!("{event}/{harness}: no `supported` boolean"));

            // Every cell records whether it is first-party verified. A positive
            // capability claim must carry a date; literal false is reserved for
            // unsupported/unresearched cells so consumers cannot emit guesses.
            let verified = &cell["verified"];

            let notes_ok = cell["notes"].as_str().is_some_and(|s| !s.trim().is_empty());

            if supported {
                assert!(
                    is_verified_date(verified),
                    "{event}/{harness}: supported capability must have verified YYYY-MM-DD date, got {verified}",
                );
                assert!(cell["policy"].is_null(), "{event}/{harness}: supported cell must not carry a gap policy");
                assert!(
                    cell["channel"].as_str().is_some_and(|s| !s.trim().is_empty()),
                    "{event}/{harness}: a supported cell needs a non-empty `channel`",
                );
                assert!(
                    cell["blocking"].is_boolean(),
                    "{event}/{harness}: a supported cell needs a boolean `blocking`",
                );
                assert!(notes_ok, "{event}/{harness}: a supported cell needs non-empty `notes`");
            } else {
                assert!(
                    is_verified_date(verified) || verified.as_bool() == Some(false),
                    "{event}/{harness}: unsupported cell needs verified YYYY-MM-DD date or literal false, got {verified}",
                );
                assert!(
                    cell["policy"].as_str().is_some_and(|s| !s.trim().is_empty()),
                    "{event}/{harness}: an unsupported cell needs a `policy` (the `features` convention)",
                );
                assert!(cell["channel"].is_null(), "{event}/{harness}: unsupported cell must have null `channel`");
                assert!(cell["blocking"].is_null(), "{event}/{harness}: unsupported cell must have null `blocking`");
                assert!(notes_ok, "{event}/{harness}: an unsupported cell needs non-empty `notes`");
            }

            if let Some(channels) = cell.get("channels") {
                let values = channels
                    .as_array()
                    .unwrap_or_else(|| panic!("{event}/{harness}: `channels` must be an array"));
                assert!(!values.is_empty(), "{event}/{harness}: `channels` must not be empty");
                assert!(
                    values.iter().all(|v| v.as_str().is_some_and(|s| !s.trim().is_empty())),
                    "{event}/{harness}: `channels` entries must be non-empty strings",
                );
                if supported {
                    assert!(
                        values.iter().any(|v| v.as_str() == cell["channel"].as_str()),
                        "{event}/{harness}: anchor `channel` must be included in `channels`",
                    );
                }
            }
        }
    }

    // PreToolUse anchor cross-check: matrix channel must agree with the shared
    // adapter table that describes each emitted shim's native hook channel.
    let pre = events["PreToolUse"]["harnesses"].as_object().unwrap();
    for harness in shipmates::adapters::targets() {
        let channel = shipmates::adapters::render::hook_channel(harness)
            .unwrap_or_else(|| panic!("no hook channel for target {harness}"));
        assert_eq!(
            pre[harness]["channel"].as_str().unwrap(),
            channel,
            "PreToolUse/{harness}: matrix channel must match adapter hook channel",
        );
        assert_eq!(
            pre[harness]["blocking"].as_bool(),
            Some(true),
            "PreToolUse/{harness}: the gate channel must be blocking",
        );
    }
}

/// Drive the REAL `commands/ship-issue.md` stages through `shipmates state` via
/// the compiled binary, proving the 0/1/2 exit-code ABI end to end. This is the
/// contract the (planned) enforcement hook depends on; the FSM comes from the
/// embedded catalog, so this also proves `stages:` parses at runtime.
#[test]
fn test_state_cli_drives_real_ship_issue_fsm_and_exit_abi() {
    let bin = env!("CARGO_BIN_EXE_shipmates");
    let temp_dir = tempfile::tempdir().unwrap();
    let base = temp_dir.path();

    let run = |args: &[&str]| {
        std::process::Command::new(bin)
            .current_dir(base)
            .args(args)
            .output()
            .expect("failed to execute shipmates state")
    };
    let code = |out: &std::process::Output| out.status.code().unwrap();

    // init at the first stage (`plan`) succeeds — exit 0.
    let out = run(&["state", "init", "--run", "212", "--command", "ship-issue"]);
    assert_eq!(code(&out), 0, "init: {}", String::from_utf8_lossy(&out.stderr));
    assert!(base.join(".shipmates/run-212.json").is_file());
    assert!(String::from_utf8_lossy(&out.stdout).contains("\"phase\": \"plan\""));

    // init again refuses to overwrite an existing run file — error, exit 2.
    let out = run(&["state", "init", "--run", "212", "--command", "ship-issue"]);
    assert_eq!(code(&out), 2, "init overwrite must fail-closed");
    assert!(String::from_utf8_lossy(&out.stderr).contains("refusing to overwrite"));

    // assert plan -> isolate is legal — exit 0.
    let out = run(&["state", "assert", "--run", "212", "--to", "isolate"]);
    assert_eq!(code(&out), 0);
    assert!(String::from_utf8_lossy(&out.stdout).contains("\"legal\":true"));

    // assert plan -> build skips isolate — build is later than plan, so it is a
    // forward jump, not a loopback: illegal, exit 1, greppable reason.
    let out = run(&["state", "assert", "--run", "212", "--to", "build"]);
    assert_eq!(code(&out), 1);
    assert!(String::from_utf8_lossy(&out.stdout).contains("\"legal\":false"));
    assert!(String::from_utf8_lossy(&out.stderr).contains("illegal transition"));

    // assert against a missing run file — error, exit 2.
    let out = run(&["state", "assert", "--run", "999", "--to", "isolate"]);
    assert_eq!(code(&out), 2);
    assert!(String::from_utf8_lossy(&out.stderr).contains("error:"));

    // a non-numeric --run is rejected by argument parsing — exit 2 (clap usage).
    let out = run(&["state", "status", "--run", "../etc/passwd"]);
    assert_eq!(code(&out), 2, "non-numeric run id must not parse");

    // advance plan -> isolate commits the phase — exit 0, file updated.
    let out = run(&["state", "advance", "--run", "212", "--to", "isolate"]);
    assert_eq!(code(&out), 0);
    let status = run(&["state", "status", "--run", "212"]);
    assert!(String::from_utf8_lossy(&status.stdout).contains("\"phase\": \"isolate\""));

    // advance forward through build and verify, then a loopback verify -> build
    // (verify declares on_fail: build) charges verify's own per-stage counter.
    assert_eq!(code(&run(&["state", "advance", "--run", "212", "--to", "build"])), 0);
    assert_eq!(code(&run(&["state", "advance", "--run", "212", "--to", "verify"])), 0);
    assert_eq!(code(&run(&["state", "advance", "--run", "212", "--to", "build"])), 0);
    let status = run(&["state", "status", "--run", "212"]);
    let body = String::from_utf8_lossy(&status.stdout);
    assert!(body.contains("\"phase\": \"build\""));
    // fix_rounds is now a per-stage map; the loopback charged `verify`, not build.
    assert!(
        body.contains("\"verify\": 1"),
        "loopback must charge the departing stage's own fix round: {body}"
    );
}

/// `shipmates state --dir <path>` resolves `.shipmates/run-<N>.json` under the
/// given path, not the process cwd — the plumbing a hook shim uses to gate a run
/// in another worktree without a `cd`. Also proves the gate deny reason on stderr
/// is the bare `gate: …` line (no "illegal transition:" prefix).
#[test]
fn test_state_dir_resolves_run_file_at_given_path() {
    let bin = env!("CARGO_BIN_EXE_shipmates");
    let base = tempfile::tempdir().unwrap(); // where the run file must land
    let cwd = tempfile::tempdir().unwrap(); // an UNRELATED working directory

    let run = |args: &[&str]| {
        std::process::Command::new(bin)
            .current_dir(cwd.path()) // never the base dir
            .args(args)
            .output()
            .expect("failed to execute shipmates state")
    };
    let code = |out: &std::process::Output| out.status.code().unwrap();
    let dir = base.path().to_str().unwrap();

    // init --dir writes under `base`, not the cwd.
    let out = run(&["state", "init", "--dir", dir, "--run", "300", "--command", "ship-issue"]);
    assert_eq!(code(&out), 0, "init: {}", String::from_utf8_lossy(&out.stderr));
    assert!(base.path().join(".shipmates/run-300.json").is_file(), "run file must land under --dir");
    assert!(!cwd.path().join(".shipmates/run-300.json").exists(), "run file must NOT land in the cwd");

    // gate --dir reads that same file: `git push` at `plan` is denied (exit 1),
    // and the stderr reason is the bare `gate: …` line with no label prefix.
    let out = run(&["state", "gate", "--dir", dir, "--run", "300", "--tool", "git push -u origin HEAD"]);
    assert_eq!(code(&out), 1);
    let stderr = String::from_utf8_lossy(&out.stderr);
    let stderr = stderr.trim();
    assert!(stderr.starts_with("gate:"), "deny reason must start with `gate:`, got: {stderr:?}");
    assert!(!stderr.contains("illegal transition"), "deny reason must not be labelled a transition: {stderr:?}");
    assert!(stderr.contains("requires phase>=build, run is at plan"), "{stderr:?}");

    // ...and once advanced to `build`, the same gate allows (exit 0).
    assert_eq!(code(&run(&["state", "advance", "--dir", dir, "--run", "300", "--to", "isolate"])), 0);
    assert_eq!(code(&run(&["state", "advance", "--dir", dir, "--run", "300", "--to", "build"])), 0);
    assert_eq!(code(&run(&["state", "gate", "--dir", dir, "--run", "300", "--tool", "git push -u origin HEAD"])), 0);
}

fn walk(dir: &std::path::Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                out.extend(walk(&path));
            } else {
                out.push(path);
            }
        }
    }
    out
}
