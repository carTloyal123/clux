//! CLI argument-parsing tests.

use super::*;
use std::path::PathBuf;

fn strings(args: &[&str]) -> Vec<String> {
    args.iter().map(|s| s.to_string()).collect()
}

#[test]
fn test_parse_cli_args_remote_new() {
    let parsed = parse_cli_args(&strings(&["--remote", "host", "new"])).unwrap();
    assert_eq!(parsed.options.remote.as_deref(), Some("host"));
    assert_eq!(parsed.command, CliCommand::New(None));
}

#[test]
fn test_parse_cli_args_attach_with_remote_after_command() {
    let parsed = parse_cli_args(&strings(&["attach", "work", "--remote", "host"])).unwrap();
    assert_eq!(parsed.options.remote.as_deref(), Some("host"));
    assert_eq!(parsed.command, CliCommand::Attach(Some("work".to_string())));
}

#[test]
fn test_parse_cli_args_socket_override() {
    let parsed = parse_cli_args(&strings(&["--socket", "/tmp/x.sock", "list"])).unwrap();
    assert_eq!(
        parsed.options.socket_path,
        Some(PathBuf::from("/tmp/x.sock"))
    );
    assert_eq!(parsed.command, CliCommand::List);
}

#[test]
fn test_parse_cli_args_remote_socket_info() {
    let parsed = parse_cli_args(&strings(&[
        "--remote",
        "host",
        "--socket",
        "/tmp/r.sock",
        "info",
    ]))
    .unwrap();
    assert_eq!(parsed.options.remote.as_deref(), Some("host"));
    assert_eq!(
        parsed.options.socket_path,
        Some(PathBuf::from("/tmp/r.sock"))
    );
    assert_eq!(parsed.command, CliCommand::Info);
}

#[test]
fn test_is_interrupted_io_matches_eintr() {
    let err = io::Error::from(io::ErrorKind::Interrupted);
    assert!(is_interrupted_io(&err));
}

#[test]
fn test_is_interrupted_io_rejects_other_errors() {
    let err = io::Error::from(io::ErrorKind::BrokenPipe);
    assert!(!is_interrupted_io(&err));
}
