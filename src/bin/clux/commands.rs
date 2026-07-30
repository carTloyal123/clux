//! The subcommands.

use crate::attach::*;
use crate::cli::*;
use crate::*;

use clux::client::{Client, ClientError};

/// Create a new session and attach to it.
pub(crate) fn cmd_new(options: &CliOptions, name: Option<String>) -> anyhow::Result<()> {
    let (inner_cols, inner_rows) = inner_dimensions();

    let mut config = build_client_config(options);
    config.term_cols = inner_cols;
    config.term_rows = inner_rows;

    let mut client = Client::connect(config, true)?;
    client.attach(name, true)?;

    run_attached(&mut client)
}
/// Attach to an existing session (or default).
pub(crate) fn cmd_attach(options: &CliOptions, name: Option<String>) -> anyhow::Result<()> {
    log::info!("cmd_attach called with name: {:?}", name);

    // Get inner dimensions (terminal size minus border)
    let (inner_cols, inner_rows) = inner_dimensions();
    log::info!(
        "Terminal inner size (after border): {}x{}",
        inner_cols,
        inner_rows
    );

    let mut config = build_client_config(options);
    config.term_cols = inner_cols;
    config.term_rows = inner_rows;
    log::debug!(
        "ClientConfig: socket_path={:?}, size={}x{}",
        config.target.socket_path(),
        config.term_cols,
        config.term_rows
    );

    log::info!("Connecting to server...");
    let mut client = Client::connect(config, true)?;
    log::info!("Connected to server successfully");

    // If name is provided, don't create if missing
    let create = name.is_none();
    log::info!("Attaching to session (create={})", create);
    client.attach(name, create)?;
    log::info!("Attached to session successfully");

    run_attached(&mut client)
}
/// Debug mode: attach to session and run one iteration then exit.
/// Useful for testing rendering without interactive use.
pub(crate) fn cmd_debug(options: &CliOptions, name: Option<String>) -> anyhow::Result<()> {
    log::info!("cmd_debug called with name: {:?}", name);

    let (inner_cols, inner_rows) = inner_dimensions();
    log::info!(
        "Terminal inner size (after border): {}x{}",
        inner_cols,
        inner_rows
    );

    let mut config = build_client_config(options);
    config.term_cols = inner_cols;
    config.term_rows = inner_rows;

    log::info!("Connecting to server...");
    let mut client = Client::connect(config, true)?;
    log::info!("Connected to server successfully");

    let create = name.is_none();
    log::info!("Attaching to session (create={})", create);
    client.attach(name, create)?;
    log::info!("Attached to session successfully");

    run_attached_with_options(&mut client, RunOptions { once: true })
}
/// List all sessions.
pub(crate) fn cmd_list(options: &CliOptions) -> anyhow::Result<()> {
    let config = build_client_config(options);
    let mut client = match Client::connect(config, false) {
        Ok(c) => c,
        Err(ClientError::ConnectionFailed(_)) => {
            println!("No server running.");
            return Ok(());
        }
        Err(e) => return Err(e.into()),
    };

    let sessions = client.list_sessions()?;

    if sessions.is_empty() {
        println!("No sessions.");
    } else {
        println!(
            "{:<12} {:>8} {:>12} {:>10}",
            "NAME", "WINDOWS", "CREATED", "ATTACHED"
        );
        for session in sessions {
            let created = format_time_ago(session.created_at);
            let attached = if session.attached_clients > 0 {
                format!(
                    "{} client{}",
                    session.attached_clients,
                    if session.attached_clients == 1 {
                        ""
                    } else {
                        "s"
                    }
                )
            } else {
                "detached".to_string()
            };
            println!(
                "{:<12} {:>8} {:>12} {:>10}",
                session.name, session.windows, created, attached
            );
        }
    }

    Ok(())
}
/// Kill a session.
pub(crate) fn cmd_kill(options: &CliOptions, name: &str) -> anyhow::Result<()> {
    let config = build_client_config(options);
    let mut client = Client::connect(config, false)?;

    client.kill_session(name)?;
    println!("Killed session '{}'", name);

    Ok(())
}
/// Kill the server.
pub(crate) fn cmd_kill_server(options: &CliOptions) -> anyhow::Result<()> {
    let config = build_client_config(options);
    let mut client = match Client::connect(config, false) {
        Ok(c) => c,
        Err(ClientError::ConnectionFailed(_)) => {
            println!("No server running.");
            return Ok(());
        }
        Err(e) => return Err(e.into()),
    };

    client.shutdown_server()?;
    println!("Server stopped");
    Ok(())
}
/// Show server info.
pub(crate) fn cmd_info(options: &CliOptions) -> anyhow::Result<()> {
    let config = build_client_config(options);
    let client = match Client::connect(config.clone(), false) {
        Ok(c) => c,
        Err(ClientError::ConnectionFailed(_)) => {
            println!("Server: not running");
            print_target_info(&config);
            return Ok(());
        }
        Err(e) => return Err(e.into()),
    };

    println!("Server: running");
    print_target_info_for_client(&client);

    Ok(())
}
