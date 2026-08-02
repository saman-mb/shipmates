use shipmates::adapters::Adapter;
use shipmates::adapters::claude_code::ClaudeCodeAdapter;
use shipmates::adapters::opencode::OpencodeAdapter;
use shipmates::catalog::{reject_positional, CanonicalCommand, CanonicalRole};
use shipmates::digest;
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
        source: PathBuf::from("test.md"),
        body: "body content".into(),
    };
    let files = ClaudeCodeAdapter.build(&[role], &[]).unwrap();
    let content = files.get("harnesses/claude-code/.claude/agents/test-role.md").unwrap();
    let hashed = digest::hash(content);
    assert_eq!(hashed, "ca2c6fd05a432e2011d4838d4cb007db3d88e2b220c85c7542183eb5de4fa0e8");
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
    assert!(stdout.contains("claude-code"));
    assert!(stdout.contains("opencode"));
}

#[test]
fn test_cli_build_and_install() {
    let temp_dir = tempfile::tempdir().unwrap();
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_shipmates"))
        .args(&["install", "--harness", "claude-code", "--dir", temp_dir.path().to_str().unwrap()])
        .output()
        .expect("failed to execute shipmates install");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Installed harness: claude-code"));
}

use shipmates::adapters::gemini::GeminiAdapter;

#[test]
fn test_gemini_adapter_integration() {
    let role = CanonicalRole {
        name: "architect".into(),
        description: "Architect role".into(),
        capabilities: vec!["read".into()],
        writes: false,
        web_scopes: vec![],
        read_scopes: vec![],
        tool_order: vec![],
        source: PathBuf::from("architect.md"),
        body: "system prompt body".into(),
    };
    let files = GeminiAdapter.build(&[role], &[]).unwrap();
    let content = files.get("harnesses/gemini/.gemini/agents/architect.md").unwrap();
    assert!(content.contains("name: architect"));
    assert!(content.contains("system prompt body"));
}
