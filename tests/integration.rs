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
    let files = ClaudeCodeAdapter::build(&[role], &[]);
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
    let files = OpencodeAdapter::build(&[], &[command]);
    let content = files.get("harnesses/opencode/.opencode/commands/test-cmd.md").unwrap();
    let hashed = digest::hash(content);
    assert_eq!(hashed, "b3c903c9c4a4065db673be60723d152a64cd8a181f35163670645e96ddd183e4");
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
    let files = OpencodeAdapter::build(&[role], &[]);
    let content = files.get("harnesses/opencode/.opencode/agents/test-role.md").unwrap();
    assert!(content.starts_with("\"*\": deny\n"));
    assert!(content.contains("\"read\": allow\n"));
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
