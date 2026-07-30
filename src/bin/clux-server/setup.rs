//! Help text and logging setup.

use std::fs::{self, File};
use std::path::PathBuf;

use clux::config::Config;

pub(crate) fn print_help() {
    println!("clux-server - The clux terminal multiplexer server");
    println!();
    println!("USAGE:");
    println!("    clux-server [OPTIONS]");
    println!();
    println!("OPTIONS:");
    println!("    -s, --socket <PATH>    Socket path (default: /tmp/clux-$UID/clux.sock)");
    println!("    -d, --debug            Enable debug logging");
    println!("        --no-auto-exit     Disable auto-shutdown (daemon mode)");
    println!("    -h, --help             Show this help message");
    println!("    -v, --version          Show version");
    println!();
    println!("LOGGING:");
    println!("    Logs are written to ~/.local/state/clux/clux-server.log by default.");
    println!("    Configure via ~/.config/clux/config.toml:");
    println!();
    println!("    [server]");
    println!("    log_level = \"info\"    # error, warn, info, debug, trace");
    println!("    log_dir = \"~/my/logs\" # or \"\" to disable file logging");
    println!();
    println!("AUTO-SHUTDOWN:");
    println!("    By default, the server automatically shuts down when:");
    println!("    - All sessions are closed (after 1 second grace period)");
    println!("    - No session is created within 30 seconds of startup");
    println!();
    println!("    Use --no-auto-exit for traditional daemon behavior where the");
    println!("    server runs indefinitely until manually stopped.");
    println!();
    println!("The server is typically started automatically by the client.");
    println!("Use 'clux kill-server' to stop a running server.");
}
/// Set up logging to file or stderr.
/// Returns the log file path if file logging is enabled.
pub(crate) fn setup_logging(log_level: &str, config: &Config) -> anyhow::Result<Option<PathBuf>> {
    use std::io::Write;

    let log_dir = config.server.effective_log_dir();

    if let Some(ref dir) = log_dir {
        // Create log directory if it doesn't exist
        fs::create_dir_all(dir)?;

        let log_path = dir.join("clux-server.log");

        // Open log file in append mode
        let log_file = File::options().create(true).append(true).open(&log_path)?;

        // Build logger that writes to file
        env_logger::Builder::new()
            .filter_level(log_level.parse().unwrap_or(log::LevelFilter::Info))
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

        Ok(Some(log_path))
    } else {
        // Log to stderr (no file)
        env_logger::Builder::from_env(env_logger::Env::default().default_filter_or(log_level))
            .format_timestamp_millis()
            .init();

        Ok(None)
    }
}
