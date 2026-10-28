use shipmates::adapters::Adapter;
use shipmates::adapters::antigravity::AntigravityAdapter;
use shipmates::adapters::claude_code::ClaudeCodeAdapter;
use shipmates::adapters::codex::CodexAdapter;
use shipmates::adapters::opencode::OpencodeAdapter;
use shipmates::catalog::{
    load_commands, load_roles, load_tools, reject_positional, CanonicalCommand, CanonicalRole,
};
use shipmates::digest;
use std::collections::{BTreeMap, BTreeSet, HashMap};
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

/// This verifies installed-artifact presence, format, and identity only. It does
/// NOT verify that a harness runtime discovers or loads these artifacts (#542).
#[test]
fn test_keyless_installed_tree_fidelity_all_targets() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let generator = std::fs::read_to_string(root.join("tools/gen_command_pages.py")).unwrap();
    let slug_block = generator
        .split_once("SLUGS = (")
        .and_then(|(_, rest)| rest.split_once(')'))
        .map(|(slugs, _)| slugs)
        .expect("tools/gen_command_pages.py must define SLUGS");
    let command_slugs: Vec<String> = slug_block
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| {
            line.trim()
                .strip_prefix('\"')
                .and_then(|line| line.strip_suffix("\","))
                .unwrap_or_else(|| panic!("invalid command slug in generator: {line}"))
                .to_string()
        })
        .collect();
    let expected_commands: BTreeSet<String> = command_slugs.iter().cloned().collect();
    assert_eq!(command_slugs.len(), 19, "canonical command list changed");
    assert_eq!(
        expected_commands.len(),
        command_slugs.len(),
        "command slugs must be unique"
    );
    let command_sources: BTreeSet<String> = load_commands(&root.join("commands"))
        .unwrap()
        .into_iter()
        .map(|command| command.name)
        .collect();
    assert_eq!(
        expected_commands, command_sources,
        "generator SLUGS and command sources differ"
    );

    let expected_roles: BTreeSet<String> = direct_files_by_suffix(&root.join("crew"), ".md")
        .keys()
        .cloned()
        .collect();
    let role_sources: BTreeSet<String> = load_roles(&root.join("crew"))
        .unwrap()
        .into_iter()
        .map(|role| role.name)
        .collect();
    assert_eq!(expected_roles.len(), 13, "canonical crew set changed");
    assert_eq!(
        expected_roles, role_sources,
        "crew filenames and role identities differ"
    );

    let manifest: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(root.join("tools/manifest.json")).unwrap())
            .expect("tools/manifest.json must be valid JSON");
    let manifest_targets = manifest
        .get("targets")
        .and_then(serde_json::Value::as_array)
        .expect("tools/manifest.json must have a targets array");
    let targets: Vec<String> = manifest_targets
        .iter()
        .map(|target| {
            target
                .as_str()
                .expect("manifest target names must be strings")
                .to_string()
        })
        .collect();
    let target_set: BTreeSet<String> = targets.iter().cloned().collect();
    assert_eq!(
        target_set.len(),
        targets.len(),
        "manifest targets must be unique"
    );
    assert_eq!(
        targets.len(),
        9,
        "installed-tree coverage must include all nine targets"
    );
    let adapter_targets: BTreeSet<String> = shipmates::adapters::targets()
        .iter()
        .map(|target| (*target).to_string())
        .collect();
    assert_eq!(
        target_set, adapter_targets,
        "manifest and implemented targets differ"
    );

    let empty_cwd = tempfile::tempdir().unwrap();
    for target in targets {
        assert!(
            walk(empty_cwd.path()).is_empty(),
            "install cwd must stay empty"
        );
        let sandbox = tempfile::tempdir().unwrap();
        let output = std::process::Command::new(env!("CARGO_BIN_EXE_shipmates"))
            // No checkout source is visible here. `install` must use the payload
            // embedded in the test binary, as a packaged CLI does.
            .current_dir(empty_cwd.path())
            .args([
                "install",
                "--harness",
                target.as_str(),
                "--dir",
                sandbox.path().to_str().unwrap(),
                "--with-tools",
                "none",
            ])
            .output()
            .unwrap_or_else(|error| panic!("failed to execute {target} install: {error}"));
        assert!(
            output.status.success(),
            "{target} install failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(
            walk(empty_cwd.path()).is_empty(),
            "{target} install must not use the current working directory"
        );

        let (command_dir_rel, skill_tree) = match target.as_str() {
            "claude-code" => (".claude/skills", true),
            "opencode" => (".opencode/commands", false),
            "antigravity" | "codex" | "github-copilot" => (".agents/skills", true),
            "cursor" => (".cursor/skills", true),
            "pi" => (".pi/skills", true),
            "grok-build" => (".grok/skills", true),
            "devin" => (".devin/skills", true),
            unknown => panic!("no installed command layout for manifest target {unknown}"),
        };
        let command_dir = sandbox.path().join(command_dir_rel);
        if skill_tree {
            assert_eq!(
                directory_names(&command_dir),
                expected_commands,
                "{target} installed skill directory set differs from canonical commands"
            );
            for identity in &expected_commands {
                let skill_dir = command_dir.join(identity);
                let path = skill_dir.join("SKILL.md");
                assert_eq!(
                    path.parent(),
                    Some(skill_dir.as_path()),
                    "{target} skill path must be nested under its identity"
                );
                let files = walk(&skill_dir);
                assert_eq!(
                    files,
                    vec![path.clone()],
                    "{target} {identity} skill has unexpected files"
                );
                let parsed = assert_installed_markdown_identity(&path, identity, &target, true);
                if target == "grok-build" {
                    assert_eq!(
                        parsed
                            .get("disable-model-invocation")
                            .and_then(serde_yaml::Value::as_bool),
                        Some(true),
                        "{target} {identity} must keep the user-invoked-only guard"
                    );
                }
                if target == "devin" {
                    let has_user_trigger = parsed
                        .get("triggers")
                        .and_then(serde_yaml::Value::as_sequence)
                        .is_some_and(|triggers| {
                            triggers
                                .iter()
                                .any(|trigger| trigger.as_str() == Some("user"))
                        });
                    assert!(
                        has_user_trigger,
                        "{target} {identity} must keep the user trigger"
                    );
                }
            }
        } else {
            let commands = direct_files_by_suffix(&command_dir, ".md");
            assert_eq!(
                commands.keys().cloned().collect::<BTreeSet<_>>(),
                expected_commands,
                "{target} installed command file set differs from canonical commands"
            );
            for (identity, path) in &commands {
                assert_eq!(path.parent(), Some(command_dir.as_path()));
                assert_installed_markdown_identity(path, identity, &target, false);
            }
            let report_order = std::fs::read_to_string(command_dir.join("shipmates-harden.md"))
                .expect("OpenCode harden command must be installed");
            assert!(
                report_order.contains("report"),
                "harden order lost report-only mode"
            );
            assert!(
                report_order.contains("$ARGUMENTS"),
                "harden order lost argument passing"
            );
            assert!(
                !report_order.contains("{{"),
                "neutral argument placeholder leaked"
            );
        }

        let (agent_dir_rel, agent_layout) = match target.as_str() {
            "claude-code" => (Some(".claude/agents"), "markdown"),
            "opencode" => (Some(".opencode/agents"), "opencode"),
            "antigravity" => (Some(".agents/agents"), "antigravity"),
            "codex" => (Some(".codex/agents"), "codex"),
            "cursor" => (None, "none"),
            "github-copilot" => (Some(".github/agents"), "copilot"),
            "pi" => (Some(".pi/agents"), "markdown"),
            "grok-build" => (Some(".grok/agents"), "markdown"),
            "devin" => (Some(".devin/agents"), "markdown"),
            unknown => panic!("no installed crew layout for manifest target {unknown}"),
        };
        if agent_layout == "none" {
            assert!(!sandbox.path().join(".cursor/agents").exists());
            assert!(!sandbox.path().join(".agents/skills").exists());
        } else {
            let agent_dir = sandbox.path().join(agent_dir_rel.unwrap());
            if agent_layout == "antigravity" {
                assert_eq!(
                    directory_names(&agent_dir),
                    expected_roles,
                    "{target} must use one per-role directory"
                );
                for identity in &expected_roles {
                    let role_dir = agent_dir.join(identity);
                    let path = role_dir.join("agent.md");
                    assert_eq!(
                        walk(&role_dir),
                        vec![path.clone()],
                        "{target} {identity} must be the sole artifact in its role directory"
                    );
                    assert_installed_markdown_identity(&path, identity, &target, true);
                }
            } else {
                let suffix = match agent_layout {
                    "codex" => ".toml",
                    "copilot" => ".agent.md",
                    _ => ".md",
                };
                let agents = direct_files_by_suffix(&agent_dir, suffix);
                assert_eq!(
                    agents.keys().cloned().collect::<BTreeSet<_>>(),
                    expected_roles,
                    "{target} installed crew set differs from canonical roles"
                );
                for (identity, path) in &agents {
                    assert_eq!(path.parent(), Some(agent_dir.as_path()));
                    if agent_layout == "codex" {
                        assert_installed_codex_identity(path, identity, &target);
                    } else {
                        let parsed = assert_installed_markdown_identity(
                            path,
                            identity,
                            &target,
                            agent_layout != "opencode",
                        );
                        if agent_layout == "opencode" {
                            assert_eq!(
                                parsed.get("mode").and_then(serde_yaml::Value::as_str),
                                Some("subagent"),
                                "{target} {identity} must retain subagent mode"
                            );
                            assert!(
                                parsed
                                    .get("permission")
                                    .and_then(serde_yaml::Value::as_mapping)
                                    .is_some_and(|permission| !permission.is_empty()),
                                "{target} {identity} must retain a permission map"
                            );
                        }
                    }
                }
            }
        }

        let receipt = sandbox
            .path()
            .join(".shipmates/receipts")
            .join(format!("{target}.json"));
        assert!(
            receipt.is_file(),
            "{target} install must write its receipt"
        );
    }
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

    assert_eq!(commands.len(), 19, "cost preamble must cover every command");
    let re_tokens = regex::Regex::new(r"\{\{[a-zA-Z:-]+\}\}").unwrap();
    for command in &commands {
        assert_eq!(
            command.narrative.matches("<!-- shipmates:bootstrap -->").count(),
            1,
            "{} must reference shared bootstrap once",
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
                > command.narrative.find("<!-- shipmates:bootstrap -->").unwrap(),
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
            role.body.matches("<!-- shipmates:bootstrap -->").count(),
            1,
            "{} must reference shared bootstrap once",
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
            // The bootstrap is rendered as a short pointer because every harness
            // auto-loads the full shared principles from its instructions file.
            assert!(
                content.contains("Shared Shipmates bootstrap principles"),
                "{target} {path} missed bootstrap pointer"
            );
            assert!(!content.contains("<!-- shipmates:bootstrap -->"), "{target} {path} leaked bootstrap marker");
            assert!(!content.contains("shipmates:command-preamble"), "{target} {path} leaked command marker");
            assert!(!content.contains("shipmates:acceptance-board"), "{target} {path} leaked board marker");
            assert!(
                !content.contains("shipmates:epic-integration-board"),
                "{target} {path} leaked epic integration board marker"
            );
            assert!(
                !content.contains("shipmates:why-merge-pr"),
                "{target} {path} leaked why-merge-pr marker"
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
            assert!(
                content.contains("Shared Shipmates bootstrap principles"),
                "{target} {path} missed bootstrap pointer"
            );
            assert!(!content.contains("<!-- shipmates:bootstrap -->"), "{target} {path} leaked bootstrap marker");
        }
    }
}

