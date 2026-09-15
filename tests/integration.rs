use shipmates::adapters::Adapter;
use shipmates::adapters::antigravity::AntigravityAdapter;
use shipmates::adapters::claude_code::ClaudeCodeAdapter;
use shipmates::adapters::codex::CodexAdapter;
use shipmates::adapters::opencode::OpencodeAdapter;
use shipmates::catalog::{
    load_commands, load_roles, load_tools, reject_positional, CanonicalCommand, CanonicalRole,
};
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
        "3b29fbd767a48839b5e6c0ef9778960372a6af89fc1d6b4f2daa929fc2846b3a"
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
        "2987e52d7d1aa16f2496230b9584f2035baecab47dc7a0bc63d4958bce4d83ec"
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
    let payload_root = out.path().join("harnesses/opencode");
    let mut actual = BTreeMap::new();
    for path in walk(&payload_root) {
        let relative = normalized_relative_path(&path, &payload_root);
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
        "principal-engineer",
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
    assert_eq!(file_count(&commands), 16);

    let report_order = std::fs::read_to_string(commands.join("ship-harden.md")).unwrap();
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

    assert_eq!(commands.len(), 16, "cost preamble must cover every command");
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

    assert_eq!(roles.len(), 13);
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
            // The ruleset is global: the shared preamble carries it, so every
            // target's every command must render it — not just the ones this repo
            // happened to wire a marker into.
            assert_eq!(
                content.matches("## Model routing").count(),
                1,
                "{target} {path} must carry the model-routing ruleset exactly once"
            );
            assert!(!content.contains("shipmates:command-preamble"), "{target} {path} leaked command marker");
            assert!(!content.contains("shipmates:acceptance-board"), "{target} {path} leaked board marker");
            assert!(
                !content.contains("shipmates:epic-integration-board"),
                "{target} {path} leaked epic integration board marker"
            );
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

/// The #407 defect was frontmatter that installed cleanly and was never
/// discovered because a strict loader rejected it. So: strict-parse every
/// emitted frontmatter block, keep every artifact's `name:` bare and equal to
/// the identity its install path implies (what `adopt::frontmatter_name_matches`
/// reads), and pin the failure mode with a negative control.
///
/// Codex is the one target whose crew is TOML rather than Markdown — those bytes
/// are digest-gated and exercised by `tests/test_codex_smoke.sh`, so they are
/// skipped here.
#[test]
fn test_emitted_frontmatter_strict_parses_and_names_stay_bare() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let roles = load_roles(&root.join("crew")).unwrap();
    let commands = load_commands(&root.join("commands")).unwrap();
    let tools = load_tools(&root.join("toolbox")).unwrap();
    assert_eq!(commands.len(), 16);
    assert!(!tools.is_empty(), "toolbox/ must hold tools for build_tools");

    // Negative control: `description: Shipmates: take an issue` is a nested
    // mapping to YAML and fails a strict parse; the double-quoted style is what
    // this change exists to ship.
    assert!(
        serde_yaml::from_str::<serde_yaml::Value>(
            "name: ship-epic\ndescription: Shipmates: take an issue\n"
        )
        .is_err(),
        "an unquoted `: ` description must fail a strict YAML parse"
    );
    assert!(
        serde_yaml::from_str::<serde_yaml::Value>(
            "name: ship-epic\ndescription: \"Shipmates: take an issue\"\n"
        )
        .is_ok(),
        "the double-quoted equivalent must parse"
    );

    let mut saw_shipmates_description = false;
    for target in shipmates::adapters::targets() {
        let adapter = shipmates::adapters::select(target).unwrap();
        let mut files = adapter.build(&roles, &commands).unwrap();
        files.extend(adapter.build_tools(&tools));

        // All sixteen commands must arrive exactly once, as a skill or as
        // opencode's command file — the path shape every harness resolves.
        for command in &commands {
            let emitted = files
                .keys()
                .filter(|path| {
                    path.ends_with(&format!("/skills/{}/SKILL.md", command.name))
                        || path.ends_with(&format!("/commands/{}.md", command.name))
                })
                .count();
            assert_eq!(emitted, 1, "{target} must emit command {}", command.name);
        }

        let mut parsed_blocks = 0;
        for (path, content) in &files {
            // Codex crew are standalone TOML (`name = "architect"`), not YAML
            // frontmatter: their bytes are digest-gated in
            // tests/payload-digests/codex.sha256 and run through
            // tests/test_codex_smoke.sh, so strict-YAML parsing does not apply.
            if path.ends_with(".toml") {
                continue;
            }
            if !(path.ends_with(".md") || path.ends_with(".mdc")) {
                continue;
            }
            let Some(frontmatter) = frontmatter_block(content) else {
                continue;
            };
            let parsed: serde_yaml::Value =
                serde_yaml::from_str(frontmatter).unwrap_or_else(|error| {
                    panic!("{target} {path}: frontmatter is not strict YAML: {error}\n{frontmatter}")
                });
            parsed_blocks += 1;
            if parsed
                .get("description")
                .and_then(|description| description.as_str())
                .is_some_and(|description| description.starts_with("Shipmates"))
            {
                saw_shipmates_description = true;
            }

            // Identity contract: a skill/command/agent `name:` is bare and
            // equal to the path's artifact name — the shape adopt reads.
            // Skills always declare it; opencode's agents and commands are
            // named by their filename, so absence is tolerated there.
            let Some(identity) =
                shipmates::installer::adopt::artifact_name(std::path::Path::new(path))
            else {
                continue;
            };
            match frontmatter.lines().find(|line| line.starts_with("name:")) {
                Some(line) => assert_eq!(
                    line,
                    format!("name: {identity}"),
                    "{target} {path}: `name:` must be bare and equal the path identity"
                ),
                None => assert!(
                    !path.ends_with("/SKILL.md"),
                    "{target} {path}: an emitted skill must declare `name: {identity}`"
                ),
            }
        }
        assert!(
            parsed_blocks > 0,
            "{target} emitted no parseable YAML frontmatter block"
        );
    }
    assert!(
        saw_shipmates_description,
        "no emitted description starts with `Shipmates`"
    );
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
        "pi",
        "windsurf",
    ] {
        assert!(stdout.contains(target), "targets output missing {target}");
    }
}

