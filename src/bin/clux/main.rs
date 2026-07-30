//! clux - The clux terminal multiplexer client.
//!
//! This is the main entry point for users. It connects to the server,
//! attaches to a session, and handles input/output.

use std::fs::{self, File};
use std::io::{self, Write};

use crossterm::style::Color;
use crossterm::terminal::{self, disable_raw_mode};
use mio::Token;

use clux::config::Config;

mod attach;
mod cli;
mod commands;
mod input;
mod render;

use cli::*;
use commands::*;

pub(crate) const SERVER_TOKEN: Token = Token(0);

/// Lines the wheel moves per notch.
pub(crate) const WHEEL_LINES: i32 = 3;

/// Lines `<prefix> PageUp`/`PageDown` moves.
pub(crate) const PAGE_LINES: i32 = 20;

fn main() -> anyhow::Result<()> {
    // Load configuration for logging settings
    let (config, _) = Config::load();

    // Initialize logging to file
    setup_logging(&config)?;

    log::info!("=== clux client starting ===");

    let args: Vec<String> = std::env::args().collect();
    log::debug!("Arguments: {:?}", args);

    let parsed = parse_cli_args(&args[1..]).map_err(anyhow::Error::msg)?;
    match parsed.command {
        CliCommand::New(name) => cmd_new(&parsed.options, name),
        CliCommand::Attach(name) => cmd_attach(&parsed.options, name),
        CliCommand::List => cmd_list(&parsed.options),
        CliCommand::Kill(name) => cmd_kill(&parsed.options, &name),
        CliCommand::KillServer => cmd_kill_server(&parsed.options),
        CliCommand::Info => cmd_info(&parsed.options),
        CliCommand::Debug(name) => cmd_debug(&parsed.options, name),
        CliCommand::Help => {
            print_help();
            Ok(())
        }
        CliCommand::Version => {
            println!("clux {}", env!("CARGO_PKG_VERSION"));
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
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
}

// ╭──────────────────────────────────────────────────────────────╮
// │                       Border Rendering                       │
// ╰──────────────────────────────────────────────────────────────╯

/// The purple color used for the clux border.
pub(crate) const BORDER_COLOR: Color = Color::Rgb {
    r: 147,
    g: 112,
    b: 219,
};

pub(crate) fn is_interrupted_io(err: &io::Error) -> bool {
    err.kind() == io::ErrorKind::Interrupted
}

pub(crate) fn restore_terminal(stdout: &mut io::Stdout) -> io::Result<()> {
    let execute_result = crossterm::execute!(
        stdout,
        crossterm::event::DisableMouseCapture,
        crossterm::cursor::Show,
        terminal::LeaveAlternateScreen,
    );
    let raw_result = disable_raw_mode();

    match (execute_result, raw_result) {
        (Ok(()), Ok(())) => Ok(()),
        (Err(err), _) => Err(err),
        (_, Err(err)) => Err(err),
    }
}

/// Get inner dimensions (terminal size minus border).
pub(crate) fn inner_dimensions() -> (u16, u16) {
    let (cols, rows) = crossterm::terminal::size().unwrap_or((80, 24));
    (cols.saturating_sub(2), rows.saturating_sub(2))
}

/// Set up logging to file.
pub(crate) fn setup_logging(config: &Config) -> anyhow::Result<()> {
    use std::io::Write;

    let log_dir = config.server.effective_log_dir();

    if let Some(ref dir) = log_dir {
        // Create log directory if it doesn't exist
        fs::create_dir_all(dir)?;

        let log_path = dir.join("clux-client.log");

        // Open log file in append mode
        let log_file = File::options().create(true).append(true).open(&log_path)?;

        // Build logger that writes to file
        env_logger::Builder::new()
            .filter_level(
                config
                    .server
                    .log_level
                    .parse()
                    .unwrap_or(log::LevelFilter::Info),
            )
            .format(move |buf, record| {
                writeln!(
                    buf,
                    "{} [{}] {}:{} - {}",
                    chrono::Local::now().format("%Y-%m-%d %H:%M:%S%.3f"),
                    record.level(),
                    record.file().unwrap_or("unknown"),
                    record.line().unwrap_or(0),
                    record.args()
                )
            })
            .target(env_logger::Target::Pipe(Box::new(log_file)))
            .init();
    } else {
        // Log to stderr (disabled file logging)
        env_logger::Builder::from_env(
            env_logger::Env::default().default_filter_or(&config.server.log_level),
        )
        .format_timestamp_millis()
        .init();
    }

    Ok(())
}