/// Every harness must be able to render or install the shared bootstrap prose
/// without panicking. This is Slice A of the bootstrap refactor: the bootstrap
/// file exists, the render layer can expand it, and the steering layer can
/// install it through a project-scope instructions file.
#[test]
fn test_bootstrap_renders_or_installs_for_every_target() {
    let bootstrap = shipmates::adapters::render::bootstrap_content();
    assert!(!bootstrap.is_empty());
    assert!(
        bootstrap.contains("## Cost discipline"),
        "bootstrap must contain cost discipline"
    );
    assert!(
        bootstrap.contains("## Argument intake"),
        "bootstrap must contain argument intake"
    );
    assert!(
        bootstrap.contains("## Model routing"),
        "bootstrap must contain model routing"
    );
    assert!(
        bootstrap.contains("## Reusable acceptance board"),
        "bootstrap must contain acceptance board"
    );
    assert!(
        bootstrap.contains("## Reusable epic integration board"),
        "bootstrap must contain epic integration board"
    );
    assert!(
        bootstrap.contains("## Reusable subagent preamble"),
        "bootstrap must contain subagent preamble"
    );
    assert!(
        bootstrap.contains("## Why merge this (PR body)"),
        "bootstrap must contain why-merge-pr"
    );

    let dialects = [
        ("claude-code", &shipmates::adapters::render::CLAUDE_CODE),
        ("opencode", &shipmates::adapters::render::OPENCODE),
        ("antigravity", &shipmates::adapters::render::ANTIGRAVITY),
        ("codex", &shipmates::adapters::render::CODEX),
        ("cursor", &shipmates::adapters::render::AGENT_SKILLS),
        (
            "github-copilot",
            &shipmates::adapters::render::GITHUB_COPILOT,
        ),
        ("pi", &shipmates::adapters::render::PI),
        ("grok-build", &shipmates::adapters::render::GROK_BUILD),
        ("devin", &shipmates::adapters::render::DEVIN),
    ];

    for (name, d) in dialects {
        let rendered = shipmates::adapters::render::render_body("<!-- shipmates:bootstrap -->", d);
        assert!(
            !rendered.contains("<!-- shipmates:bootstrap -->"),
            "{name} leaked bootstrap marker"
        );
        match d.bootstrap_mode {
            shipmates::adapters::render::BootstrapMode::Inline => {
                assert!(
                    rendered.contains("## Cost discipline"),
                    "{name} inline bootstrap missing cost discipline"
                );
            }
            shipmates::adapters::render::BootstrapMode::Pointer => {
                assert!(
                    rendered.contains(d.instructions_primary),
                    "{name} pointer bootstrap missing instructions reference"
                );
            }
            shipmates::adapters::render::BootstrapMode::Omit => {
                assert!(
                    rendered.trim().is_empty(),
                    "{name} omit bootstrap was not empty"
                );
            }
        }
    }

    // Project-scope install for GitHub Copilot, the only TierC harness.
    let project = tempfile::tempdir().unwrap();
    let instructions_dir = project.path().join(".github").join("instructions");
    std::fs::create_dir_all(&instructions_dir).unwrap();
    let path = instructions_dir.join("shipmates.instructions.md");
    std::fs::write(&path, "# Existing project instructions\n\nKeep me.\n").unwrap();

    let rendered = shipmates::adapters::render::render_instructions(
        bootstrap,
        shipmates::adapters::render::GITHUB_COPILOT.instructions_primary,
        shipmates::adapters::render::GITHUB_COPILOT.instructions_fallback,
    );
    let outcome =
        shipmates::steering::install_project_steering("github-copilot", project.path(), &rendered)
            .unwrap();
    assert!(
        matches!(outcome, shipmates::steering::SteeringOutcome::Updated(_)),
        "copilot project steering should be updated"
    );
    let content = std::fs::read_to_string(&path).unwrap();
    assert!(
        content.contains("Keep me."),
        "copilot project steering must preserve existing content"
    );
    assert!(
        content.contains("<!-- shipmates:global-steering -->"),
        "copilot project steering must contain managed block start"
    );
    assert!(
        content.contains("## Cost discipline"),
        "copilot project steering must contain bootstrap content"
    );
    assert!(
        content.contains("AGENTS.md"),
        "copilot project steering must render instructions token"
    );

    let outcome2 =
        shipmates::steering::install_project_steering("github-copilot", project.path(), &rendered)
            .unwrap();
    assert!(
        matches!(outcome2, shipmates::steering::SteeringOutcome::Unchanged(_)),
        "copilot project steering should be idempotent"
    );

    let outcome3 =
        shipmates::steering::uninstall_project_steering("github-copilot", project.path()).unwrap();
    assert!(
        matches!(outcome3, shipmates::steering::SteeringOutcome::Removed(_)),
        "copilot project steering should be removed"
    );
    let after = std::fs::read_to_string(&path).unwrap();
    assert!(
        !after.contains("<!-- shipmates:global-steering -->"),
        "copilot project steering must remove managed block"
    );
    assert!(
        after.contains("Keep me."),
        "copilot project steering must keep existing content after uninstall"
    );
}