#[test]
fn test_non_claude_targets_build_via_cli() {
    let temp_dir = tempfile::tempdir().unwrap();
    for target in ["codex", "cursor", "github-copilot", "pi", "windsurf"] {
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
    let pi_skill = temp_dir
        .path()
        .join("harnesses/pi/.agents/skills/ship-issue/SKILL.md");
    assert!(pi_skill.is_file(), "pi ship-issue skill not emitted");
    // ...and the shared rendering is byte-identical across those harnesses.
    let codex_bytes = std::fs::read(&codex_skill).unwrap();
    let copilot_bytes = std::fs::read(&copilot_skill).unwrap();
    let pi_bytes = std::fs::read(&pi_skill).unwrap();
    assert_eq!(
        codex_bytes, copilot_bytes,
        "shared skill must be identical across harnesses"
    );
    assert_eq!(
        codex_bytes, pi_bytes,
        "pi shared skill must be identical across harnesses"
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

/// Canonical content is rendered **once** and shared by four harnesses whose
/// crews live in four different trees, so a harness-specific path in it is wrong
/// for three of the four.
///
/// This was live for a long time. Every shared-tree command told the model to
/// resolve a role from `.agents/agents/*.md` — correct only for Antigravity.
/// pi reads `.pi/agents/`, Codex `.codex/agents/`, Copilot `.github/agents/`. An
/// agent that believed the instruction would conclude the role had not resolved
/// and fall back to a general-purpose agent, quietly downgrading a specialist
/// seat. The token cannot be fixed by changing its value: the same bytes are
/// rendered for all four.
///
/// The dialect still supports both tokens — a harness rendering through its own
/// dialect may use them correctly — so this guard is about canonical *content*,
/// not about the render layer's vocabulary.
#[test]
fn canonical_content_names_no_harness_specific_crew_path() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let mut sources: Vec<PathBuf> = std::fs::read_dir(root.join("commands"))
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "md"))
        .collect();
    sources.extend(
        std::fs::read_dir(root.join("steering"))
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .filter(|path| path.extension().is_some_and(|ext| ext == "md")),
    );
    sources.push(root.join("docs/COST.md"));

    let mut offenders = Vec::new();
    for path in sources {
        let text = std::fs::read_to_string(&path).unwrap();
        for token in ["{{agents-glob}}", "{{general-purpose}}"] {
            if text.contains(token) {
                offenders.push(format!(
                    "{}: {token}",
                    path.strip_prefix(&root).unwrap_or(&path).display()
                ));
            }
        }
    }
    assert!(
        offenders.is_empty(),
        "canonical content is shared by four harnesses whose crews live in four \
         different trees, so it must not name one of them: {offenders:?}"
    );
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

/// #454: a home/global pi install prints project-local guidance; a `--dir`
/// project install must not.
#[test]
fn test_pi_global_install_prints_project_local_guidance() {
    let home = tempfile::tempdir().unwrap();
    let project = tempfile::tempdir().unwrap();

    let global = std::process::Command::new(env!("CARGO_BIN_EXE_shipmates"))
        .env("HOME", home.path())
        .args([
            "install",
            "--harness",
            "pi",
            "--global",
            "--with-tools",
            "none",
        ])
        .output()
        .expect("failed to execute shipmates install --global pi");
    assert!(
        global.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&global.stdout),
        String::from_utf8_lossy(&global.stderr)
    );
    let global_out = String::from_utf8_lossy(&global.stdout);
    assert!(
        global_out.contains("Installed harness: pi"),
        "missing install line: {global_out}"
    );
    assert!(
        global_out.contains("nearest ancestor")
            && global_out.contains("~/.pi/agent/")
            && global_out.contains("--local"),
        "missing #454 home-install guidance: {global_out}"
    );

    let local = std::process::Command::new(env!("CARGO_BIN_EXE_shipmates"))
        .env("HOME", home.path())
        .args([
            "install",
            "--harness",
            "pi",
            "--dir",
            project.path().to_str().unwrap(),
            "--with-tools",
            "none",
        ])
        .output()
        .expect("failed to execute shipmates install --dir pi");
    assert!(
        local.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&local.stdout),
        String::from_utf8_lossy(&local.stderr)
    );
    let local_out = String::from_utf8_lossy(&local.stdout);
    assert!(
        local_out.contains("Installed harness: pi"),
        "missing install line: {local_out}"
    );
    assert!(
        !local_out.contains("nearest ancestor") && !local_out.contains("shadowed"),
        "project-local pi install must not print the home-shadow hint: {local_out}"
    );
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
    // Antigravity discovers `{workspace}/.agents/agents/{agent_name}/` and
    // reads the `agent.md` inside it. A flat `<name>.md` installs cleanly and
    // is never read, which is why the crew silently never loaded before this
    // shape was corrected.
    let content = files
        .get("harnesses/antigravity/.agents/agents/architect/agent.md")
        .expect("antigravity must emit a directory per agent holding agent.md");
    assert!(content.contains("name: architect"));
    assert!(content.contains("subagent: true"));
    assert!(content.contains("system prompt body"));
    assert!(
        !files.contains_key("harnesses/antigravity/.agents/agents/architect.md"),
        "the flat shape Antigravity never reads must not be emitted"
    );
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
    // top-level `reasoningEffort`, and pi's `thinking:` line.
    fn carries_effort(content: &str) -> bool {
        content
            .lines()
            .any(|l| {
                let l = l.trim_start();
                l.starts_with("effort:") || l.starts_with("thinking:")
            })
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

/// Every harness's `model_surface` record must be complete, closed-enum and
/// internally consistent. The issue's "no unstated cells" requirement is a
/// mechanical invariant, not a prose one: prose in the ADR cannot stop a cell
/// from going blank, and only a check can — the same lesson the `agents` and
/// `effort` guards above encode. `enumeration.command` being non-empty exactly
/// when `available` is true is what stops a blank command from being read
/// downstream as a usable pool.
#[test]
fn test_matrix_model_surface_is_complete() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let matrix: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(root.join("tools/harness_matrix.json")).unwrap(),
    )
    .unwrap();
    let schema = matrix["model_surface_schema"]
        .as_object()
        .expect("harness_matrix.json has no model_surface_schema block");
    assert_eq!(
        schema["empty_pool_fallback"].as_str(),
        Some("inherit"),
        "the schema must record `inherit` as the empty-pool fallback"
    );

    let harnesses = matrix["harnesses"]
        .as_object()
        .expect("harness_matrix.json has no harnesses map");

    const DISCOVERY_TIERS: [&str; 3] = ["query", "declared", "inherit"];
    const OVERRIDE_KINDS: [&str; 4] = ["per-spawn", "static-agent-file", "session-level", "none"];
    const EFFORT_KINDS: [&str; 4] = [
        "separate-key",
        "folded-into-model-string",
        "run-level",
        "none",
    ];
    const ENFORCEMENTS: [&str; 4] = ["abort", "warn", "fallback", "none"];
    const REQUIRED_KEYS: [&str; 9] = [
        "discovery_tier",
        "enumeration",
        "identity",
        "runtime_model_override",
        "effort",
        "declared_pool",
        "reported_gaps",
        "verified_on",
        "notes",
    ];
    let date = Regex::new(r"^\d{4}-\d{2}-\d{2}$").unwrap();

    // A named accessor, so a missing or wrong-typed nested cell names the harness
    // and the path instead of panicking on an opaque `unwrap()`.
    fn cell<'a>(name: &str, path: &str, value: &'a serde_json::Value) -> &'a str {
        value.as_str().unwrap_or_else(|| {
            panic!(
                "{name}: model_surface.{path} is missing or not a string — \
                 every cell is stated, never left blank"
            )
        })
    }
    fn non_empty(name: &str, path: &str, value: &serde_json::Value) {
        assert!(
            !cell(name, path, value).trim().is_empty(),
            "{name}: model_surface.{path} is blank — a missing feature is a stated finding, \
             never an empty cell"
        );
    }

    // The schema block is the human-readable copy of these enums. If the two ever
    // disagree, the closed enum is being enforced against a stale list.
    for (path, expected) in [
        ("discovery_tier", &DISCOVERY_TIERS[..]),
        ("runtime_model_override.kind", &OVERRIDE_KINDS[..]),
        ("effort.kind", &EFFORT_KINDS[..]),
        ("declared_pool.enforcement", &ENFORCEMENTS[..]),
    ] {
        let declared: Vec<&str> = schema["enums"][path]
            .as_array()
            .unwrap_or_else(|| panic!("model_surface_schema.enums has no `{path}`"))
            .iter()
            .map(|value| value.as_str().unwrap())
            .collect();
        assert_eq!(
            declared, expected,
            "model_surface_schema.enums[{path}] disagrees with this test's closed enum"
        );
    }

    for name in shipmates::adapters::targets() {
        let surface = harnesses[name]["model_surface"]
            .as_object()
            .unwrap_or_else(|| panic!("{name}: harness_matrix.json has no `model_surface`"));
        for key in REQUIRED_KEYS {
            assert!(
                surface.contains_key(key),
                "{name}: model_surface is missing `{key}` — every cell is stated, never left blank"
            );
        }
        let tier = cell(name, "discovery_tier", &surface["discovery_tier"]);
        assert!(
            DISCOVERY_TIERS.contains(&tier),
            "{name}: discovery_tier `{tier}` is outside the closed enum"
        );
        let override_kind = cell(
            name,
            "runtime_model_override.kind",
            &surface["runtime_model_override"]["kind"],
        );
        assert!(
            OVERRIDE_KINDS.contains(&override_kind),
            "{name}: runtime_model_override.kind `{override_kind}` is outside the closed enum"
        );
        let effort_kind = cell(name, "effort.kind", &surface["effort"]["kind"]);
        assert!(
            EFFORT_KINDS.contains(&effort_kind),
            "{name}: effort.kind `{effort_kind}` is outside the closed enum"
        );
        let enforcement = cell(
            name,
            "declared_pool.enforcement",
            &surface["declared_pool"]["enforcement"],
        );
        assert!(
            ENFORCEMENTS.contains(&enforcement),
            "{name}: declared_pool.enforcement `{enforcement}` is outside the closed enum"
        );

        // Every descriptive cell is a stated finding, never blank.
        non_empty(name, "identity", &surface["identity"]);
        non_empty(name, "notes", &surface["notes"]);
        non_empty(name, "enumeration.notes", &surface["enumeration"]["notes"]);
        non_empty(
            name,
            "runtime_model_override.notes",
            &surface["runtime_model_override"]["notes"],
        );
        non_empty(name, "effort.vocabulary", &surface["effort"]["vocabulary"]);
        non_empty(name, "effort.clamp", &surface["effort"]["clamp"]);
        non_empty(name, "effort.notes", &surface["effort"]["notes"]);
        non_empty(
            name,
            "declared_pool.mechanism",
            &surface["declared_pool"]["mechanism"],
        );
        non_empty(name, "declared_pool.notes", &surface["declared_pool"]["notes"]);

        let enumeration = surface["enumeration"]
            .as_object()
            .unwrap_or_else(|| panic!("{name}: enumeration is not an object"));
        let available = enumeration["available"]
            .as_bool()
            .unwrap_or_else(|| panic!("{name}: enumeration.available is not a boolean"));
        let command = enumeration["command"]
            .as_str()
            .unwrap_or_else(|| panic!("{name}: enumeration.command is not a string"));
        assert_eq!(
            available,
            !command.trim().is_empty(),
            "{name}: enumeration.command must be non-empty exactly when available is true — \
             a blank command must never read downstream as a pool"
        );
        let verified_on = cell(name, "verified_on", &surface["verified_on"]);
        assert!(
            date.is_match(verified_on),
            "{name}: verified_on `{verified_on}` is not YYYY-MM-DD"
        );
        let gaps = surface["reported_gaps"].as_array().unwrap_or_else(|| {
            panic!("{name}: reported_gaps must be an array (empty when nothing was reported)")
        });
        for (index, gap) in gaps.iter().enumerate() {
            for field in ["item", "label", "url"] {
                non_empty(
                    name,
                    &format!("reported_gaps[{index}].{field}"),
                    &gap[field],
                );
            }
            assert_eq!(
                cell(
                    name,
                    &format!("reported_gaps[{index}].label"),
                    &gap["label"]
                ),
                "reported-not-documented",
                "{name}: a reported gap must be labelled `reported-not-documented` — \
                 a report is never promoted to a fact"
            );
        }
    }
}

