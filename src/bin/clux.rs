//! clux - The clux terminal multiplexer client.
//!
//! This is the main entry point for users. It connects to the server,
//! attaches to a session, and handles input/output.

use std::fs::{self, File};
use std::io::{self, Write};
use std::path::PathBuf;
use std::time::{Duration, Instant};

use crossterm::event::{self, Event, KeyCode, KeyModifiers, MouseEvent, MouseEventKind};
use crossterm::terminal::{self, disable_raw_mode, enable_raw_mode};
use crossterm::{
    cursor::MoveTo,
    queue,
    style::{Color, Print, ResetColor, SetForegroundColor},
};
use mio::unix::SourceFd;
use mio::{Events, Interest, Poll, Token};

use clux::client::{cells_to_ansi, Client, ClientConfig, ClientError, ClientTarget, ScreenBuffer};
use clux::config::Config;
use clux::event::encode_mouse_sgr;
use clux::protocol::{CommandAction, DetachReason, Direction, ServerMessage, WindowLayout};
use clux::server::default_socket_path;

const SERVER_TOKEN: Token = Token(0);

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct CliOptions {
    remote: Option<String>,
    socket_path: Option<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum CliCommand {
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
struct ParsedCli {
    options: CliOptions,
    command: CliCommand,
}

fn parse_cli_args(args: &[String]) -> Result<ParsedCli, String> {
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

fn build_client_config(options: &CliOptions) -> ClientConfig {
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

fn print_target_info(config: &ClientConfig) {
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

fn print_target_info_for_client(client: &Client) {
    if let Some(destination) = client.remote_destination() {
        println!("Mode: remote");
        println!("Remote: {}", destination);
    } else {
        println!("Mode: local");
    }
    println!("Socket: {:?}", client.socket_path());
}

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

/// Create a new session and attach to it.
fn cmd_new(options: &CliOptions, name: Option<String>) -> anyhow::Result<()> {
    let (inner_cols, inner_rows) = inner_dimensions();

    let mut config = build_client_config(options);
    config.term_cols = inner_cols;
    config.term_rows = inner_rows;

    let mut client = Client::connect(config, true)?;
    client.attach(name, true)?;

    run_attached(&mut client)
}

/// Attach to an existing session (or default).
fn cmd_attach(options: &CliOptions, name: Option<String>) -> anyhow::Result<()> {
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
fn cmd_debug(options: &CliOptions, name: Option<String>) -> anyhow::Result<()> {
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
fn cmd_list(options: &CliOptions) -> anyhow::Result<()> {
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
fn cmd_kill(options: &CliOptions, name: &str) -> anyhow::Result<()> {
    let config = build_client_config(options);
    let mut client = Client::connect(config, false)?;

    client.kill_session(name)?;
    println!("Killed session '{}'", name);

    Ok(())
}

/// Kill the server.
fn cmd_kill_server(options: &CliOptions) -> anyhow::Result<()> {
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
fn cmd_info(options: &CliOptions) -> anyhow::Result<()> {
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

/// Options for run_attached
#[derive(Default)]
struct RunOptions {
    /// Run only one iteration of the event loop (for debugging)
    once: bool,
}

/// Run the main event loop while attached to a session.
fn run_attached(client: &mut Client) -> anyhow::Result<()> {
    run_attached_with_options(client, RunOptions::default())
}

/// Run the main event loop while attached to a session with options.
fn run_attached_with_options(client: &mut Client, options: RunOptions) -> anyhow::Result<()> {
    log::info!("=== run_attached starting ===");

    // Load configuration for keybindings
    let (config, _) = Config::load();
    let prefix_key = config.prefix.key.clone();
    log::debug!("Prefix key configured as: {}", prefix_key);
    let prefix_parsed = clux::config::ParsedKey::parse(&prefix_key)
        .map_err(|e| anyhow::anyhow!("Invalid prefix key: {}", e))?;

    // Get terminal size for border
    let (term_cols, term_rows) = crossterm::terminal::size()?;
    log::info!("Terminal size: {}x{}", term_cols, term_rows);

    // Get session name for border display
    let session_name = client.session_name().unwrap_or("").to_string();
    log::info!("Session name: {}", session_name);

    // Set up terminal
    log::info!("Setting up terminal (raw mode, alternate screen)...");
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    crossterm::execute!(
        stdout,
        terminal::EnterAlternateScreen,
        crossterm::cursor::Hide,
        crossterm::event::EnableMouseCapture,
    )?;

    // Draw initial border
    render_border(&mut stdout, term_cols, term_rows, &session_name, "")?;
    stdout.flush()?;
    log::info!("Terminal setup complete, border drawn");

    // Set up polling
    log::debug!("Setting up mio poll...");
    let mut poll = Poll::new()?;
    let mut events = Events::with_capacity(128);

    // Register server connection
    let fd = client.as_raw_fd();
    log::debug!("Registering server connection fd={} with mio", fd);
    poll.registry()
        .register(&mut SourceFd(&fd), SERVER_TOKEN, Interest::READABLE)?;

    // State
    let mut running = true;
    let mut command_mode = false;
    let mut mouse_mode_enabled = false; // Track if focused pane wants mouse events
    let mut detach_reason: Option<DetachReason> = None; // Track why we detached
    let timeout = Duration::from_millis(50);
    let mut loop_count = 0u64;

    // V2 rendering state
    // Inner dimensions (excluding border)
    let inner_cols = term_cols.saturating_sub(2) as usize;
    let inner_rows = term_rows.saturating_sub(2) as usize;
    let mut screen_buffer = ScreenBuffer::new(inner_cols, inner_rows);
    let mut use_v2_rendering = false;

    log::info!("Entering main event loop...");

    // Main event loop
    while running {
        loop_count += 1;
        if loop_count % 100 == 1 {
            log::trace!("Event loop iteration {}", loop_count);
        }

        // Poll for events
        poll.poll(&mut events, Some(timeout))?;

        let event_count = events.iter().count();
        if event_count > 0 {
            log::debug!("Got {} events from poll", event_count);
        }

        // Handle server messages
        for event in events.iter() {
            log::debug!(
                "Processing event: token={:?}, readable={}",
                event.token(),
                event.is_readable()
            );
            if event.token() == SERVER_TOKEN {
                log::debug!("Server socket is readable, trying to receive messages...");
                let frame_start = Instant::now();
                let mut did_render = false;
                loop {
                    match client.try_recv() {
                        Ok(Some(msg)) => {
                            log::info!("Received server message: {:?}", msg_summary(&msg));
                            match handle_server_message(
                                msg,
                                &mut stdout,
                                &mut screen_buffer,
                                use_v2_rendering,
                            )? {
                                MessageResult::Continue => {
                                    log::debug!("Message handled, continuing");
                                }
                                MessageResult::Detached(reason) => {
                                    log::info!("Detached: {:?}", reason);
                                    detach_reason = Some(reason);
                                    running = false;
                                    break;
                                }
                                MessageResult::Shutdown => {
                                    log::info!("Server shutdown");
                                    running = false;
                                    break;
                                }
                                MessageResult::MouseModeChanged(enabled) => {
                                    log::info!("Client mouse mode updated: {}", enabled);
                                    mouse_mode_enabled = enabled;
                                }
                                MessageResult::LayoutChanged(layout) => {
                                    log::info!(
                                        "Switching to v2 rendering, {} panes",
                                        layout.panes.len()
                                    );
                                    use_v2_rendering = true;
                                    screen_buffer.set_layout(layout);
                                    // Render immediately to show dividers
                                    render_screen_buffer(&mut stdout, &screen_buffer)?;
                                    did_render = true;
                                }
                                MessageResult::PaneUpdated => {
                                    // Screen buffer already updated, flush and position cursor
                                    stdout.flush()?;
                                    // Position cursor after all rendering is done
                                    let cursor = screen_buffer.cursor();
                                    if cursor.visible {
                                        crossterm::execute!(
                                            stdout,
                                            crossterm::cursor::MoveTo(cursor.col, cursor.row),
                                            crossterm::cursor::Show,
                                        )?;
                                    }
                                    did_render = true;
                                }
                            }
                        }
                        Ok(None) => {
                            log::trace!("No more messages available");
                            break;
                        }
                        Err(e) => {
                            log::error!("Error receiving from server: {}", e);
                            running = false;
                            break;
                        }
                    }
                }
                // Update frame timing if we rendered something
                if did_render {
                    let frame_time_us = frame_start.elapsed().as_micros() as u64;
                    // Update the frame time display in the border
                    let frame_info = format!("{:.2}ms", frame_time_us as f64 / 1000.0);
                    update_frame_time(&mut stdout, term_cols, &frame_info)?;
                }
            }
        }

        // Handle keyboard/mouse input
        while event::poll(Duration::ZERO)? {
            match event::read()? {
                Event::Key(key) => {
                    log::debug!("Key event: {:?} modifiers={:?}", key.code, key.modifiers);

                    // Check for prefix key
                    if !command_mode && prefix_parsed.matches(key.code, key.modifiers) {
                        log::info!("Prefix key pressed, entering command mode");
                        command_mode = true;
                        continue;
                    }

                    if command_mode {
                        log::debug!("In command mode, processing key...");
                        command_mode = false;

                        // Handle command-mode key
                        if let Some(action) = key_to_command_action(&key, &config) {
                            log::info!("Command action: {:?}", action);
                            match action {
                                InternalAction::Detach => {
                                    log::info!("Detaching...");
                                    client.detach()?;
                                    running = false;
                                }
                                InternalAction::Quit => {
                                    log::info!("Quitting...");
                                    client.send_command(CommandAction::Quit)?;
                                    running = false;
                                }
                                InternalAction::SendPrefix => {
                                    // Send the prefix key itself to the PTY
                                    if let Some(bytes) = key_to_bytes(&key) {
                                        client.send_input(bytes)?;
                                    }
                                }
                                InternalAction::Command(cmd) => {
                                    client.send_command(cmd)?;
                                }
                            }
                        }
                    } else {
                        // Send key to PTY
                        if let Some(bytes) = key_to_bytes(&key) {
                            client.send_input(bytes)?;
                        }
                    }
                }
                Event::Mouse(mouse) => {
                    // Only forward mouse events if the focused pane has enabled mouse mode
                    if !mouse_mode_enabled {
                        continue;
                    }

                    // Only forward button press/release and scroll events
                    // Motion events (Moved, Drag) require mode 1002/1003
                    let dominated_event = matches!(
                        mouse.kind,
                        MouseEventKind::Down(_)
                            | MouseEventKind::Up(_)
                            | MouseEventKind::ScrollUp
                            | MouseEventKind::ScrollDown
                            | MouseEventKind::ScrollLeft
                            | MouseEventKind::ScrollRight
                    );

                    if !dominated_event {
                        // Skip motion events for now
                        continue;
                    }

                    // Determine if this is a press or release event
                    // Up events use 'm' suffix, all others use 'M' suffix
                    let is_press = !matches!(mouse.kind, MouseEventKind::Up(_));

                    // Adjust coordinates for border (subtract 1 for inner area)
                    // The border is 1 cell wide on each side
                    let adjusted = MouseEvent {
                        kind: mouse.kind,
                        column: mouse.column.saturating_sub(1),
                        row: mouse.row.saturating_sub(1),
                        modifiers: mouse.modifiers,
                    };

                    // Encode as SGR mouse protocol and send to server
                    let bytes = encode_mouse_sgr(&adjusted, is_press);
                    client.send_input(bytes)?;
                }
                Event::Resize(cols, rows) => {
                    log::info!("Terminal resized to {}x{}", cols, rows);

                    // Send inner dimensions to server (minus border)
                    let inner_cols = cols.saturating_sub(2);
                    let inner_rows = rows.saturating_sub(2);
                    client.send_resize(inner_cols, inner_rows)?;

                    // Resize the screen buffer and reset v2 rendering state
                    // The server will send a new LayoutChanged + PaneUpdate
                    screen_buffer.resize(inner_cols as usize, inner_rows as usize);
                    use_v2_rendering = false;

                    // Redraw border
                    render_border(&mut stdout, cols, rows, &session_name, "")?;
                }
                Event::Paste(text) => {
                    // Send bracketed paste
                    let mut bytes = Vec::new();
                    bytes.extend_from_slice(b"\x1b[200~");
                    bytes.extend_from_slice(text.as_bytes());
                    bytes.extend_from_slice(b"\x1b[201~");
                    client.send_input(bytes)?;
                }
                _ => {}
            }
        }

        stdout.flush()?;

        // If --once mode, exit after first iteration that received messages
        if options.once && loop_count > 0 {
            log::info!("--once mode: exiting after first iteration");
            // Wait a moment to let any rendering complete
            std::thread::sleep(Duration::from_millis(100));
            break;
        }
    }

    // Cleanup
    crossterm::execute!(
        stdout,
        crossterm::event::DisableMouseCapture,
        crossterm::cursor::Show,
        terminal::LeaveAlternateScreen,
    )?;
    disable_raw_mode()?;

    // Print detach message if we were detached
    if let Some(reason) = detach_reason {
        match reason {
            DetachReason::ClientRequested => {
                if !session_name.is_empty() {
                    println!("[detached (from session {})]", session_name);
                } else {
                    println!("[detached]");
                }
            }
            DetachReason::SessionClosed => {
                println!("[exited]");
            }
            DetachReason::ServerShutdown => {
                println!("[server shutting down]");
            }
            DetachReason::Replaced => {
                println!("[detached (replaced by another client)]");
            }
        }
    }

    Ok(())
}

/// Result of handling a server message.
enum MessageResult {
    Continue,
    Detached(DetachReason),
    Shutdown,
    MouseModeChanged(bool),
    LayoutChanged(WindowLayout),
    PaneUpdated,
}

/// Get a summary of a server message for logging.
fn msg_summary(msg: &ServerMessage) -> String {
    match msg {
        ServerMessage::HelloAck {
            version,
            server_pid,
        } => {
            format!("HelloAck(version={}, pid={})", version, server_pid)
        }
        ServerMessage::Attached {
            session_id,
            session_name,
            needs_full_redraw,
        } => {
            format!(
                "Attached(session={}, name={}, redraw={})",
                session_id, session_name, needs_full_redraw
            )
        }
        ServerMessage::Detached { reason } => {
            format!("Detached(reason={:?})", reason)
        }
        ServerMessage::FullScreen { rows, cursor, .. } => {
            format!(
                "FullScreen(rows={}, cursor=({},{}))",
                rows.len(),
                cursor.row,
                cursor.col
            )
        }
        ServerMessage::Update {
            changed_rows,
            cursor,
            ..
        } => {
            format!(
                "Update(changed={}, cursor=({},{}))",
                changed_rows.len(),
                cursor.row,
                cursor.col
            )
        }
        ServerMessage::SessionList(sessions) => {
            format!("SessionList(count={})", sessions.len())
        }
        ServerMessage::Error { message } => {
            format!("Error({})", message)
        }
        ServerMessage::Pong => "Pong".to_string(),
        ServerMessage::Shutdown => "Shutdown".to_string(),
        ServerMessage::MouseMode { enabled } => format!("MouseMode(enabled={})", enabled),
        ServerMessage::LayoutChanged { layout } => {
            format!("LayoutChanged(panes={})", layout.panes.len())
        }
        ServerMessage::PaneUpdate {
            pane_id,
            changed_rows,
            cursor,
        } => {
            format!(
                "PaneUpdate(pane={}, rows={}, cursor={:?})",
                pane_id,
                changed_rows.len(),
                cursor.as_ref().map(|c| (c.row, c.col))
            )
        }
    }
}

/// Handle a message from the server.
/// Content is rendered with offset (1, 1) to account for the border.
/// When using v2 rendering, updates are applied to the screen_buffer.
fn handle_server_message(
    msg: ServerMessage,
    stdout: &mut io::Stdout,
    screen_buffer: &mut ScreenBuffer,
    use_v2_rendering: bool,
) -> anyhow::Result<MessageResult> {
    log::debug!("handle_server_message: {}", msg_summary(&msg));

    // Content is rendered inside the border, offset by 1 in each direction
    const X_OFFSET: u16 = 1;
    const Y_OFFSET: u16 = 1;

    match msg {
        ServerMessage::FullScreen {
            rows,
            cursor,
            status_line: _,
        } => {
            // V1 protocol - only use if not in v2 mode
            if use_v2_rendering {
                log::debug!("Ignoring FullScreen in v2 rendering mode");
                return Ok(MessageResult::Continue);
            }

            log::info!(
                "Rendering full screen: {} rows, cursor at ({},{})",
                rows.len(),
                cursor.row,
                cursor.col
            );

            // Get terminal size to know inner width
            let (term_cols, _) = crossterm::terminal::size().unwrap_or((80, 24));
            let inner_cols = term_cols.saturating_sub(2) as usize;

            // Render all rows with offset for border
            for (i, row) in rows.iter().enumerate() {
                // Move to position inside border
                queue!(stdout, MoveTo(X_OFFSET, Y_OFFSET + i as u16))?;

                // Write content, truncated to inner width
                let content: String = row.content.chars().take(inner_cols).collect();
                write!(stdout, "{}", content)?;

                // Clear to end of inner area (but not the border)
                let content_width = content.chars().count();
                if content_width < inner_cols {
                    let padding = " ".repeat(inner_cols - content_width);
                    write!(stdout, "{}", padding)?;
                }
            }

            // Position cursor with offset
            if cursor.visible {
                let cursor_col = X_OFFSET + cursor.col;
                let cursor_row = Y_OFFSET + cursor.row;
                log::debug!(
                    "Positioning cursor at ({}, {}) (screen: {}, {})",
                    cursor.col,
                    cursor.row,
                    cursor_col,
                    cursor_row
                );
                crossterm::execute!(
                    stdout,
                    crossterm::cursor::MoveTo(cursor_col, cursor_row),
                    crossterm::cursor::Show,
                )?;
            } else {
                crossterm::execute!(stdout, crossterm::cursor::Hide)?;
            }

            log::debug!("Full screen render complete");
            Ok(MessageResult::Continue)
        }
        ServerMessage::Update {
            changed_rows,
            cursor,
            status_line: _,
        } => {
            // V1 protocol - only use if not in v2 mode
            if use_v2_rendering {
                log::debug!("Ignoring Update in v2 rendering mode");
                return Ok(MessageResult::Continue);
            }

            // Get terminal size to know inner width
            let (term_cols, _) = crossterm::terminal::size().unwrap_or((80, 24));
            let inner_cols = term_cols.saturating_sub(2) as usize;

            // Update only changed rows with offset for border
            for (row_idx, row) in changed_rows {
                queue!(stdout, MoveTo(X_OFFSET, Y_OFFSET + row_idx))?;

                // Write content, truncated to inner width
                let content: String = row.content.chars().take(inner_cols).collect();
                write!(stdout, "{}", content)?;

                // Clear to end of inner area
                let content_width = content.chars().count();
                if content_width < inner_cols {
                    let padding = " ".repeat(inner_cols - content_width);
                    write!(stdout, "{}", padding)?;
                }
            }

            // Position cursor with offset
            if cursor.visible {
                let cursor_col = X_OFFSET + cursor.col;
                let cursor_row = Y_OFFSET + cursor.row;
                crossterm::execute!(
                    stdout,
                    crossterm::cursor::MoveTo(cursor_col, cursor_row),
                    crossterm::cursor::Show,
                )?;
            }

            Ok(MessageResult::Continue)
        }
        ServerMessage::LayoutChanged { layout } => {
            log::info!(
                "Layout changed: {} panes, screen {}x{}",
                layout.panes.len(),
                layout.screen_cols,
                layout.screen_rows
            );
            Ok(MessageResult::LayoutChanged(layout))
        }
        ServerMessage::PaneUpdate {
            pane_id,
            changed_rows,
            cursor,
        } => {
            log::debug!(
                "PaneUpdate: pane={}, rows={}, cursor={:?}",
                pane_id,
                changed_rows.len(),
                cursor.as_ref().map(|c| (c.row, c.col))
            );

            // Apply update to screen buffer
            screen_buffer.apply_pane_update(pane_id, &changed_rows);

            // Render the changed rows from the screen buffer
            for pane_row in &changed_rows {
                // Find pane position in layout to compute screen row
                if let Some(layout) = screen_buffer.layout() {
                    if let Some(pane) = layout.panes.iter().find(|p| p.pane_id == pane_id) {
                        let screen_row = pane.y + pane_row.row_idx;

                        // Get the full row from screen buffer and render it
                        if let Some(row_cells) = screen_buffer.get_row(screen_row as usize) {
                            queue!(stdout, MoveTo(X_OFFSET, Y_OFFSET + screen_row))?;
                            let ansi = cells_to_ansi(row_cells);
                            write!(stdout, "{}", ansi)?;
                        }
                    }
                }
            }

            // Store cursor position if provided (for focused pane)
            // We don't position it immediately because subsequent pane updates might
            // move the terminal cursor while rendering their rows. The cursor will be
            // positioned after all messages are processed.
            if let Some(c) = cursor {
                // Cursor is in pane-local coordinates, need to translate to screen
                if let Some(layout) = screen_buffer.layout() {
                    if let Some(pane) = layout.panes.iter().find(|p| p.pane_id == pane_id) {
                        let cursor_col = X_OFFSET + pane.x + c.col;
                        let cursor_row = Y_OFFSET + pane.y + c.row;
                        screen_buffer.set_cursor(cursor_row, cursor_col, c.visible);
                    }
                }
            }

            Ok(MessageResult::PaneUpdated)
        }
        ServerMessage::Detached { reason } => Ok(MessageResult::Detached(reason)),
        ServerMessage::Shutdown => Ok(MessageResult::Shutdown),
        ServerMessage::Error { message } => {
            log::error!("Server error: {}", message);
            Ok(MessageResult::Continue)
        }
        ServerMessage::MouseMode { enabled } => {
            log::info!("Mouse mode changed: enabled={}", enabled);
            Ok(MessageResult::MouseModeChanged(enabled))
        }
        _ => {
            // Ignore other messages
            Ok(MessageResult::Continue)
        }
    }
}

/// Render the entire screen buffer to stdout.
/// Used after layout changes.
#[allow(dead_code)]
fn render_screen_buffer(
    stdout: &mut io::Stdout,
    screen_buffer: &ScreenBuffer,
) -> anyhow::Result<()> {
    const X_OFFSET: u16 = 1;
    const Y_OFFSET: u16 = 1;

    let (_screen_cols, screen_rows) = screen_buffer.dimensions();

    for row_idx in 0..screen_rows {
        if let Some(row_cells) = screen_buffer.get_row(row_idx) {
            queue!(stdout, MoveTo(X_OFFSET, Y_OFFSET + row_idx as u16))?;
            let ansi = cells_to_ansi(row_cells);
            write!(stdout, "{}", ansi)?;
        }
    }

    stdout.flush()?;
    Ok(())
}

/// Internal action result.
#[derive(Debug)]
enum InternalAction {
    Detach,
    Quit,
    SendPrefix,
    Command(CommandAction),
}

/// Convert a command-mode key to an action.
fn key_to_command_action(key: &event::KeyEvent, config: &Config) -> Option<InternalAction> {
    let key_char = match key.code {
        KeyCode::Char(c) => Some(c),
        KeyCode::Up => {
            return Some(InternalAction::Command(CommandAction::NavigatePane(
                Direction::Up,
            )))
        }
        KeyCode::Down => {
            return Some(InternalAction::Command(CommandAction::NavigatePane(
                Direction::Down,
            )))
        }
        KeyCode::Left => {
            return Some(InternalAction::Command(CommandAction::NavigatePane(
                Direction::Left,
            )))
        }
        KeyCode::Right => {
            return Some(InternalAction::Command(CommandAction::NavigatePane(
                Direction::Right,
            )))
        }
        _ => None,
    };

    let c = key_char?;

    // Check app bindings FIRST (detach, quit, send_prefix take priority)
    if c.to_string() == config.keybindings.app.detach {
        return Some(InternalAction::Detach);
    }
    if c.to_string() == config.keybindings.app.quit {
        return Some(InternalAction::Quit);
    }
    if c.to_string() == config.keybindings.app.send_prefix {
        return Some(InternalAction::SendPrefix);
    }

    // Check pane bindings
    if c.to_string() == config.keybindings.pane.split_horizontal {
        return Some(InternalAction::Command(CommandAction::SplitHorizontal));
    }
    if c.to_string() == config.keybindings.pane.split_vertical {
        return Some(InternalAction::Command(CommandAction::SplitVertical));
    }
    if c.to_string() == config.keybindings.pane.close {
        return Some(InternalAction::Command(CommandAction::ClosePane));
    }
    if c.to_string() == config.keybindings.pane.navigate_up {
        return Some(InternalAction::Command(CommandAction::NavigatePane(
            Direction::Up,
        )));
    }
    if c.to_string() == config.keybindings.pane.navigate_down {
        return Some(InternalAction::Command(CommandAction::NavigatePane(
            Direction::Down,
        )));
    }
    if c.to_string() == config.keybindings.pane.navigate_left {
        return Some(InternalAction::Command(CommandAction::NavigatePane(
            Direction::Left,
        )));
    }
    if c.to_string() == config.keybindings.pane.navigate_right {
        return Some(InternalAction::Command(CommandAction::NavigatePane(
            Direction::Right,
        )));
    }

    // Check window bindings
    if c.to_string() == config.keybindings.window.new {
        return Some(InternalAction::Command(CommandAction::NewWindow));
    }
    if c.to_string() == config.keybindings.window.close {
        return Some(InternalAction::Command(CommandAction::CloseWindow));
    }
    if c.to_string() == config.keybindings.window.next {
        return Some(InternalAction::Command(CommandAction::NextWindow));
    }
    if c.to_string() == config.keybindings.window.previous {
        return Some(InternalAction::Command(CommandAction::PrevWindow));
    }

    // Check window selection (1-9, 0)
    if let Some(n) = c.to_digit(10) {
        let index = if n == 0 { 9 } else { (n - 1) as usize };
        return Some(InternalAction::Command(CommandAction::SelectWindow(index)));
    }

    None
}

/// Convert a key event to bytes to send to PTY.
fn key_to_bytes(key: &event::KeyEvent) -> Option<Vec<u8>> {
    let mut bytes = Vec::new();

    match key.code {
        KeyCode::Char(c) => {
            if key.modifiers.contains(KeyModifiers::CONTROL) {
                // Ctrl+letter sends 1-26
                if c.is_ascii_alphabetic() {
                    let ctrl_char = (c.to_ascii_uppercase() as u8) - b'A' + 1;
                    bytes.push(ctrl_char);
                }
            } else if key.modifiers.contains(KeyModifiers::ALT) {
                // Alt sends ESC prefix
                bytes.push(0x1b);
                bytes.push(c as u8);
            } else {
                let mut buf = [0u8; 4];
                let s = c.encode_utf8(&mut buf);
                bytes.extend_from_slice(s.as_bytes());
            }
        }
        KeyCode::Enter => bytes.push(b'\r'),
        KeyCode::Tab => bytes.push(b'\t'),
        KeyCode::Backspace => bytes.push(0x7f),
        KeyCode::Esc => bytes.push(0x1b),
        KeyCode::Up => bytes.extend_from_slice(b"\x1b[A"),
        KeyCode::Down => bytes.extend_from_slice(b"\x1b[B"),
        KeyCode::Right => bytes.extend_from_slice(b"\x1b[C"),
        KeyCode::Left => bytes.extend_from_slice(b"\x1b[D"),
        KeyCode::Home => bytes.extend_from_slice(b"\x1b[H"),
        KeyCode::End => bytes.extend_from_slice(b"\x1b[F"),
        KeyCode::PageUp => bytes.extend_from_slice(b"\x1b[5~"),
        KeyCode::PageDown => bytes.extend_from_slice(b"\x1b[6~"),
        KeyCode::Insert => bytes.extend_from_slice(b"\x1b[2~"),
        KeyCode::Delete => bytes.extend_from_slice(b"\x1b[3~"),
        KeyCode::F(n) => {
            let seq = match n {
                1 => b"\x1bOP".as_slice(),
                2 => b"\x1bOQ",
                3 => b"\x1bOR",
                4 => b"\x1bOS",
                5 => b"\x1b[15~",
                6 => b"\x1b[17~",
                7 => b"\x1b[18~",
                8 => b"\x1b[19~",
                9 => b"\x1b[20~",
                10 => b"\x1b[21~",
                11 => b"\x1b[23~",
                12 => b"\x1b[24~",
                _ => return None,
            };
            bytes.extend_from_slice(seq);
        }
        _ => return None,
    }

    if bytes.is_empty() {
        None
    } else {
        Some(bytes)
    }
}

/// Format a unix timestamp as a human-readable time ago string.
fn format_time_ago(timestamp: u64) -> String {
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

fn print_help() {
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

#[cfg(test)]
mod tests {
    use super::*;

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
}

// ╭──────────────────────────────────────────────────────────────╮
// │                       Border Rendering                       │
// ╰──────────────────────────────────────────────────────────────╯

/// The purple color used for the clux border.
const BORDER_COLOR: Color = Color::Rgb {
    r: 147,
    g: 112,
    b: 219,
};

/// Render the clux border around the terminal.
fn render_border(
    stdout: &mut io::Stdout,
    cols: u16,
    rows: u16,
    session_name: &str,
    window_info: &str,
) -> io::Result<()> {
    queue!(stdout, SetForegroundColor(BORDER_COLOR))?;

    // Top border with corners
    queue!(stdout, MoveTo(0, 0), Print("╭"))?;

    // Build top border content with window info
    let top_content = if !window_info.is_empty() {
        format!(" {} ", window_info)
    } else {
        String::new()
    };
    let top_chars: Vec<char> = top_content.chars().collect();

    for x in 1..cols.saturating_sub(1) {
        let idx = (x - 1) as usize;
        if idx < top_chars.len() {
            queue!(stdout, Print(top_chars[idx]))?;
        } else {
            queue!(stdout, Print("─"))?;
        }
    }
    queue!(stdout, Print("╮"))?;

    // Side borders
    for row in 1..rows.saturating_sub(1) {
        queue!(stdout, MoveTo(0, row), Print("│"))?;
        queue!(stdout, MoveTo(cols.saturating_sub(1), row), Print("│"))?;
    }

    // Bottom border with corners and "clux" label + session name
    let bottom_row = rows.saturating_sub(1);
    queue!(stdout, MoveTo(0, bottom_row), Print("╰"))?;

    // Build label with session name
    let label = if !session_name.is_empty() {
        format!(" clux:{} ", session_name)
    } else {
        " clux ".to_string()
    };
    let label_chars: Vec<char> = label.chars().collect();
    let label_len = label_chars.len() as u16;
    let border_width = cols.saturating_sub(2);
    let label_start = (border_width.saturating_sub(label_len)) / 2;

    for x in 1..cols.saturating_sub(1) {
        let pos = x - 1;
        if pos >= label_start && pos < label_start + label_len {
            let label_idx = (pos - label_start) as usize;
            queue!(stdout, Print(label_chars[label_idx]))?;
        } else {
            queue!(stdout, Print("─"))?;
        }
    }
    queue!(stdout, Print("╯"))?;

    queue!(stdout, ResetColor)?;
    Ok(())
}

/// Update just the frame time display in the top-right corner of the border.
fn update_frame_time(stdout: &mut io::Stdout, cols: u16, frame_info: &str) -> io::Result<()> {
    // Format: " 0.00ms " in top-right corner
    let display = format!(" {} ", frame_info);
    let display_len = display.len() as u16;
    let x_pos = cols.saturating_sub(display_len + 1); // +1 for the corner

    queue!(stdout, crossterm::cursor::SavePosition)?;
    queue!(stdout, SetForegroundColor(Color::DarkGrey))?;
    queue!(stdout, MoveTo(x_pos, 0))?;
    queue!(stdout, Print(&display))?;
    queue!(stdout, ResetColor)?;
    queue!(stdout, crossterm::cursor::RestorePosition)?;
    stdout.flush()?;
    Ok(())
}

/// Get inner dimensions (terminal size minus border).
fn inner_dimensions() -> (u16, u16) {
    let (cols, rows) = crossterm::terminal::size().unwrap_or((80, 24));
    (cols.saturating_sub(2), rows.saturating_sub(2))
}

/// Set up logging to file.
fn setup_logging(config: &Config) -> anyhow::Result<()> {
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
