//! Argument parsing and the informational commands.

use std::path::PathBuf;

use clux::client::{Client, ClientConfig, ClientTarget};
use clux::server::default_socket_path;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct CliOptions {
    pub(crate) remote: Option<String>,
    pub(crate) socket_path: Option<PathBuf>,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum CliCommand {
    New(Option<String>),
    Attach(Option<String>),
    List,
    Kill(String),
    KillServer,
    Info,
    Debug(Option<String>),
    Help,
    Version,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ParsedCli {
    pub(crate) options: CliOptions,
    pub(crate) command: CliCommand,
}
pub(crate) fn parse_cli_args(args: &[String]) -> Result<ParsedCli, String> {
    let mut options = CliOptions::default();
    let mut positionals = Vec::new();
    let mut i = 0;

    while i < args.len() {
        match args[i].as_str() {
            "--remote" => {
                let value = args
                    .get(i + 1)
                    .ok_or_else(|| "--remote requires a value".to_string())?;
                options.remote = Some(value.clone());
                i += 2;
            }
            "--socket" => {
                let value = args
                    .get(i + 1)
                    .ok_or_else(|| "--socket requires a value".to_string())?;
                options.socket_path = Some(PathBuf::from(value));
                i += 2;
            }
            "-h" | "--help" | "help" => {
                return Ok(ParsedCli {
                    options,
                    command: CliCommand::Help,
                });
            }
            "-v" | "--version" => {
                return Ok(ParsedCli {
                    options,
                    command: CliCommand::Version,
                });
            }
            arg if arg.starts_with('-') => {
                return Err(format!("Unknown option: {}", arg));
            }
            arg => {
                positionals.push(arg.to_string());
                i += 1;
            }
        }
    }

    let command = match positionals.first().map(String::as_str) {
        None => CliCommand::New(None),
        Some("new") => CliCommand::New(positionals.get(1).cloned()),
        Some("attach") | Some("a") => CliCommand::Attach(positionals.get(1).cloned()),
        Some("list") | Some("ls") => CliCommand::List,
        Some("kill") => {
            let name = positionals
                .get(1)
                .cloned()
                .ok_or_else(|| "Usage: clux kill <session-name>".to_string())?;
            CliCommand::Kill(name)
        }
        Some("kill-server") => CliCommand::KillServer,
        Some("info") => CliCommand::Info,
        Some("debug") => CliCommand::Debug(positionals.get(1).cloned()),
        Some(other) => CliCommand::Attach(Some(other.to_string())),
    };

    Ok(ParsedCli { options, command })
}
pub(crate) fn build_client_config(options: &CliOptions) -> ClientConfig {
    let mut config = ClientConfig::default();
    let socket_path = options
        .socket_path
        .clone()
        .unwrap_or_else(default_socket_path);

    config.target = match &options.remote {
        Some(destination) => ClientTarget::RemoteSsh {
            destination: destination.clone(),
            socket_path,
        },
        None => ClientTarget::Local { socket_path },
    };

    config
}
pub(crate) fn print_target_info(config: &ClientConfig) {
    match &config.target {
        ClientTarget::Local { socket_path } => {
            println!("Mode: local");
            println!("Socket: {:?}", socket_path);
        }
        ClientTarget::RemoteSsh {
            destination,
            socket_path,
        } => {
            println!("Mode: remote");
            println!("Remote: {}", destination);
            println!("Socket: {:?}", socket_path);
        }
    }
}
pub(crate) fn print_target_info_for_client(client: &Client) {
    if let Some(destination) = client.remote_destination() {
        println!("Mode: remote");
        println!("Remote: {}", destination);
    } else {
        println!("Mode: local");
    }
    println!("Socket: {:?}", client.socket_path());
}
pub(crate) fn print_help() {
    println!("clux - A terminal multiplexer focused on UX");
    println!();
    println!("USAGE:");
    println!("    clux [GLOBAL OPTIONS] [COMMAND] [ARGS]");
    println!();
    println!("COMMANDS:");
    println!("    (none)              Create a new session (same as 'new')");
    println!("    new [name]          Create a new session");
    println!("    attach [name]       Attach to existing session (or first available)");
    println!("    list, ls            List all sessions");
    println!("    kill <name>         Kill a session");
    println!("    kill-server         Stop the server");
    println!("    info                Show server status");
    println!("    help                Show this help message");
    println!();
    println!("GLOBAL OPTIONS:");
    println!("        --remote <DEST> Connect to a remote host over ssh");
    println!("        --socket <PATH> Override the server socket path");
    println!("    -h, --help          Show this help message");
    println!("    -v, --version       Show version");
    println!();
    println!("EXAMPLES:");
    println!("    clux --remote devbox new");
    println!("    clux attach work --remote devbox");
    println!("    clux --remote devbox --socket /tmp/clux-alt.sock list");
    println!();
    println!("OTHER OPTIONS:");
    println!("    -h, --help          Show this help message");
    println!("    -v, --version       Show version");
    println!();
    println!("KEYBINDINGS (default prefix: Alt+C):");
    println!("    <prefix> d          Detach from session");
    println!("    <prefix> -          Split horizontally");
    println!("    <prefix> p          Split vertically");
    println!("    <prefix> h/j/k/l    Navigate panes");
    println!("    <prefix> n          New window");
    println!("    <prefix> ]/[        Next/previous window");
    println!("    <prefix> q          Quit");
    println!();
    println!("CONFIG:");
    println!("    ~/.config/clux/config.toml");
}
/// Format a unix timestamp as a human-readable time ago string.
pub(crate) fn format_time_ago(timestamp: u64) -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);

    let diff = now.saturating_sub(timestamp);

    if diff < 60 {
        "just now".to_string()
    } else if diff < 3600 {
        format!("{} min ago", diff / 60)
    } else if diff < 86400 {
        format!("{} hr ago", diff / 3600)
    } else {
        format!("{} days ago", diff / 86400)
    }
}