/// The model-routing ruleset is a *global* one. It is expanded from the shared
/// cost-discipline preamble, so **every** command carries it — not only the two
/// that spawn the most subagents. A marker left in a single command would make
/// the ruleset look installed everywhere while reaching only that command.
#[test]
fn test_every_command_carries_the_model_routing_ruleset() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let commands = load_commands(&root.join("commands")).unwrap();
    assert!(!commands.is_empty());
    let files = ClaudeCodeAdapter.build(&[], &commands).unwrap();

    // The ruleset is sourced once, from the shared cost-discipline preamble.
    let doctrine = std::fs::read_to_string(root.join("docs/COST.md")).unwrap();
    let preamble = doctrine
        .split_once("<!-- command-preamble:start -->")
        .expect("cost doctrine has no command-preamble start marker")
        .1
        .split_once("<!-- command-preamble:end -->")
        .expect("cost doctrine has no command-preamble end marker")
        .0;
    assert!(
        preamble.contains("<!-- shipmates:model-routing -->"),
        "the cost-discipline preamble must expand the model-routing ruleset"
    );

    for command in &commands {
        let path = format!(
            "harnesses/claude-code/.claude/skills/{}/SKILL.md",
            command.name
        );
        let rendered = files
            .get(&path)
            .unwrap_or_else(|| panic!("no rendered payload for {path}"));
        assert_eq!(
            rendered.matches("## Model routing").count(),
            1,
            "{}: every command must carry the model-routing ruleset exactly once",
            command.name
        );
        assert!(
            !rendered.contains("shipmates:model-routing"),
            "{}: the marker must not survive into the payload",
            command.name
        );
    }
}