/// The #407 defect was frontmatter that installed cleanly and was never
/// discovered because a strict loader rejected it. So: strict-parse every
/// emitted frontmatter block, keep every artifact's `name:` bare and equal to
/// the identity its install path implies (what `adopt::frontmatter_name_matches`
/// reads), and pin the failure mode with a negative control.
///
/// Codex crew are standalone TOML (`name = "architect"`), not YAML frontmatter.
/// Those files are parsed by the emitted shape (toml_basic keys + a toml_literal
/// `developer_instructions`) rather than skipped — a toml-only regression cannot
/// hide behind YAML `parsed_blocks`.
#[test]
fn test_emitted_frontmatter_strict_parses_and_names_stay_bare() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let roles = load_roles(&root.join("crew")).unwrap();
    let commands = load_commands(&root.join("commands")).unwrap();
    let tools = load_tools(&root.join("toolbox")).unwrap();
    assert_eq!(commands.len(), 19);
    assert!(!tools.is_empty(), "toolbox/ must hold tools for build_tools");

    // Negative control: `description: Shipmates: take an issue` is a nested
    // mapping to YAML and fails a strict parse; the double-quoted style is what
    // this change exists to ship.
    assert!(
        serde_yaml::from_str::<serde_yaml::Value>(
            "name: shipmates-ship-epic\ndescription: Shipmates: take an issue\n"
        )
        .is_err(),
        "an unquoted `: ` description must fail a strict YAML parse"
    );
    assert!(
        serde_yaml::from_str::<serde_yaml::Value>(
            "name: shipmates-ship-epic\ndescription: \"Shipmates: take an issue\"\n"
        )
        .is_ok(),
        "the double-quoted equivalent must parse"
    );

    let mut saw_shipmates_description = false;
    for target in shipmates::adapters::targets() {
        let adapter = shipmates::adapters::select(target).unwrap();
        let mut files = adapter.build(&roles, &commands).unwrap();
        files.extend(adapter.build_steering("steer"));
        files.extend(adapter.build_tools(&tools));

        // All eighteen commands must arrive exactly once, as a skill or as
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
        let mut parsed_toml = 0;
        let toml_emitted = files.keys().filter(|path| path.ends_with(".toml")).count();
        for (path, content) in &files {
            if path.ends_with(".toml") {
                let parsed = parse_emitted_codex_toml(content).unwrap_or_else(|error| {
                    panic!("{target} {path}: Codex TOML is not the emitted shape: {error}\n{content}")
                });
                for key in ["name", "description", "developer_instructions"] {
                    assert!(
                        parsed.get(key).is_some_and(|value| !value.is_empty()),
                        "{target} {path}: required TOML key `{key}` missing or empty"
                    );
                }
                parsed_toml += 1;
                if let Some(identity) =
                    shipmates::installer::adopt::artifact_name(std::path::Path::new(path))
                {
                    assert_eq!(
                        parsed.get("name").map(String::as_str),
                        Some(identity.as_str()),
                        "{target} {path}: TOML `name` must equal the path identity"
                    );
                }
                continue;
            }
            // Cursor steering is `.mdc`; Copilot steering is `.instructions.md`.
            // Both carry YAML frontmatter and must parse the same way as `.md`.
            if !(path.ends_with(".md")
                || path.ends_with(".mdc")
                || path.ends_with(".instructions.md"))
            {
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
        assert_eq!(
            parsed_toml, toml_emitted,
            "{target} skipped a .toml agent ({parsed_toml} parsed, {toml_emitted} emitted)"
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
        "grok-build",
        "devin",
    ] {
        assert!(stdout.contains(target), "targets output missing {target}");
    }
}

#[test]
fn test_non_claude_targets_build_via_cli() {
    let temp_dir = tempfile::tempdir().unwrap();
    for target in [
        "codex",
        "cursor",
        "github-copilot",
        "pi",
        "grok-build",
        "devin",
    ] {
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
    // Harnesses that read the open Agent Skills tree ship skills to the
    // shared `.agents/skills/` location.
    let codex_skill = temp_dir
        .path()
        .join("harnesses/codex/.agents/skills/shipmates-ship-issue/SKILL.md");
    assert!(codex_skill.is_file(), "codex shipmates-ship-issue skill not emitted");
    let copilot_skill = temp_dir
        .path()
        .join("harnesses/github-copilot/.agents/skills/shipmates-ship-issue/SKILL.md");
    assert!(
        copilot_skill.is_file(),
        "copilot shipmates-ship-issue skill not emitted"
    );
    // Pi ships to its native `.pi/skills/` tree
    let pi_skill = temp_dir
        .path()
        .join("harnesses/pi/.pi/skills/shipmates-ship-issue/SKILL.md");
    assert!(pi_skill.is_file(), "pi shipmates-ship-issue skill not emitted");
    let pi_content = std::fs::read_to_string(&pi_skill).unwrap();
    assert!(pi_content.contains("allowed-tools:"), "pi must carry allowed-tools: {pi_content}");
    assert!(pi_content.contains("disable-model-invocation: true"), "pi must carry guard: {pi_content}");
    assert!(pi_content.contains("/skill:"), "pi must use /skill: routing: {pi_content}");

    // ...and the shared rendering is byte-identical across the shared harnesses.
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

/// #482/#483: contributor-tree `install --harness pi` writes
/// `.shipmates/contributor-steering.md` and claims it on the receipt.
#[test]
fn test_pi_contributor_install_writes_steering_and_receipt() {
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path();
    std::fs::create_dir_all(target.join("commands")).unwrap();
    std::fs::write(target.join("commands/shipmates-ship-issue.md"), "---\n---\n").unwrap();
    std::fs::create_dir_all(target.join("toolbox")).unwrap();
    std::fs::create_dir_all(target.join("tools")).unwrap();
    std::fs::write(target.join("tools/gen_command_pages.py"), "# gen").unwrap();

    let output = std::process::Command::new(env!("CARGO_BIN_EXE_shipmates"))
        .args([
            "install",
            "--harness",
            "pi",
            "--dir",
            target.to_str().unwrap(),
            "--with-tools",
            "none",
        ])
        .output()
        .expect("failed to execute shipmates install --harness pi");
    assert!(
        output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let steering = target.join(".shipmates/contributor-steering.md");
    assert!(
        steering.is_file(),
        "pi contributor install must write {}",
        steering.display()
    );
    let receipt_path = target.join(".shipmates/receipts/pi.json");
    let receipt: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&receipt_path).unwrap()).unwrap();
    let claimed = receipt["files"]
        .as_array()
        .unwrap()
        .iter()
        .any(|file| file["path"] == ".shipmates/contributor-steering.md");
    assert!(
        claimed,
        "pi receipt must claim .shipmates/contributor-steering.md: {receipt}"
    );
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
        global_out.contains("~/.pi/agent/") && global_out.contains("/skill:"),
        "missing pi home-install guidance: {global_out}"
    );
    assert!(
        home.path()
            .join(".pi/agent/skills/shipmates-ship-issue/SKILL.md")
            .is_file(),
        "global pi install must write command skills to ~/.pi/agent/skills/"
    );
    assert!(
        home.path().join(".pi/agent/agents/architect.md").is_file(),
        "global pi install must write crew to ~/.pi/agent/agents/"
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
        !local_out.contains("nearest ancestor") && !local_out.contains("shadow"),
        "project-local pi install must not print the home-shadow hint: {local_out}"
    );
    assert!(
        project
            .path()
            .join(".pi/skills/shipmates-ship-issue/SKILL.md")
            .is_file(),
        "project pi install must write command skills to .pi/skills/"
    );
    assert!(
        project.path().join(".pi/agents/architect.md").is_file(),
        "project pi install must write crew to .pi/agents/"
    );
}


/// #548: A global Pi install ships command and tool skills to ~/.pi/agent/skills/,
/// and crew to ~/.pi/agent/agents/. When both a global and project Pi install
/// exist, doctor warns about the overlapping skill trees (#513).
#[test]
fn test_pi_global_and_project_installs_ship_to_their_respective_roots() {
    let home = tempfile::tempdir().unwrap();
    let project = home.path().join("workdir");
    std::fs::create_dir_all(&project).unwrap();

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
        .expect("global pi install");
    assert!(
        global.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&global.stdout),
        String::from_utf8_lossy(&global.stderr)
    );
    assert!(
        home.path()
            .join(".pi/agent/skills/shipmates-ship-issue/SKILL.md")
            .is_file(),
        "global pi install must ship skills to ~/.pi/agent/skills/"
    );
    assert!(
        home.path().join(".pi/agent/agents/architect.md").is_file(),
        "global pi install must ship crew to ~/.pi/agent/agents/"
    );

    let local = std::process::Command::new(env!("CARGO_BIN_EXE_shipmates"))
        .env("HOME", home.path())
        .args([
            "install",
            "--harness",
            "pi",
            "--dir",
            project.to_str().unwrap(),
            "--with-tools",
            "none",
        ])
        .output()
        .expect("project pi install");
    assert!(
        local.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&local.stdout),
        String::from_utf8_lossy(&local.stderr)
    );
    assert!(
        project
            .join(".pi/skills/shipmates-ship-issue/SKILL.md")
            .is_file(),
        "project pi install must ship skills to .pi/skills/"
    );
    assert!(
        project.join(".pi/agents/architect.md").is_file(),
        "project pi install must ship crew to .pi/agents/"
    );

    let doc = std::process::Command::new(env!("CARGO_BIN_EXE_shipmates"))
        .env("HOME", home.path())
        .args([
            "doctor",
            "--harness",
            "pi",
            "--dir",
            project.to_str().unwrap(),
        ])
        .output()
        .expect("doctor check");
    let doc_out = String::from_utf8_lossy(&doc.stdout);
    assert!(
        doc_out.contains("Pi skill trees") && doc_out.contains("shipmates-ship-issue"),
        "doctor must warn when global and project pi skill trees overlap (#513): {doc_out}"
    );
}

