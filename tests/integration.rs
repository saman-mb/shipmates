use shipmates::adapters::Adapter;
use shipmates::adapters::antigravity::AntigravityAdapter;
use shipmates::adapters::claude_code::ClaudeCodeAdapter;
use shipmates::adapters::codex::CodexAdapter;
use shipmates::adapters::opencode::OpencodeAdapter;
use shipmates::catalog::{load_commands, load_roles, reject_positional, CanonicalCommand, CanonicalRole};
use shipmates::digest;
use std::collections::BTreeMap;
use std::path::PathBuf;
use regex::Regex;

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
    let content = files
        .get("harnesses/claude-code/.claude/agents/test-role.md")
        .unwrap();
    let hashed = digest::hash(content);
    assert_eq!(
        hashed,
        "491b209dc45c12fd8b89e113ba775ca5c6c03b0b977c868427cdbf22e0705209"
    );
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
        narrative: "narrative".into(),
        invocation: "invoke".into(),
        board: "board".into(),
        source: PathBuf::from("cmd.md"),
    };
    let files = OpencodeAdapter.build(&[], &[command]).unwrap();
    let content = files
        .get("harnesses/opencode/.opencode/commands/test-cmd.md")
        .unwrap();
    let hashed = digest::hash(content);
    assert_eq!(
        hashed,
        "d7f5ef7b388b4472f7005bd7788b93ff8a637cc2e889528f8a505af60d3fbe5f"
    );
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
    let content = files
        .get("harnesses/opencode/.opencode/agents/test-role.md")
        .unwrap();
    assert!(content.starts_with("---\n"));
    assert!(content.contains("  \"*\": deny\n"));
    assert!(content.contains("  read: allow\n"));
}

#[test]
fn test_opencode_cli_build_matches_golden_payload() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let out = tempfile::tempdir().unwrap();
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_shipmates"))
        .current_dir(&root)
        .args(["build", "--target", "opencode", "--out", out.path().to_str().unwrap()])
        .output()
        .expect("failed to execute opencode build");
    assert!(
        output.status.success(),
        "opencode build failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let expected = read_payload_digest(&root.join("tests/payload-digests/opencode.sha256"));
    let payload = out.path().join("harnesses/opencode/.opencode");
    let mut actual = BTreeMap::new();
    for path in walk(&payload) {
        let relative = path
            .strip_prefix(&payload)
            .unwrap()
            .to_string_lossy()
            .replace('\\', "/");
        actual.insert(relative, digest::compute_sha256(&path).unwrap());
    }

    assert_eq!(actual, expected, "opencode build drifted from golden payload");
}