/// The per-harness facts are hand-written in three places, and only the record is
/// gated. This checks the copy a captain actually reads: every row of the shipped
/// `## Model routing` table must agree with `tools/harness_matrix.json`
/// `model_surface`, for every target, on every axis the table carries. Without it
/// the shipped table is a fourth opinion that can go stale silently — which is how
/// the ADR's "three targets" drifted from the record's four.
#[test]
fn test_shipped_model_routing_table_matches_the_matrix() {
    /// The shipped cell leads with prose, then a `·` gloss. Cut the lead segment,
    /// and map it onto the record's enum by longest-prefix match so a cell may add
    /// words ("static agent file per subagent") without lying about its kind.
    fn leading(cell: &str) -> &str {
        let cut = cell
            .char_indices()
            .find(|(_, c)| matches!(c, ',' | ';' | '·'))
            .map(|(index, _)| index)
            .unwrap_or(cell.len());
        cell[..cut].trim()
    }
    fn enum_for(cells: &[(&str, &str)], cell: &str, column: &str, target: &str) -> String {
        let lead = leading(cell);
        cells
            .iter()
            .find(|(prefix, _)| lead.starts_with(prefix))
            .map(|(_, value)| (*value).to_string())
            .unwrap_or_else(|| {
                panic!(
                    "{target}: the shipped table's {column} cell `{cell}` leads with `{lead}`, which \
                     names no value in the record's enum — the shipped copy has drifted"
                )
            })
    }

    const OVERRIDE_CELLS: [(&str, &str); 3] = [
        ("per-spawn", "per-spawn"),
        ("static agent file", "static-agent-file"),
        ("session-level", "session-level"),
    ];
    const ENFORCEMENT_CELLS: [(&str, &str); 4] = [
        ("abort", "abort"),
        ("warn", "warn"),
        ("fallback", "fallback"),
        ("none", "none"),
    ];
    const EFFORT_CELLS: [(&str, &str); 4] = [
        ("separate key", "separate-key"),
        ("run-level", "run-level"),
        ("folded into the model string", "folded-into-model-string"),
        ("none", "none"),
    ];

    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let doctrine = std::fs::read_to_string(root.join("docs/COST.md")).unwrap();
    let block = doctrine
        .split_once("<!-- model-routing:start -->")
        .expect("the model-routing block has no start marker")
        .1
        .split_once("<!-- model-routing:end -->")
        .expect("the model-routing block has no end marker")
        .0;
    let matrix: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(root.join("tools/harness_matrix.json")).unwrap(),
    )
    .unwrap();
    let harnesses = matrix["harnesses"].as_object().unwrap();

    let mut shipped: Vec<String> = Vec::new();
    for line in block.lines() {
        let line = line.trim();
        if !line.starts_with('|') {
            continue;
        }
        let cells: Vec<&str> = line.trim_matches('|').split('|').map(str::trim).collect();
        if cells.len() != 5 || cells[0] == "Target" || cells[0].starts_with("---") {
            continue;
        }
        let target = cells[0];
        let surface = harnesses
            .get(target)
            .unwrap_or_else(|| panic!("the shipped table names `{target}`, which is no target"))["model_surface"]
            .clone();
        assert_eq!(
            cells[1],
            surface["discovery_tier"].as_str().unwrap(),
            "{target}: the shipped discovery tier disagrees with the record"
        );
        for (column, cell, expected) in [
            (
                "override",
                cells[2],
                enum_for(&OVERRIDE_CELLS, cells[2], "override", target),
            ),
            (
                "enforcement",
                cells[3],
                enum_for(&ENFORCEMENT_CELLS, cells[3], "enforcement", target),
            ),
            (
                "effort",
                cells[4],
                enum_for(&EFFORT_CELLS, cells[4], "effort", target),
            ),
        ] {
            let recorded = match column {
                "override" => surface["runtime_model_override"]["kind"].as_str().unwrap(),
                "enforcement" => surface["declared_pool"]["enforcement"].as_str().unwrap(),
                _ => surface["effort"]["kind"].as_str().unwrap(),
            };
            assert_eq!(
                expected, recorded,
                "{target}: the shipped {column} cell `{cell}` disagrees with the record"
            );
        }
        shipped.push(target.to_string());
    }

    let mut expected: Vec<String> = shipmates::adapters::targets()
        .iter()
        .map(|target| (*target).to_string())
        .collect();
    expected.sort();
    shipped.sort();
    assert_eq!(
        shipped, expected,
        "the shipped per-target table must carry exactly one row per target"
    );
}

/// The frontmatter text between the opening and closing `---` lines, for a file
/// that begins with a frontmatter block.
fn frontmatter_block(content: &str) -> Option<&str> {
    let rest = content.strip_prefix("---\n")?;
    let mut offset = 0;
    for line in rest.split_inclusive('\n') {
        if line.trim_end() == "---" {
            return Some(&rest[..offset]);
        }
        offset += line.len();
    }
    None
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