/// #553, #554: Pi native skills in project carry tool scoping, user guard,
/// and /skill: routing.
#[test]
fn test_pi_project_install_ships_native_skills_with_scoping() {
    let home = tempfile::tempdir().unwrap();
    let project = home.path().join("workdir");
    std::fs::create_dir_all(&project).unwrap();

    let local = std::process::Command::new(env!("CARGO_BIN_EXE_shipmates"))
        .env("HOME", home.path())
        .args([
            "install",
            "--harness",
            "pi",
            "--dir",
            project.to_str().unwrap(),
            "--with-tools",
            "none",
        ])
        .output()
        .expect("project pi install");
    assert!(local.status.success());

    let skill_path = project.join(".pi/skills/shipmates-ship-issue/SKILL.md");
    let content = std::fs::read_to_string(&skill_path).unwrap();
    assert!(
        content.contains("allowed-tools: bash read write edit subagent grep find web_search fetch_content\n"),
        "{content}"
    );
    assert!(
        content.contains("disable-model-invocation: true\n"),
        "{content}"
    );
    assert!(
        content.contains("argument-hint: "),
        "{content}"
    );
    assert!(content.contains("/skill:shipmates-"), "{content}");
}

/// #548, #554: Project Pi and a shared-tree sibling (Antigravity) install to
/// their respective native trees (.pi/skills and .agents/skills).
#[test]
fn test_project_pi_and_shared_tree_sibling_install_to_their_respective_trees() {
    let home = tempfile::tempdir().unwrap();
    let project = home.path().join("workdir");
    std::fs::create_dir_all(&project).unwrap();

    for harness in ["pi", "antigravity"] {
        let out = std::process::Command::new(env!("CARGO_BIN_EXE_shipmates"))
            .env("HOME", home.path())
            .args([
                "install",
                "--harness",
                harness,
                "--dir",
                project.to_str().unwrap(),
                "--with-tools",
                "none",
            ])
            .output()
            .unwrap_or_else(|_| panic!("{harness} project install"));
        assert!(
            out.status.success(),
            "{harness}: stdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
    }

    assert!(
        project.join(".pi/skills/shipmates-ship-issue/SKILL.md").is_file(),
        "Pi native skills must land at .pi/skills/"
    );
    assert!(
        project.join(".agents/skills/shipmates-ship-issue/SKILL.md").is_file(),
        "Antigravity shared skills must land at .agents/skills/"
    );

    let doc = std::process::Command::new(env!("CARGO_BIN_EXE_shipmates"))
        .env("HOME", home.path())
        .args([
            "doctor",
            "--harness",
            "pi",
            "--dir",
            project.to_str().unwrap(),
        ])
        .output()
        .expect("doctor check");
    let doc_out = String::from_utf8_lossy(&doc.stdout);
    assert!(
        doc_out.contains("Pi skill trees") && doc_out.contains("shipmates-ship-issue"),
        "doctor must warn when pi skills and shared skills both exist in project: {doc_out}"
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
/// negative test for antigravity/github-copilot/cursor/devin: their
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
        schema["empty_surface_fallback"].as_str(),
        Some("inherit"),
        "the schema must record `inherit` as the fallback when the target offers nothing to read"
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
        ("effort.clamp_kind", &CLAMP_KINDS[..]),
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
        // The verdict and the prose beside it are one claim stated twice. A
        // `clamps-down` row whose prose documents nothing (or the reverse) is a
        // record disagreeing with itself — and the shipped-clause guard reads
        // the verdict, so unchecked prose drift would gate real cells against a
        // stale claim (#485).
        let clamp_kind = cell(name, "effort.clamp_kind", &surface["effort"]["clamp_kind"]);
        assert!(
            CLAMP_KINDS.contains(&clamp_kind),
            "{name}: effort.clamp_kind `{clamp_kind}` is outside the closed enum"
        );
        let clamp_prose = cell(name, "effort.clamp", &surface["effort"]["clamp"]);
        if clamp_kind == "clamps-down" {
            assert!(
                !clamp_is_hedged(clamp_prose),
                "{name}: effort.clamp_kind says the harness clamps down, but its clamp prose \
                 documents no clamp: `{clamp_prose}`"
            );
        } else {
            assert!(
                clamp_is_hedged(clamp_prose),
                "{name}: effort.clamp_kind `{clamp_kind}` records no clamp, but its clamp prose \
                 does not say so: `{clamp_prose}`"
            );
        }
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

/// Every harness must carry a complete `runtime_verified` record (#497). Live
/// runtime status used to live only in prose, so it went stale silently — the
/// same failure mode the `agents` / `effort` / `model_surface` guards already
/// prevent. `unknown` is a stated finding; promoting it to `yes` without
/// evidence is the defect this check exists to catch at the schema layer.
#[test]
fn test_matrix_runtime_verified_is_complete() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let matrix: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(root.join("tools/harness_matrix.json")).unwrap(),
    )
    .unwrap();
    let schema = matrix["runtime_verified_schema"]
        .as_object()
        .expect("harness_matrix.json has no runtime_verified_schema block");

    let harnesses = matrix["harnesses"]
        .as_object()
        .expect("harness_matrix.json has no harnesses map");

    const STATUSES: [&str; 3] = ["full", "partial", "none"];
    const CREW: [&str; 4] = ["yes", "no", "n/a", "unknown"];
    const TRI: [&str; 3] = ["yes", "no", "unknown"];
    const REQUIRED_KEYS: [&str; 7] = [
        "status",
        "verified_on",
        "crew_resolve",
        "argument_passing",
        "command_e2e",
        "commands_exercised",
        "notes",
    ];
    let date = Regex::new(r"^\d{4}-\d{2}-\d{2}$").unwrap();

    for (path, expected) in [
        ("status", &STATUSES[..]),
        ("crew_resolve", &CREW[..]),
        ("argument_passing", &TRI[..]),
        ("command_e2e", &TRI[..]),
    ] {
        let declared: Vec<&str> = schema["enums"][path]
            .as_array()
            .unwrap_or_else(|| panic!("runtime_verified_schema.enums has no `{path}`"))
            .iter()
            .map(|value| value.as_str().unwrap())
            .collect();
        assert_eq!(
            declared, expected,
            "runtime_verified_schema.enums[{path}] disagrees with this test's closed enum"
        );
    }

    for name in shipmates::adapters::targets() {
        let row = harnesses[name]["runtime_verified"]
            .as_object()
            .unwrap_or_else(|| panic!("{name}: harness_matrix.json has no `runtime_verified`"));
        for key in REQUIRED_KEYS {
            assert!(
                row.contains_key(key),
                "{name}: runtime_verified is missing `{key}` — every cell is stated, never left blank"
            );
        }

        let status = row["status"].as_str().unwrap_or_else(|| {
            panic!("{name}: runtime_verified.status is missing or not a string")
        });
        assert!(
            STATUSES.contains(&status),
            "{name}: status `{status}` is outside the closed enum"
        );

        let crew = row["crew_resolve"].as_str().unwrap_or_else(|| {
            panic!("{name}: runtime_verified.crew_resolve is missing or not a string")
        });
        assert!(
            CREW.contains(&crew),
            "{name}: crew_resolve `{crew}` is outside the closed enum"
        );

        let args = row["argument_passing"].as_str().unwrap_or_else(|| {
            panic!("{name}: runtime_verified.argument_passing is missing or not a string")
        });
        assert!(
            TRI.contains(&args),
            "{name}: argument_passing `{args}` is outside the closed enum"
        );

        let e2e = row["command_e2e"].as_str().unwrap_or_else(|| {
            panic!("{name}: runtime_verified.command_e2e is missing or not a string")
        });
        assert!(
            TRI.contains(&e2e),
            "{name}: command_e2e `{e2e}` is outside the closed enum"
        );

        let verified_on = row["verified_on"].as_str().unwrap_or_else(|| {
            panic!("{name}: runtime_verified.verified_on is missing or not a string")
        });
        let commands = row["commands_exercised"]
            .as_array()
            .unwrap_or_else(|| panic!("{name}: commands_exercised must be an array"));
        let notes = row["notes"].as_str().unwrap_or_else(|| {
            panic!("{name}: runtime_verified.notes is missing or not a string")
        });
        assert!(
            !notes.trim().is_empty(),
            "{name}: runtime_verified.notes is blank"
        );

        // Skills-only adapters cannot claim crew_resolve=yes.
        let agents = harnesses[name]["agents"]
            .as_bool()
            .unwrap_or_else(|| panic!("{name}: agents flag missing"));
        if !agents {
            assert_eq!(
                crew, "n/a",
                "{name}: skills-only adapter must set crew_resolve=n/a, not `{crew}`"
            );
        }

        match status {
            "none" => {
                assert!(
                    verified_on.is_empty(),
                    "{name}: status=none requires empty verified_on, got `{verified_on}`"
                );
                assert!(
                    commands.is_empty(),
                    "{name}: status=none requires empty commands_exercised"
                );
            }
            "full" => {
                assert!(
                    date.is_match(verified_on),
                    "{name}: status=full requires YYYY-MM-DD verified_on, got `{verified_on}`"
                );
                assert!(
                    matches!(crew, "yes" | "n/a"),
                    "{name}: status=full requires crew_resolve yes|n/a, got `{crew}`"
                );
                assert_eq!(args, "yes", "{name}: status=full requires argument_passing=yes");
                assert_eq!(e2e, "yes", "{name}: status=full requires command_e2e=yes");
                assert!(
                    !commands.is_empty(),
                    "{name}: status=full requires at least one commands_exercised entry"
                );
            }
            "partial" => {
                assert!(
                    date.is_match(verified_on),
                    "{name}: status=partial requires YYYY-MM-DD verified_on, got `{verified_on}`"
                );
            }
            _ => unreachable!(),
        }
    }
}

/// Docs/content ACs on the acceptance board require a machine-checkable pin
/// or an explicit `manual-only` label (#528). Silent ACCEPT from reading the
/// page is forbidden; this test fails if that sentence is deleted from COST.md.
#[test]
fn test_acceptance_board_docs_acs_require_machine_pin_or_manual_only() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let doctrine = std::fs::read_to_string(root.join("docs/COST.md")).unwrap();
    let board = doctrine
        .split_once("<!-- acceptance-board:start -->")
        .expect("cost doctrine has no acceptance-board start marker")
        .1
        .split_once("<!-- acceptance-board:end -->")
        .expect("cost doctrine has no acceptance-board end marker")
        .0;
    assert!(
        board.contains("machine-checkable pin"),
        "acceptance-board must require a machine-checkable pin for docs/content ACs"
    );
    assert!(
        board.contains("manual-only"),
        "acceptance-board must name the `manual-only` escape for docs/content ACs"
    );
    assert!(
        board.contains("child-launch") && board.contains("board=off"),
        "acceptance-board must name a spawn-dead stop, never silent board=off"
    );
    let issue = std::fs::read_to_string(root.join("commands/shipmates-ship-issue.md")).unwrap();
    assert!(
        issue.contains("child-launch") && issue.contains("never silently set `board=off`"),
        "/shipmates-ship-issue Stage 0 must stop on a dead child-launch"
    );
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

    // The ruleset now lives in the shared bootstrap file; docs/COST.md keeps
    // the canonical public copy, and every rendered command points to it.
    let bootstrap = std::fs::read_to_string(root.join("steering/bootstrap.md")).unwrap();
    assert!(
        bootstrap.contains("## Model routing"),
        "steering/bootstrap.md must contain the model-routing ruleset"
    );

    for command in &commands {
        let path = format!(
            "harnesses/claude-code/.claude/skills/{}/SKILL.md",
            command.name
        );
        let rendered = files
            .get(&path)
            .unwrap_or_else(|| panic!("no rendered payload for {path}"));
        assert!(
            rendered.contains("Shared Shipmates bootstrap principles"),
            "{}: every command must reference the shared bootstrap",
            command.name
        );
        assert!(
            !rendered.contains("<!-- shipmates:bootstrap -->"),
            "{}: the bootstrap marker must not survive into the payload",
            command.name
        );
    }
}

