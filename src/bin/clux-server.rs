//! clux-server - The clux terminal multiplexer server.
//!
//! This binary runs the server process that manages sessions and handles
//! client connections. It is typically started automatically by the client
//! but can also be run manually.

use std::fs::{self, File};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use clux::config::Config;
use clux::server::{default_socket_path, AutoShutdownConfig, Server, ServerConfig};

fn main() -> anyhow::Result<()> {
    // Parse arguments
    let args: Vec<String> = std::env::args().collect();

    let mut socket_path: Option<PathBuf> = None;
    let mut debug = false;
    let mut no_auto_exit = false;

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--socket" | "-s" => {
                if i + 1 < args.len() {
                    socket_path = Some(PathBuf::from(&args[i + 1]));
                    i += 2;
                } else {
                    eprintln!("Error: --socket requires an argument");
                    std::process::exit(1);
                }
            }
            "--debug" | "-d" => {
                debug = true;
                i += 1;
            }
            "--no-auto-exit" => {
                no_auto_exit = true;
                i += 1;
            }
            "--help" | "-h" => {
                print_help();
                return Ok(());
            }
            "--version" | "-v" => {
                println!("clux-server {}", env!("CARGO_PKG_VERSION"));
                return Ok(());
            }
            arg => {
                eprintln!("Unknown argument: {}", arg);
                print_help();
                std::process::exit(1);
            }
        }
    }

    // Load configuration for logging settings
    let (config, _) = Config::load();

    // Determine log level: CLI flag overrides config
    let log_level = if debug {
        "debug"
    } else {
        &config.server.log_level
    };

    // Set up logging - file-based by default, stderr if file logging disabled
    let log_file_path = setup_logging(log_level, &config)?;

    log::info!("=== clux-server starting ===");
    if let Some(ref path) = log_file_path {
        // Also print to stderr so user knows where logs are going
        eprintln!("clux-server: logging to {}", path.display());
    }
    log::debug!("Debug logging enabled");

    // Build config
    let config = ServerConfig {
        socket_path: socket_path.unwrap_or_else(default_socket_path),
        ..Default::default()
    };

    // Build auto-shutdown config
    let auto_shutdown = if no_auto_exit {
        AutoShutdownConfig {
            enabled: false,
            ..Default::default()
        }
    } else {
        AutoShutdownConfig::default()
    };

    log::info!("Starting clux server");
    log::info!("Socket: {:?}", config.socket_path);
    log::info!("Shell: {}", config.shell);

    // Set up signal handling
    let running = Arc::new(AtomicBool::new(true));
    let running_clone = running.clone();

    ctrlc::set_handler(move || {
        log::info!("Received shutdown signal");
        running_clone.store(false, Ordering::SeqCst);
    })?;

    // Create and run the server
    let mut server = Server::with_auto_shutdown(config, auto_shutdown)?;

    log::info!("Server ready, waiting for connections...");

    // Run until signaled to stop or server shuts itself down
    while running.load(Ordering::SeqCst) && server.is_running() {
        if let Err(e) = server.run() {
            log::error!("Server error: {}", e);
            break;
        }
    }

    log::info!("Server stopped");
    Ok(())
}

fn print_help() {
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
fn setup_logging(log_level: &str, config: &Config) -> anyhow::Result<Option<PathBuf>> {
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