#[test]
fn test_opencode_embedded_install_fidelity() {
    let empty_cwd = tempfile::tempdir().unwrap();
    let sandbox = tempfile::tempdir().unwrap();
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_shipmates"))
        // No checkout source is visible here. `install` must use the payload
        // embedded in the test binary, as a packaged CLI does.
        .current_dir(empty_cwd.path())
        .args([
            "install",
            "--harness",
            "opencode",
            "--dir",
            sandbox.path().to_str().unwrap(),
            "--with-tools",
            "none",
        ])
        .output()
        .expect("failed to execute opencode install");
    assert!(
        output.status.success(),
        "opencode install failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let agents = sandbox.path().join(".opencode/agents");
    let commands = sandbox.path().join(".opencode/commands");
    let expected_roles = [
        "architect",
        "art-director",
        "data-scientist",
        "devops-engineer",
        "performance-engineer",
        "product-manager",
        "sdet",
        "security-engineer",
        "senior-engineer",
        "site-reliability-engineer",
        "technical-writer",
        "ux-ui-designer",
    ];

    for role in expected_roles {
        let path = agents.join(format!("{role}.md"));
        let content = std::fs::read_to_string(&path).unwrap_or_else(|_| panic!("missing {path:?}"));
        assert!(content.contains("mode: subagent\n"), "{path:?} is not a subagent");
        assert!(content.contains("permission:\n"), "{path:?} has no permission map");
    }
    assert_eq!(file_count(&agents), expected_roles.len());
    assert_eq!(file_count(&commands), 14);

    let report_order = std::fs::read_to_string(commands.join("harden.md")).unwrap();
    assert!(report_order.contains("report"), "harden order lost report-only mode");
    assert!(report_order.contains("$ARGUMENTS"), "harden order lost argument passing");
    assert!(!report_order.contains("{{"), "neutral argument placeholder leaked");
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
fn test_prompt_cost_layout_is_shared_and_cache_friendly() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let commands = load_commands(&root.join("commands")).unwrap();
    let roles = load_roles(&root.join("crew")).unwrap();

    assert_eq!(commands.len(), 14, "cost preamble must cover every command");
    let re_tokens = regex::Regex::new(r"\{\{[a-zA-Z:-]+\}\}").unwrap();
    for command in &commands {
        assert_eq!(
            command.narrative.matches("<!-- shipmates:command-preamble -->").count(),
            1,
            "{} must reference shared command preamble once",
            command.name
        );
        assert_eq!(
            command.narrative.matches("## Runtime input").count(),
            1,
            "{} must have one runtime-input section",
            command.name
        );
        assert_eq!(
            command.narrative.matches("$ARGUMENTS").count(),
            1,
            "{} must keep its only argument token in runtime input",
            command.name
        );
        assert!(
            command.narrative.find("## Runtime input").unwrap()
                > command.narrative.find("<!-- shipmates:command-preamble -->").unwrap(),
            "{} places volatile input below stable workflow",
            command.name
        );
        let tokens: Vec<&str> = re_tokens
            .find_iter(&command.narrative)
            .map(|m| m.as_str())
            .collect();
        let allowed = [
            "{{project-instructions}}",
            "{{project-instructions-fallback}}",
            "{{agents-glob}}",
            "{{session-key}}",
            "{{general-purpose}}",
            "{{role:planner}}",
            "{{planner-agent}}",
            "{{role:senior-engineer}}",
            "{{role:sdet}}",
            "{{role-reference}}",
        ];
        for token in &tokens {
            let is_argument = token.starts_with("{{") && !token.contains(':');
            assert!(
                allowed.contains(token) || is_argument,
                "{} has unknown exporter token {token}",
                command.name
            );
        }

        let source = std::fs::read_to_string(root.join("commands").join(format!("{}.md", command.name)))
            .unwrap();
        for key in ["arguments:", "invocation:", "board:"] {
            assert!(!source.lines().any(|line| line.starts_with(key)), "{} has {key}", command.name);
        }
    }

    assert_eq!(roles.len(), 12);
    for role in &roles {
        assert_eq!(
            role.body.matches("<!-- shipmates:subagent-preamble -->").count(),
            1,
            "{} must reference shared subagent preamble once",
            role.name
        );
    }

    for target in shipmates::adapters::targets() {
        let files = shipmates::adapters::select(target).unwrap().build(&roles, &commands).unwrap();

        for command in &commands {
            let suffixes = [
                format!("/{}/SKILL.md", command.name),
                format!("/commands/{}.md", command.name),
            ];
            let matches: Vec<_> = files
                .iter()
                .filter(|(path, _)| suffixes.iter().any(|suffix| path.ends_with(suffix)))
                .collect();
            assert_eq!(matches.len(), 1, "{target} must emit one {} command", command.name);
            let (path, content) = matches[0];
            assert!(content.contains("## Cost discipline"), "{target} {path} missed command preamble");
            assert!(!content.contains("shipmates:command-preamble"), "{target} {path} leaked command marker");
        }

        let role_outputs: Vec<_> = files
            .iter()
            .filter(|(path, _)| path.contains("/agents/") && !path.ends_with("AGENTS.md"))
            .collect();
        for role in &roles {
            let matches: Vec<_> = role_outputs
                .iter()
                .filter(|(path, _)| path.contains(&format!("/agents/{}.", role.name)))
                .collect();
            if matches.is_empty() {
                continue;
            }
            assert_eq!(matches.len(), 1, "{target} must emit one {} role", role.name);
            let (path, content) = matches[0];
            assert!(content.contains("## Return discipline"), "{target} {path} missed role preamble");
            assert!(!content.contains("shipmates:subagent-preamble"), "{target} {path} leaked role marker");
        }
    }
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
            .args([
                "build",
                "--target",
                target,
                "--out",
                temp_dir.path().to_str().unwrap(),
            ])
            .output()
            .expect("failed to execute shipmates build");
        assert!(
            output.status.success(),
            "{target} build failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    // Every harness that reads the open Agent Skills tree ships its skills to the
    // shared `.agents/skills/` location, not a harness-private one.
    let codex_skill = temp_dir
        .path()
        .join("harnesses/codex/.agents/skills/ship-issue/SKILL.md");
    assert!(codex_skill.is_file(), "codex ship-issue skill not emitted");
    let copilot_skill = temp_dir
        .path()
        .join("harnesses/github-copilot/.agents/skills/ship-issue/SKILL.md");
    assert!(
        copilot_skill.is_file(),
        "copilot ship-issue skill not emitted"
    );
    // ...and the shared rendering is byte-identical across those harnesses.
    let codex_bytes = std::fs::read(&codex_skill).unwrap();
    let copilot_bytes = std::fs::read(&copilot_skill).unwrap();
    assert_eq!(
        codex_bytes, copilot_bytes,
        "shared skill must be identical across harnesses"
    );
}

/// The Copilot payload digest is a checked-in golden file for the complete
/// `build --target github-copilot` output.  Check both missing and unexpected
/// files so a newly emitted file cannot bypass the fixture.
#[test]
fn test_github_copilot_build_matches_golden_payload() {
    let temp_dir = tempfile::tempdir().unwrap();
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_shipmates"))
        .args([
            "build",
            "--target",
            "github-copilot",
            "--out",
            temp_dir.path().to_str().unwrap(),
        ])
        .output()
        .expect("failed to execute github-copilot build");
    assert!(
        output.status.success(),
        "github-copilot build failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let digest_path = root.join("tests/payload-digests/github-copilot.sha256");
    let payload_root = temp_dir.path().join("harnesses/github-copilot");
    let mut expected = std::collections::BTreeMap::new();
    for line in std::fs::read_to_string(digest_path).unwrap().lines().skip(2) {
        let (path, hash) = line.split_once(' ').expect("malformed Copilot golden entry");
        expected.insert(path.to_string(), hash.to_string());
    }

    for (path, expected_hash) in &expected {
        let file = payload_root.join(path);
        assert!(file.is_file(), "golden payload file missing: {path}");
        let content = std::fs::read_to_string(file).unwrap();
        assert_eq!(digest::hash(&content), *expected_hash, "golden mismatch: {path}");
    }

    let actual: std::collections::BTreeSet<String> = walk(&payload_root)
        .into_iter()
        .map(|path| normalized_relative_path(&path, &payload_root))
        .collect();
    let expected_paths: std::collections::BTreeSet<String> = expected.keys().cloned().collect();
    assert_eq!(actual, expected_paths, "Copilot payload file set drifted from golden");
}

fn normalized_relative_path(path: &std::path::Path, root: &std::path::Path) -> String {
    path.strip_prefix(root)
        .unwrap()
        .components()
        .map(|component| component.as_os_str().to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join("/")
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
        narrative: "Write `{{project-instructions}}` if one exists, else `{{project-instructions-fallback}}`; resolve via {{agents-glob}}; use {{repo}}."
            .into(),
        invocation: "invoke".into(),
        board: "board".into(),
        source: PathBuf::from("cmd.md"),
    };
    let files = CodexAdapter.build(&[], &[command]).unwrap();
    let content = files
        .get("harnesses/codex/.agents/skills/onboard/SKILL.md")
        .unwrap();
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
        .args([
            "install",
            "--harness",
            "claude-code",
            "--dir",
            temp_dir.path().to_str().unwrap(),
        ])
        .output()
        .expect("failed to execute shipmates install");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Installed harness: claude-code"));
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
        let files = shipmates::adapters::select(target)
            .unwrap()
            .build(&[role()], &[])
            .unwrap();
        for (path, content) in &files {
            if !path.contains("/agents/") {
                continue;
            }
            assert!(
                !content
                    .lines()
                    .any(|l| l.trim_start().starts_with("model:")),
                "{target} agent file {path} emitted a model line:\n{content}"
            );
            assert!(
                !content
                    .lines()
                    .any(|l| l.trim_start().starts_with("model = ")),
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
    let content = files
        .get("harnesses/antigravity/.agents/agents/architect.md")
        .unwrap();
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
    let matrix: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(root.join("tools/harness_matrix.json")).unwrap(),
    )
    .unwrap();
    let harnesses = matrix["harnesses"]
        .as_object()
        .expect("harness_matrix.json has no harnesses map");

    let declared: std::collections::BTreeSet<&str> = harnesses.keys().map(|k| k.as_str()).collect();
    let shipped: std::collections::BTreeSet<&str> =
        shipmates::adapters::targets().into_iter().collect();
    assert_eq!(
        declared, shipped,
        "harness_matrix.json and adapters::targets() disagree"
    );

    let temp_dir = tempfile::tempdir().unwrap();
    for (name, entry) in harnesses {
        let claims_agents = entry["agents"]
            .as_bool()
            .unwrap_or_else(|| panic!("{name}: no `agents` boolean"));
        assert!(
            entry["agents_notes"]
                .as_str()
                .is_some_and(|s| !s.trim().is_empty()),
            "{name}: `agents` must carry `agents_notes` recording the evidence — a bare flag is how              three harnesses stayed wrong",
        );

        let out = temp_dir.path().join(name);
        let status = std::process::Command::new(env!("CARGO_BIN_EXE_shipmates"))
            .args(["build", "--target", name, "--out", out.to_str().unwrap()])
            .status()
            .expect("failed to execute shipmates build");
        assert!(status.success(), "{name}: build failed");

        let emits_agents = walk(&out).iter().any(|p| {
            p.components().any(|c| c.as_os_str() == "agents")
                && p.file_name().is_some_and(|f| f != "AGENTS.md")
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
    let matrix: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(root.join("tools/harness_matrix.json")).unwrap(),
    )
    .unwrap();
    let harnesses = matrix["harnesses"]
        .as_object()
        .expect("harness_matrix.json has no harnesses map");

    // Detect a reasoning-effort key across every dialect: claude-code's
    // `effort:` line, codex's `model_reasoning_effort` TOML key, opencode's
    // top-level `reasoningEffort`.
    fn carries_effort(content: &str) -> bool {
        content
            .lines()
            .any(|l| l.trim_start().starts_with("effort:"))
            || content.contains("model_reasoning_effort")
            || content.contains("reasoningEffort")
    }

    let temp_dir = tempfile::tempdir().unwrap();
    for name in shipmates::adapters::targets() {
        let entry = &harnesses[name];
        let claims_effort = entry["effort"]
            .as_bool()
            .unwrap_or_else(|| panic!("{name}: no `effort` boolean"));
        assert!(
            entry["effort_notes"]
                .as_str()
                .is_some_and(|s| !s.trim().is_empty()),
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

fn file_count(dir: &std::path::Path) -> usize {
    std::fs::read_dir(dir)
        .unwrap()
        .filter_map(Result::ok)
        .filter(|entry| entry.path().is_file())
        .count()
}

fn read_payload_digest(path: &std::path::Path) -> BTreeMap<String, String> {
    std::fs::read_to_string(path)
        .unwrap()
        .lines()
        .skip(2)
        .map(|line| {
            let mut fields = line.split_whitespace();
            let relative = fields.next().expect("digest entry has no path");
            let hash = fields.next().expect("digest entry has no hash");
            assert!(fields.next().is_none(), "digest entry has extra fields: {line}");
            (relative.to_string(), hash.to_string())
        })
        .collect()
}