/// Every command that opens a PR must carry the shared Why-merge-this doctrine.
/// The marker lives only on PR-opening commands; non-PR commands must stay clean
/// so the contract cannot silently widen.
#[test]
fn test_pr_opening_commands_reference_bootstrap() {
    const PR_OPENING: &[&str] = &[
        "shipmates-ship-issue",
        "shipmates-fix-bug",
        "shipmates-document",
        "shipmates-onboard",
        "shipmates-harden",
        "shipmates-spike",
        "shipmates-polish",
        "shipmates-ship-epic",
        "shipmates-migrate",
        "shipmates-refactor",
    ];

    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let commands = load_commands(&root.join("commands")).unwrap();
    assert!(!commands.is_empty());
    let files = ClaudeCodeAdapter.build(&[], &commands).unwrap();

    // The why-merge doctrine now lives in the shared bootstrap file.
    let bootstrap = std::fs::read_to_string(root.join("steering/bootstrap.md")).unwrap();
    assert!(
        bootstrap.contains("Why merge this") || bootstrap.contains("why-merge-pr"),
        "steering/bootstrap.md must contain the why-merge doctrine"
    );

    let pr_set: std::collections::HashSet<&str> = PR_OPENING.iter().copied().collect();
    for command in &commands {
        let path = format!(
            "harnesses/claude-code/.claude/skills/{}/SKILL.md",
            command.name
        );
        let rendered = files
            .get(&path)
            .unwrap_or_else(|| panic!("no rendered payload for {path}"));

        if pr_set.contains(command.name.as_str()) {
            assert!(
                rendered.contains("Shared Shipmates bootstrap principles"),
                "{}: PR-opening command must reference the shared bootstrap",
                command.name
            );
            assert!(
                !rendered.contains("<!-- shipmates:bootstrap -->"),
                "{}: the bootstrap marker must not survive into the payload",
                command.name
            );
        }
    }

    for name in PR_OPENING {
        assert!(
            commands.iter().any(|c| c.name == *name),
            "PR-opening set names unknown command `{name}`"
        );
    }
}

/// The per-harness facts are hand-written in three places, and only the record is
/// gated. This checks the copy a captain actually reads: every row of the shipped
/// `## Model routing` table must agree with `tools/harness_matrix.json`
/// `model_surface`, for every target, on every axis the table carries. Without it
/// the shipped table is a fourth opinion that can go stale silently — which is how
/// the ADR's "three targets" drifted from the record's four. The match is on the
/// record's own enums, exactly: the table's job is the *level the orchestrator
/// acts on*, and a gloss restating the record is bytes inlined into every command
/// on every target (#450).
#[test]
fn test_shipped_model_routing_table_matches_the_matrix() {
    /// Map a shipped cell onto the record's enum by **exact** match on the
    /// display spelling. A cell that grew a `·`-joined gloss no longer equals
    /// any spelling, so it fails here rather than passing on a prefix.
    fn record_value(cells: &[(&str, &str)], cell: &str, column: &str, target: &str) -> String {
        cells
            .iter()
            .find(|(shown, _)| *shown == cell)
            .map(|(_, value)| (*value).to_string())
            .unwrap_or_else(|| {
                panic!(
                    "{target}: the shipped table's {column} cell `{cell}` is not one of the record's \
                     enums ({}) — a gloss has crept back in, or the shipped copy has drifted",
                    cells
                        .iter()
                        .map(|(shown, _)| format!("`{shown}`"))
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            })
    }

    /// The effort cell is the record's effort kind plus **at most one** ` · `
    /// clamp clause. The clamp changes the `requested→resolved` audit field, so
    /// the orchestrator acts on it; the record's remaining effort prose is not
    /// worth inlining 128 times, because it changes no decision (#450). The
    /// clause itself is now gated against the record by `check_clamp_clause`
    /// (#485), so it can abbreviate the record but never overstate it.
    fn effort_value(
        cells: &[(&str, &str)],
        cell: &str,
        effort: &serde_json::Value,
        target: &str,
    ) -> String {
        assert!(
            cell.matches('·').count() <= 1,
            "{target}: the shipped effort cell `{cell}` carries more than one ` · ` clause"
        );
        let (kind, clamp) = match cell.split_once(" · ") {
            Some((kind, clamp)) => (kind, Some(clamp)),
            None => (cell, None),
        };
        if let Some(clamp) = clamp {
            assert!(
                !clamp.contains(';'),
                "{target}: the shipped effort cell's clamp clause `{clamp}` carries a `;` \
                 continuation"
            );
            assert!(
                clamp.len() <= 72,
                "{target}: the shipped effort cell's clamp clause `{clamp}` is {} bytes — long \
                 enough to be a second opinion on the record again (#450)",
                clamp.len()
            );
            if let Err(defect) = check_clamp_clause(clamp, effort, target) {
                panic!("{defect}");
            }
        }
        cells
            .iter()
            .find(|(shown, _)| *shown == kind)
            .map(|(_, value)| (*value).to_string())
            .unwrap_or_else(|| {
                panic!(
                    "{target}: the shipped table's effort cell `{cell}` does not lead with one of \
                     the record's enum spellings — the kind before any ` · ` clause must match one \
                     exactly ({})",
                    cells
                        .iter()
                        .map(|(shown, _)| format!("`{shown}`"))
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            })
    }

    const OVERRIDE_CELLS: [(&str, &str); 4] = [
        ("per-spawn", "per-spawn"),
        ("static agent file", "static-agent-file"),
        ("session-level", "session-level"),
        ("none", "none"),
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
    let mut table: Vec<&str> = Vec::new();
    for line in block.lines() {
        let line = line.trim();
        if !line.starts_with('|') {
            continue;
        }
        table.push(line);
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
                record_value(&OVERRIDE_CELLS, cells[2], "override", target),
            ),
            (
                "enforcement",
                cells[3],
                record_value(&ENFORCEMENT_CELLS, cells[3], "enforcement", target),
            ),
            (
                "effort",
                cells[4],
                effort_value(&EFFORT_CELLS, cells[4], &surface["effort"], target),
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

    // This table is inlined into every command on every target (19 commands × 8
    // targets), so its size is a cost decision, not a formatting one. #450
    // trimmed it to the record's enums plus the one clamp clause the orchestrator
    // acts on; this ceiling is what stops a gloss growing back silently.
    let table_bytes = table.join("\n").len();
    assert!(
        table_bytes <= 1200,
        "the shipped per-target table is {table_bytes} bytes, past its #450 ceiling of 1,200 — \
         trim a cell back to the record's own value"
    );

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

/// Parse a Codex crew file by the shape `codex::serialize` emits: `name`,
/// `description`, and optional `model_reasoning_effort` as toml_basic strings,
/// then `developer_instructions` as a toml_literal (`'''\n…'''`). No toml crate.
fn parse_emitted_codex_toml(content: &str) -> Result<HashMap<String, String>, String> {
    const LITERAL: &str = "developer_instructions = '''\n";
    let Some(literal_at) = content.find(LITERAL) else {
        return Err("missing developer_instructions toml_literal".into());
    };
    let header = &content[..literal_at];
    let after = &content[literal_at + LITERAL.len()..];
    let Some(end) = after.find("'''") else {
        return Err("unterminated developer_instructions toml_literal".into());
    };
    if !after[end + 3..].trim().is_empty() {
        return Err("trailing bytes after developer_instructions".into());
    }
    let mut keys = HashMap::new();
    keys.insert("developer_instructions".into(), after[..end].to_string());
    for line in header.lines() {
        if line.is_empty() {
            continue;
        }
        let Some((key, raw)) = line.split_once(" = ") else {
            return Err(format!("expected `key = value`, got {line:?}"));
        };
        keys.insert(key.to_string(), parse_emitted_toml_basic(raw)?);
    }
    Ok(keys)
}

fn parse_emitted_toml_basic(raw: &str) -> Result<String, String> {
    let inner = raw
        .strip_prefix('"')
        .and_then(|s| s.strip_suffix('"'))
        .ok_or_else(|| format!("expected toml_basic double quotes, got {raw:?}"))?;
    let mut out = String::with_capacity(inner.len());
    let mut chars = inner.chars();
    while let Some(character) = chars.next() {
        if character != '\\' {
            out.push(character);
            continue;
        }
        match chars.next() {
            Some('\\') => out.push('\\'),
            Some('"') => out.push('"'),
            Some('n') => out.push('\n'),
            Some('t') => out.push('\t'),
            Some('r') => out.push('\r'),
            other => return Err(format!("unknown toml_basic escape {other:?} in {raw}")),
        }
    }
    Ok(out)
}

/// The closed set of `effort.clamp_kind` verdicts the record may carry (#485).
/// Defined once here: the shipped-clause guard, the completeness guard and the
/// demonstration test all read this same list.
const CLAMP_KINDS: [&str; 3] = ["clamps-down", "not-documented", "no-surface"];

/// Lowercase alphanumeric words — the unit both the hedge test and the
/// entailment test work on, so `per-model` and `per model` agree.
fn clamp_words(text: &str) -> Vec<String> {
    text.to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|word| !word.is_empty())
        .map(str::to_string)
        .collect()
}

/// Whether a record's clamp prose documents *no* clamp — the absence it states
/// with a hedge phrase (`Not documented`, `None documented`, `No down-clamp
/// documented`) or with a bare `None`.
fn clamp_is_hedged(text: &str) -> bool {
    let words = clamp_words(text);
    let has = |needle: &str| words.iter().any(|word| word == needle);
    has("none")
        || has("undocumented")
        || words.join(" ").contains("not documented")
        || has("no") && (has("clamp") || has("documented"))
}

/// Every word the clause names must appear in the record text it abbreviates,
/// so a clause can shorten the record but never invent a claim.
fn clamp_entails(record_text: &str, clause: &str) -> bool {
    let haystack = clamp_words(record_text);
    clamp_words(clause)
        .iter()
        .all(|word| haystack.iter().any(|candidate| candidate == word))
}

/// #485 — the mapping rule binding a shipped clamp clause to the record.
///
/// `effort.clamp` is prose, so a guard needs a decidable rule. The record
/// carries the verdict machine-readably in `effort.clamp_kind`, and this binds
/// the shipped clause to it three ways:
///
/// - `clamps-down`: the clause must **not** hedge (it describes a clamp the
///   record documents), and every word it names must appear in the record's own
///   `clamp` text — it may abbreviate the record, never invent a mechanism.
/// - `not-documented`: the clause **must** hedge to the same degree — a clause
///   asserting a clamp where the record documents none is exactly the drift
///   this guard exists to stop.
/// - `no-surface`: there is nothing to clamp, so the clause may only restate
///   the row's own effort text (vocabulary + clamp + notes) — which is how
///   `only an interactive cycle` stays honest on a row whose clamp is `None`.
///
/// The rule refuses a clause that **overstates** the record. It is not a
/// semantic-equivalence check and deliberately cannot be: it is a guard against
/// a gloss growing back, not a proof of meaning.
fn check_clamp_clause(
    clause: &str,
    effort: &serde_json::Value,
    target: &str,
) -> Result<(), String> {
    let clamp = effort["clamp"].as_str().unwrap_or("");
    if clamp.is_empty() {
        return Err(format!(
            "{target}: the record documents no clamp at all, so the shipped cell must carry no \
             ` · ` clamp clause, found `{clause}` (#485)"
        ));
    }
    let clamp_kind = effort["clamp_kind"].as_str().unwrap_or("");
    if !CLAMP_KINDS.contains(&clamp_kind) {
        return Err(format!(
            "{target}: effort.clamp_kind `{clamp_kind}` is outside the closed set {CLAMP_KINDS:?} \
             — the record must state the verdict the shipped clause is gated on (#485)"
        ));
    }
    let hedged = clamp_is_hedged(clause);
    match clamp_kind {
        "clamps-down" if hedged => Err(format!(
            "{target}: the shipped clamp clause `{clause}` hedges a clamp the record documents \
             (`{clamp}`) — the clause must not assert less than the record either (#485)"
        )),
        "not-documented" if !hedged => Err(format!(
            "{target}: the shipped clamp clause `{clause}` asserts a clamp the record does not \
             document (`{clamp}`) — it must hedge to the same degree (#485)"
        )),
        "clamps-down" => {
            if clamp_entails(clamp, clause) {
                Ok(())
            } else {
                Err(format!(
                    "{target}: the shipped clamp clause `{clause}` names a mechanism the record's \
                     own clamp text does not document (`{clamp}`) — abbreviate the record, never \
                     invent it (#485)"
                ))
            }
        }
        "no-surface" => {
            let row_text = format!(
                "{} {} {}",
                effort["vocabulary"].as_str().unwrap_or(""),
                clamp,
                effort["notes"].as_str().unwrap_or("")
            );
            if clamp_entails(&row_text, clause) {
                Ok(())
            } else {
                Err(format!(
                    "{target}: the shipped clamp clause `{clause}` is not entailed by the row's own \
                     effort text on a `no-surface` row (#485)"
                ))
            }
        }
        // `not-documented` with a hedge is the only passing combination left.
        _ => Ok(()),
    }
}

/// #485 — the shipped-clause guard must be **shown failing**, not merely
/// asserted to pass. This drives `check_clamp_clause` with clauses that
/// overstate the record — including one built from the record's real bytes — so
/// the guard's binding is exercised rather than described.
#[test]
fn test_clamp_guard_rejects_an_overstated_clause() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let matrix: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(root.join("tools/harness_matrix.json")).unwrap(),
    )
    .unwrap();

    // Built from the live record: a row that documents no clamp, handed a
    // clause that asserts one — the exact drift #450 exists to stop.
    let antigravity = &matrix["harnesses"]["antigravity"]["model_surface"]["effort"];
    assert_eq!(antigravity["clamp_kind"].as_str(), Some("not-documented"));
    assert!(
        check_clamp_clause("unsupported level clamps down", antigravity, "antigravity").is_err(),
        "a clause asserting a clamp the record does not document must be refused"
    );

    // The mirror image: hedging a clamp the record does document.
    let claude_code = &matrix["harnesses"]["claude-code"]["model_surface"]["effort"];
    assert!(
        check_clamp_clause("no clamp documented", claude_code, "claude-code").is_err(),
        "a clause hedging a clamp the record documents must be refused"
    );

    // A mechanism the record never names, on a row that does document a clamp.
    assert!(
        check_clamp_clause("hard refusal above its tier", claude_code, "claude-code").is_err(),
        "a clause naming a mechanism the record does not document must be refused"
    );

    // And the real clause still passes on the same row, so the guard is
    // refusing the overstatement rather than every clause.
    assert_eq!(
        check_clamp_clause("unsupported level clamps down", claude_code, "claude-code"),
        Ok(())
    );
}

/// #531 — the declared model pool is removed, and nothing that ships may
/// describe it. The routing block is asserted clause-by-clause in `render.rs`;
/// this sweeps the canonical trees that become a captain's payload, so a stale
/// mention in a command, a crew role or a toolbox tool fails here instead of
/// shipping as advice to maintain a file that no longer exists.
///
/// The vocabulary is named specifically rather than as the bare word `pool`:
/// the word has legitimate uses in this payload — the reviewer/builder pool, a
/// candidate pool, a name pool, pooling in a performance note — and only the
/// declared-config sense was retired.
#[test]
fn test_no_canonical_file_describes_the_removed_model_pool() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    const RESIDUE: [&str; 8] = [
        "model-pool",
        "pool unusable",
        "pool out of scope",
        "declared pool",
        "declared-pool",
        "pool source",
        "pool discovery",
        "which pool",
    ];
    let mut files: Vec<PathBuf> = vec![root.join("docs/COST.md")];
    for tree in ["commands", "crew", "toolbox"] {
        files.extend(walk(&root.join(tree)));
    }
    assert!(
        files.len() > 20,
        "the sweep found only {} canonical files — the trees moved, so this guard proves nothing",
        files.len()
    );
    for path in files {
        // Binary files cannot describe a removed feature in prose, and the
        // canonical trees carry at least one (a gitignored `__pycache__` under
        // `toolbox/`, which `build.rs` skips so it never ships). Skipping them
        // keeps this guard about text, which is what it guards.
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        let flat = text
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ");
        for residue in RESIDUE {
            assert!(
                !flat.contains(residue),
                "{}: the removed declared model pool is still described here (`{residue}`) — the \
                 orchestrator supplies the ranking now, so the doctrine must not send a captain \
                 looking for a file that does nothing (#531)",
                path.strip_prefix(&root).unwrap().display()
            );
        }
    }
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

fn directory_names(dir: &std::path::Path) -> BTreeSet<String> {
    std::fs::read_dir(dir)
        .unwrap_or_else(|error| panic!("missing directory {dir:?}: {error}"))
        .map(|entry| {
            let entry = entry.unwrap();
            assert!(
                entry.file_type().unwrap().is_dir(),
                "unexpected non-directory artifact in {dir:?}: {:?}",
                entry.path()
            );
            entry.file_name().to_string_lossy().into_owned()
        })
        .collect()
}

fn direct_files_by_suffix(dir: &std::path::Path, suffix: &str) -> BTreeMap<String, PathBuf> {
    std::fs::read_dir(dir)
        .unwrap_or_else(|error| panic!("missing directory {dir:?}: {error}"))
        .map(|entry| {
            let entry = entry.unwrap();
            let path = entry.path();
            assert!(
                entry.file_type().unwrap().is_file(),
                "unexpected non-file artifact in {dir:?}: {path:?}"
            );
            let file_name = entry.file_name().to_string_lossy().into_owned();
            let identity = file_name
                .strip_suffix(suffix)
                .unwrap_or_else(|| panic!("unexpected file in {dir:?}: {file_name}"))
                .to_string();
            assert!(!identity.is_empty(), "empty artifact identity in {dir:?}");
            (identity, path)
        })
        .collect()
}

fn assert_installed_markdown_identity(
    path: &std::path::Path,
    identity: &str,
    target: &str,
    require_name: bool,
) -> serde_yaml::Value {
    let content = std::fs::read_to_string(path)
        .unwrap_or_else(|error| panic!("cannot read {target} artifact {path:?}: {error}"));
    let frontmatter = frontmatter_block(&content)
        .unwrap_or_else(|| panic!("{target} artifact {path:?} has no frontmatter"));
    let parsed: serde_yaml::Value = serde_yaml::from_str(frontmatter)
        .unwrap_or_else(|error| panic!("{target} artifact {path:?} has invalid YAML: {error}"));
    match parsed.get("name") {
        Some(name) => assert_eq!(
            name.as_str(),
            Some(identity),
            "{target} artifact {path:?} name must equal its path identity"
        ),
        // OpenCode commands and agents deliberately use exact filename identity;
        // their emitted frontmatter carries description/mode/permission, not name.
        None => assert!(
            !require_name,
            "{target} artifact {path:?} must declare name {identity}"
        ),
    }
    parsed
}

fn assert_installed_codex_identity(path: &std::path::Path, identity: &str, target: &str) {
    let content = std::fs::read_to_string(path)
        .unwrap_or_else(|error| panic!("cannot read {target} artifact {path:?}: {error}"));
    let parsed = parse_emitted_codex_toml(&content).unwrap_or_else(|error| {
        panic!("{target} artifact {path:?} has invalid emitted TOML: {error}")
    });
    for key in ["name", "description", "developer_instructions"] {
        assert!(
            parsed.get(key).is_some_and(|value| !value.is_empty()),
            "{target} artifact {path:?} is missing TOML key {key}"
        );
    }
    assert_eq!(
        parsed.get("name").map(String::as_str),
        Some(identity),
        "{target} artifact {path:?} TOML name must equal its path identity"
    );
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
