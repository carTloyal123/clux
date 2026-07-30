//! Server module for clux.
//!
//! The server manages sessions, handles client connections, and routes
//! PTY I/O between shells and connected clients.

mod client_conn;
mod listener;
pub mod stdio_bridge;

pub use client_conn::{ClientConnection, ClientState};
pub use listener::SocketListener;

use std::collections::HashMap;
use std::io::{self, ErrorKind};
use std::os::unix::io::AsRawFd;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use nix::libc;

use mio::unix::SourceFd;
use mio::{Events, Interest, Poll, Token};

use crate::pane::PaneId;
use crate::protocol::{
    ClientMessage, CursorState, DetachReason, PaneLayout, PaneRow, ProtocolError, ServerMessage,
    WindowLayout, PROTOCOL_VERSION,
};
use crate::pty::detect_shell;
use crate::session::{ClientId, SessionId, SessionManager};

/// Token for the listener socket.
const LISTENER_TOKEN: Token = Token(0);

/// Base token for client connections (CLIENT_BASE + client_id).
const CLIENT_TOKEN_BASE: usize = 1000;

/// Base token for PTY file descriptors (PTY_BASE + session_id * 1000 + pane_index).
const PTY_TOKEN_BASE: usize = 100_000;

/// Server configuration.
#[derive(Debug, Clone)]
pub struct ServerConfig {
    /// Path to the Unix socket.
    pub socket_path: PathBuf,
    /// Shell to use for new sessions.
    pub shell: String,
    /// Default terminal dimensions.
    pub default_cols: u16,
    pub default_rows: u16,
    /// Turn URL-shaped text into real hyperlinks for the host terminal.
    ///
    /// Host terminals detect URLs against their own grid, where every clux row
    /// looks like a hard-wrapped line, so they cannot follow a URL that wraps
    /// inside a pane. Clux knows where its logical lines end, so it resolves
    /// those links itself. See [`crate::urls`].
    pub detect_plain_urls: bool,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            socket_path: default_socket_path(),
            shell: detect_shell(),
            default_cols: 80,
            default_rows: 24,
            detect_plain_urls: true,
        }
    }
}

/// Get the default socket path for the current user.
pub fn default_socket_path() -> PathBuf {
    let uid = unsafe { libc::getuid() };
    let dir = PathBuf::from(format!("/tmp/clux-{}", uid));
    dir.join("clux.sock")
}

/// Server error type.
#[derive(Debug, thiserror::Error)]
pub enum ServerError {
    #[error("IO error: {0}")]
    Io(#[from] io::Error),

    #[error("Protocol error: {0}")]
    Protocol(#[from] ProtocolError),

    #[error("Session error: {0}")]
    Session(#[from] crate::session::SessionError),

    #[error("Server already running at {0}")]
    AlreadyRunning(PathBuf),

    #[error("Failed to create socket directory: {0}")]
    SocketDir(io::Error),
}

/// Result type for server operations.
pub type ServerResult<T> = Result<T, ServerError>;

/// Configuration for automatic server shutdown.
#[derive(Debug, Clone)]
pub struct AutoShutdownConfig {
    /// Whether auto-shutdown is enabled.
    pub enabled: bool,
    /// Grace period before shutdown after last session closes.
    /// This allows for rapid "close session, create new session" workflows.
    pub grace_period: Duration,
    /// Timeout for first session creation after server start.
    /// If no session is created within this time, the server shuts down.
    /// This handles orphaned servers from failed client startup.
    pub first_session_timeout: Duration,
}

impl Default for AutoShutdownConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            grace_period: Duration::from_secs(1),
            first_session_timeout: Duration::from_secs(30),
        }
    }
}

/// The clux server.
pub struct Server {
    /// Server configuration.
    config: ServerConfig,
    /// mio poll instance.
    poll: Poll,
    /// Socket listener.
    listener: SocketListener,
    /// Session manager.
    sessions: SessionManager,
    /// Connected clients.
    clients: HashMap<ClientId, ClientConnection>,
    /// Mapping from client token to client ID.
    token_to_client: HashMap<Token, ClientId>,
    /// Mapping from PTY token to (SessionId, PaneId).
    token_to_pty: HashMap<Token, (crate::session::SessionId, crate::pane::PaneId)>,
    /// Client terminal sizes (for multi-client resize calculation).
    client_sizes: HashMap<ClientId, (u16, u16)>,
    /// Next client ID to assign.
    next_client_id: u32,
    /// Whether the server should continue running.
    running: bool,
    /// Auto-shutdown configuration.
    auto_shutdown: AutoShutdownConfig,
    /// Time when the server started (for first-session timeout).
    started_at: Instant,
    /// Time when shutdown became pending (last session closed).
    /// None if sessions exist or shutdown not pending.
    shutdown_pending_since: Option<Instant>,
    /// Whether a session has ever been created (to distinguish startup from post-session state).
    session_ever_created: bool,
}

impl Server {
    /// Create a new server with the given configuration.
    pub fn new(config: ServerConfig) -> ServerResult<Self> {
        Self::with_auto_shutdown(config, AutoShutdownConfig::default())
    }

    /// Create a new server with custom auto-shutdown configuration.
    pub fn with_auto_shutdown(
        config: ServerConfig,
        auto_shutdown: AutoShutdownConfig,
    ) -> ServerResult<Self> {
        let poll = Poll::new()?;

        // Create socket directory if needed
        if let Some(parent) = config.socket_path.parent() {
            std::fs::create_dir_all(parent).map_err(ServerError::SocketDir)?;
        }

        // Create and register the listener
        let listener = SocketListener::bind(&config.socket_path)?;
        poll.registry().register(
            &mut SourceFd(&listener.as_raw_fd()),
            LISTENER_TOKEN,
            Interest::READABLE,
        )?;

        log::info!("Server listening on {:?}", config.socket_path);
        if auto_shutdown.enabled {
            log::info!(
                "Auto-shutdown enabled: grace_period={:?}, first_session_timeout={:?}",
                auto_shutdown.grace_period,
                auto_shutdown.first_session_timeout
            );
        } else {
            log::info!("Auto-shutdown disabled (daemon mode)");
        }

        Ok(Self {
            sessions: SessionManager::new(config.shell.clone()),
            config,
            poll,
            listener,
            clients: HashMap::new(),
            token_to_client: HashMap::new(),
            token_to_pty: HashMap::new(),
            client_sizes: HashMap::new(),
            next_client_id: 0,
            running: true,
            auto_shutdown,
            started_at: Instant::now(),
            shutdown_pending_since: None,
            session_ever_created: false,
        })
    }

    /// Run the server event loop.
    pub fn run(&mut self) -> ServerResult<()> {
        let mut events = Events::with_capacity(128);
        let timeout = Duration::from_millis(100);

        while self.running {
            self.poll.poll(&mut events, Some(timeout))?;

            for event in events.iter() {
                match event.token() {
                    LISTENER_TOKEN => {
                        self.accept_client()?;
                    }
                    token if token.0 >= CLIENT_TOKEN_BASE && token.0 < PTY_TOKEN_BASE => {
                        if let Some(&client_id) = self.token_to_client.get(&token) {
                            self.handle_client_event(client_id)?;
                        }
                    }
                    token if token.0 >= PTY_TOKEN_BASE => {
                        // TODO: Handle PTY events (Phase 7.6)
                        self.handle_pty_event(token)?;
                    }
                    _ => {}
                }
            }

            // Check for dead clients
            self.cleanup_dead_clients();

            // Check for dead panes (shells that exited)
            self.cleanup_dead_panes();

            // Check auto-shutdown conditions
            self.check_auto_shutdown();
        }

        log::info!("Server shutting down");
        Ok(())
    }

    /// Check for and clean up dead panes across all sessions.
    fn cleanup_dead_panes(&mut self) {
        // Collect dead panes from all sessions
        let mut dead_panes: Vec<(SessionId, PaneId)> = Vec::new();

        for (session_id, session) in self.sessions.iter_mut() {
            for (_, pane_id) in session.window_manager.check_dead_panes() {
                dead_panes.push((*session_id, pane_id));
            }
        }

        if dead_panes.is_empty() {
            return;
        }

        // Track which sessions need screen refresh
        let mut sessions_to_refresh: Vec<SessionId> = Vec::new();
        // Track sessions to close (last pane died)
        let mut sessions_to_close: Vec<SessionId> = Vec::new();

        // Close dead panes and deregister their PTYs
        for (session_id, pane_id) in dead_panes {
            // First deregister the PTY
            self.deregister_pane_pty(session_id, pane_id);

            // Then close the pane in the window manager
            if let Some(session) = self.sessions.get_mut(session_id) {
                // Check if this is the last pane in the entire session
                let total_panes = session.window_manager.total_pane_count();

                if total_panes == 1 {
                    // This is the last pane in the session - close the session
                    log::info!(
                        "Last pane {:?} in session {:?} died, closing session",
                        pane_id,
                        session_id
                    );
                    if !sessions_to_close.contains(&session_id) {
                        sessions_to_close.push(session_id);
                    }
                } else if session.window_manager.close_pane(pane_id) {
                    log::info!("Closed dead pane {:?} in session {:?}", pane_id, session_id);
                    if !sessions_to_refresh.contains(&session_id) {
                        sessions_to_refresh.push(session_id);
                    }
                }
            }
        }

        // Close sessions where the last pane died
        for session_id in sessions_to_close {
            // Notify all attached clients
            let attached_clients: Vec<ClientId> = self
                .sessions
                .get(session_id)
                .map(|s| s.attached_clients().to_vec())
                .unwrap_or_default();

            for client_id in attached_clients {
                let _ = self.send_to_client(
                    client_id,
                    &ServerMessage::Detached {
                        reason: DetachReason::SessionClosed,
                    },
                );
            }

            // Clean up PTY mappings for this session
            self.token_to_pty.retain(|_, (sid, _)| *sid != session_id);

            // Close the session
            self.sessions.close_session(session_id);
            log::info!("Session {:?} closed (last pane exited)", session_id);
        }

        // Send screen refresh to affected sessions
        for session_id in sessions_to_refresh {
            if let Err(e) = self.broadcast_full_screen(session_id) {
                log::warn!(
                    "Failed to refresh screen after pane cleanup for session {:?}: {}",
                    session_id,
                    e
                );
            }
        }
    }

    /// Check auto-shutdown conditions and stop the server if met.
    ///
    /// This implements session-driven server lifetime:
    /// - Server shuts down when all sessions are closed (after grace period)
    /// - Server shuts down if no session is created within first_session_timeout
    fn check_auto_shutdown(&mut self) {
        if !self.auto_shutdown.enabled {
            return;
        }

        let now = Instant::now();
        let has_sessions = self.sessions.count() > 0;

        if has_sessions {
            // Sessions exist - cancel any pending shutdown and mark that we've had sessions
            self.shutdown_pending_since = None;
            self.session_ever_created = true;
            return;
        }

        // No sessions exist - check shutdown conditions

        // Case 1: First-session timeout (server started but no session ever created)
        if !self.session_ever_created {
            let elapsed = now.duration_since(self.started_at);
            if elapsed >= self.auto_shutdown.first_session_timeout {
                log::info!(
                    "No session created after {:?}, shutting down (first-session timeout)",
                    elapsed
                );
                self.running = false;
                return;
            }
            // Still waiting for first session
            return;
        }

        // Case 2: Grace period after last session closed
        match self.shutdown_pending_since {
            Some(pending_since) => {
                let elapsed = now.duration_since(pending_since);
                if elapsed >= self.auto_shutdown.grace_period {
                    log::info!(
                        "Last session closed {:?} ago, shutting down (grace period expired)",
                        elapsed
                    );
                    self.running = false;
                }
                // Still within grace period
            }
            None => {
                // Start the grace period countdown
                log::info!(
                    "All sessions closed, starting {:?} grace period before shutdown",
                    self.auto_shutdown.grace_period
                );
                self.shutdown_pending_since = Some(now);
            }
        }
    }

    /// Accept a new client connection.
    fn accept_client(&mut self) -> ServerResult<()> {
        match self.listener.accept() {
            Ok(stream) => {
                let client_id = ClientId(self.next_client_id);
                self.next_client_id += 1;

                let token = Token(CLIENT_TOKEN_BASE + client_id.0 as usize);

                // Register the client socket for reading
                self.poll.registry().register(
                    &mut SourceFd(&stream.as_raw_fd()),
                    token,
                    Interest::READABLE,
                )?;

                let conn = ClientConnection::new(client_id, stream);
                self.clients.insert(client_id, conn);
                self.token_to_client.insert(token, client_id);

                log::info!("Accepted client {:?}", client_id);
                Ok(())
            }
            Err(e) if e.kind() == ErrorKind::WouldBlock => Ok(()),
            Err(e) => Err(ServerError::Io(e)),
        }
    }

    /// Handle an event from a client socket.
    fn handle_client_event(&mut self, client_id: ClientId) -> ServerResult<()> {
        // Read message from client
        let message = {
            let client = match self.clients.get_mut(&client_id) {
                Some(c) => c,
                None => return Ok(()),
            };

            match client.try_read_message() {
                Ok(Some(msg)) => msg,
                Ok(None) => return Ok(()), // No complete message yet
                Err(ProtocolError::ConnectionClosed) => {
                    log::info!("Client {:?} disconnected", client_id);
                    self.remove_client(client_id);
                    return Ok(());
                }
                Err(e) => {
                    log::warn!("Client {:?} protocol error: {}", client_id, e);
                    self.remove_client(client_id);
                    return Ok(());
                }
            }
        };

        // Process the message
        self.process_client_message(client_id, message)
    }

    /// Process a message from a client.
    fn process_client_message(
        &mut self,
        client_id: ClientId,
        message: ClientMessage,
    ) -> ServerResult<()> {
        log::debug!("Client {:?} sent: {:?}", client_id, message);

        match message {
            ClientMessage::Hello {
                version,
                term_cols,
                term_rows,
                term_type: _,
            } => {
                // Store client size
                self.client_sizes.insert(client_id, (term_cols, term_rows));

                // Send HelloAck
                let response = ServerMessage::HelloAck {
                    version: PROTOCOL_VERSION,
                    server_pid: std::process::id(),
                };
                self.send_to_client(client_id, &response)?;

                // Update client state
                if let Some(client) = self.clients.get_mut(&client_id) {
                    client.state = ClientState::Ready;

                    // Check version compatibility
                    if version != PROTOCOL_VERSION {
                        log::warn!(
                            "Client {:?} version mismatch: {} vs {}",
                            client_id,
                            version,
                            PROTOCOL_VERSION
                        );
                    }
                }
            }

            ClientMessage::Attach {
                session_name,
                create,
            } => {
                self.handle_attach(client_id, session_name, create)?;
            }

            ClientMessage::Detach => {
                self.handle_detach(client_id)?;
            }

            ClientMessage::Input(bytes) => {
                self.handle_input(client_id, bytes)?;
            }

            ClientMessage::Resize { cols, rows } => {
                self.handle_resize(client_id, cols, rows)?;
            }

            ClientMessage::Scroll { lines } => {
                self.handle_scroll(client_id, lines)?;
            }

            ClientMessage::Command(action) => {
                self.handle_command(client_id, action)?;
            }

            ClientMessage::ListSessions => {
                let list = self.sessions.list_info();
                self.send_to_client(client_id, &ServerMessage::SessionList(list))?;
            }

            ClientMessage::KillSession { name } => {
                self.handle_kill_session(client_id, &name)?;
            }

            ClientMessage::RenameSession { new_name } => {
                self.handle_rename_session(client_id, new_name)?;
            }

            ClientMessage::Ping => {
                self.send_to_client(client_id, &ServerMessage::Pong)?;
            }

            ClientMessage::ShutdownServer => {
                log::info!("Shutdown requested by client {:?}", client_id);
                self.running = false;
            }
        }

        Ok(())
    }

    /// Handle client attach request.
    fn handle_attach(
        &mut self,
        client_id: ClientId,
        session_name: Option<String>,
        create: bool,
    ) -> ServerResult<()> {
        let (cols, rows) = self
            .client_sizes
            .get(&client_id)
            .copied()
            .unwrap_or((self.config.default_cols, self.config.default_rows));

        // Find or create the session
        let normalized_session_name = match session_name {
            Some(name) => match SessionManager::normalize_session_name(name) {
                Ok(name) => Some(name),
                Err(err) => {
                    self.send_to_client(
                        client_id,
                        &ServerMessage::Error {
                            message: err.to_string(),
                        },
                    )?;
                    return Ok(());
                }
            },
            None => None,
        };

        let (session_id, newly_created) = if let Some(ref name) = normalized_session_name {
            if let Some(id) = self.sessions.id_for_name(name) {
                (id, false)
            } else if create {
                let id = self
                    .sessions
                    .create_session(Some(name.clone()), cols, rows)?;
                (id, true)
            } else {
                self.send_to_client(
                    client_id,
                    &ServerMessage::Error {
                        message: format!("Session '{}' not found", name),
                    },
                )?;
                return Ok(());
            }
        } else {
            // Attach to default session (may create)
            let had_sessions = self.sessions.count() > 0;
            let id = self.sessions.get_or_create_default(cols, rows)?;
            (id, !had_sessions)
        };

        // Register PTYs if session was just created
        if newly_created {
            self.register_session_ptys(session_id)?;
        }

        // Attach client to session and get session name
        let session_name = if let Some(session) = self.sessions.get_mut(session_id) {
            session.attach_client(client_id);

            // Recalculate effective size (smallest-client-wins)
            let (eff_cols, eff_rows) = session.effective_size(&self.client_sizes);
            if let Err(e) = session.window_manager.resize(eff_cols, eff_rows) {
                log::warn!("Failed to resize session after attach: {}", e);
            }

            session.name.clone()
        } else {
            return Ok(());
        };

        // Update client state
        if let Some(client) = self.clients.get_mut(&client_id) {
            client.state = ClientState::Attached(session_id);
        }

        // Send attached confirmation
        self.send_to_client(
            client_id,
            &ServerMessage::Attached {
                session_id: session_id.0,
                session_name: session_name.clone(),
            },
        )?;

        // Send the initial screen: layout, then every pane's content
        if let Some(layout) = self.build_window_layout(session_id) {
            let layout_msg = ServerMessage::LayoutChanged { layout };
            self.send_to_client(client_id, &layout_msg)?;
            self.send_all_pane_updates(session_id, &[client_id])?;
        }

        log::info!(
            "Client {:?} attached to session {:?} '{}'",
            client_id,
            session_id,
            session_name
        );

        Ok(())
    }

    /// Handle client detach request.
    fn handle_detach(&mut self, client_id: ClientId) -> ServerResult<()> {
        // Get the session this client is attached to
        let session_id = {
            let client = match self.clients.get(&client_id) {
                Some(c) => c,
                None => return Ok(()),
            };

            match client.state {
                ClientState::Attached(id) => id,
                _ => return Ok(()),
            }
        };

        // Detach from session
        if let Some(session) = self.sessions.get_mut(session_id) {
            session.detach_client(client_id);

            // Recalculate effective size after client leaves
            let (eff_cols, eff_rows) = session.effective_size(&self.client_sizes);
            if let Err(e) = session.window_manager.resize(eff_cols, eff_rows) {
                log::warn!("Failed to resize session after detach: {}", e);
            }
        }

        // Update client state
        if let Some(client) = self.clients.get_mut(&client_id) {
            client.state = ClientState::Ready;
        }

        // Remove client size
        self.client_sizes.remove(&client_id);

        // Send detached confirmation
        self.send_to_client(
            client_id,
            &ServerMessage::Detached {
                reason: DetachReason::ClientRequested,
            },
        )?;

        Ok(())
    }

    /// Handle input from a client.
    fn handle_input(&mut self, client_id: ClientId, bytes: Vec<u8>) -> ServerResult<()> {
        // Get the session this client is attached to
        let session_id = {
            let client = match self.clients.get(&client_id) {
                Some(c) => c,
                None => return Ok(()),
            };

            match client.state {
                ClientState::Attached(id) => id,
                _ => return Ok(()),
            }
        };

        // Write to the focused pane's PTY
        let mut left_scrollback = false;
        if let Some(session) = self.sessions.get_mut(session_id) {
            if let Some(pane) = session.window_manager.focused_pane_mut() {
                // Typing returns to the live view, as in any terminal.
                left_scrollback = pane.terminal.reset_scroll();

                if let Err(e) = pane.pty.write_all(&bytes) {
                    log::warn!("Failed to write to PTY: {}", e);
                }
            }
        }

        if left_scrollback {
            self.send_focused_pane_update(session_id)?;
        }

        Ok(())
    }

    /// Scroll the focused pane through its scrollback.
    ///
    /// `lines` is positive to go back in history, negative to come forward, zero
    /// to jump back to the live view.
    fn handle_scroll(&mut self, client_id: ClientId, lines: i32) -> ServerResult<()> {
        let session_id = match self.clients.get(&client_id).map(|c| c.state) {
            Some(ClientState::Attached(id)) => id,
            _ => return Ok(()),
        };

        let moved = match self.sessions.get_mut(session_id) {
            Some(session) => match session.window_manager.focused_pane_mut() {
                Some(pane) => {
                    if lines == 0 {
                        pane.terminal.reset_scroll()
                    } else {
                        pane.terminal.scroll_view(lines)
                    }
                }
                None => false,
            },
            None => false,
        };

        if moved {
            self.send_focused_pane_update(session_id)?;
        }

        Ok(())
    }

    /// Resend the focused pane to every attached client.
    ///
    /// Used when the view moved rather than the content: the whole pane is dirty,
    /// and the cursor is hidden while looking at history.
    fn send_focused_pane_update(&mut self, session_id: SessionId) -> ServerResult<()> {
        let detect_plain_urls = self.config.detect_plain_urls;

        let update = {
            let Some(session) = self.sessions.get_mut(session_id) else {
                return Ok(());
            };
            let Some(pane) = session.window_manager.focused_pane_mut() else {
                return Ok(());
            };

            let rows_to_send = pane.terminal.take_dirty_rows();
            if rows_to_send.is_empty() {
                return Ok(());
            }

            let mut links =
                pane.terminal
                    .resolve_links(pane.id.0, detect_plain_urls, &rows_to_send);

            let changed_rows: Vec<PaneRow> = rows_to_send
                .iter()
                .map(|&row_idx| {
                    let view = pane.terminal.view_row(row_idx);
                    let row_links = links
                        .remove(&row_idx)
                        .unwrap_or_default()
                        .into_iter()
                        .map(Into::into)
                        .collect();
                    PaneRow::with_links(row_idx, view.cells, row_links).wrapped(view.wrapped)
                })
                .collect();

            let cursor = pane.terminal.cursor();
            ServerMessage::PaneUpdate {
                pane_id: pane.id.0,
                changed_rows,
                cursor: Some(CursorState {
                    row: cursor.row as u16,
                    col: cursor.col as u16,
                    // No cursor while viewing history: it belongs to the live view.
                    visible: cursor.visible && !pane.terminal.is_scrolled(),
                }),
            }
        };

        self.broadcast_to_session(session_id, &update)
    }

    /// Handle terminal resize from a client.
    fn handle_resize(&mut self, client_id: ClientId, cols: u16, rows: u16) -> ServerResult<()> {
        log::info!(
            "handle_resize: client={:?}, size={}x{}",
            client_id,
            cols,
            rows
        );

        // Update stored client size
        self.client_sizes.insert(client_id, (cols, rows));

        // Get the session this client is attached to
        let session_id = {
            let client = match self.clients.get(&client_id) {
                Some(c) => c,
                None => return Ok(()),
            };

            match client.state {
                ClientState::Attached(id) => id,
                _ => return Ok(()),
            }
        };

        // Recalculate effective size and resize session
        if let Some(session) = self.sessions.get_mut(session_id) {
            let (eff_cols, eff_rows) = session.effective_size(&self.client_sizes);
            log::info!(
                "Resizing session {:?} to {}x{}",
                session_id,
                eff_cols,
                eff_rows
            );
            if let Err(e) = session.window_manager.resize(eff_cols, eff_rows) {
                log::warn!("Failed to resize session: {}", e);
            }
        }

        // Send full screen refresh to all clients after resize
        self.broadcast_full_screen(session_id)?;

        Ok(())
    }

    /// Handle a command action from a client.
    fn handle_command(
        &mut self,
        client_id: ClientId,
        action: crate::protocol::CommandAction,
    ) -> ServerResult<()> {
        // Get the session this client is attached to
        let session_id = {
            let client = match self.clients.get(&client_id) {
                Some(c) => c,
                None => return Ok(()),
            };

            match client.state {
                ClientState::Attached(id) => id,
                _ => return Ok(()),
            }
        };

        // Track post-action work needed (to avoid borrow conflicts)
        enum PostAction {
            None,
            Refresh,
            RegisterPtyAndRefresh(PaneId),
            DeregisterPtyAndRefresh(PaneId),
            CloseSession,
        }

        let post_action = {
            let session = match self.sessions.get_mut(session_id) {
                Some(s) => s,
                None => return Ok(()),
            };

            use crate::protocol::CommandAction::*;
            match action {
                SplitHorizontal => {
                    log::info!("Executing SplitHorizontal command");
                    match session
                        .window_manager
                        .split(crate::pane::SplitDirection::Horizontal)
                    {
                        Ok(new_pane_id) => {
                            log::info!("Split horizontal created new pane {:?}", new_pane_id);
                            PostAction::RegisterPtyAndRefresh(new_pane_id)
                        }
                        Err(e) => {
                            log::error!("Split horizontal failed: {:?}", e);
                            PostAction::None
                        }
                    }
                }
                SplitVertical => {
                    log::info!("Executing SplitVertical command");
                    match session
                        .window_manager
                        .split(crate::pane::SplitDirection::Vertical)
                    {
                        Ok(new_pane_id) => {
                            log::info!("Split vertical created new pane {:?}", new_pane_id);
                            PostAction::RegisterPtyAndRefresh(new_pane_id)
                        }
                        Err(e) => {
                            log::error!("Split vertical failed: {:?}", e);
                            PostAction::None
                        }
                    }
                }
                ClosePane => {
                    let pane_id = session.window_manager.focused_pane_id();
                    session.window_manager.close_focused_pane();
                    PostAction::DeregisterPtyAndRefresh(pane_id)
                }
                NavigatePane(dir) => {
                    let pane_dir = match dir {
                        crate::protocol::Direction::Up => crate::pane::Direction::Up,
                        crate::protocol::Direction::Down => crate::pane::Direction::Down,
                        crate::protocol::Direction::Left => crate::pane::Direction::Left,
                        crate::protocol::Direction::Right => crate::pane::Direction::Right,
                    };
                    session.window_manager.navigate_pane(pane_dir);
                    // Focus change needs screen refresh for visual feedback
                    PostAction::Refresh
                }
                NewWindow => {
                    if session.window_manager.create_window().is_ok() {
                        // The new window is now active, get its focused pane ID
                        let new_pane_id = session.window_manager.focused_pane_id();
                        PostAction::RegisterPtyAndRefresh(new_pane_id)
                    } else {
                        PostAction::None
                    }
                }
                CloseWindow => {
                    session.window_manager.close_active_window();
                    // Window close needs full screen refresh
                    PostAction::Refresh
                }
                NextWindow => {
                    session.window_manager.next_window();
                    // Window switch needs full screen refresh to show new window
                    PostAction::Refresh
                }
                PrevWindow => {
                    session.window_manager.prev_window();
                    // Window switch needs full screen refresh to show new window
                    PostAction::Refresh
                }
                SelectWindow(index) => {
                    session.window_manager.select_window(index);
                    // Window switch needs full screen refresh to show new window
                    PostAction::Refresh
                }
                Quit => PostAction::CloseSession,
            }
        };

        // Execute post-actions after session borrow is released
        match post_action {
            PostAction::None => {}
            PostAction::Refresh => {
                // Send full screen to all clients to show updated state
                self.broadcast_full_screen(session_id)?;
            }
            PostAction::RegisterPtyAndRefresh(pane_id) => {
                self.register_new_pane_pty(session_id, pane_id)?;
                // Send full screen to all clients to show new layout
                self.broadcast_full_screen(session_id)?;
            }
            PostAction::DeregisterPtyAndRefresh(pane_id) => {
                self.deregister_pane_pty(session_id, pane_id);
                // Send full screen to all clients to show updated layout
                self.broadcast_full_screen(session_id)?;
            }

            PostAction::CloseSession => {
                self.sessions.close_session(session_id);
                self.send_to_client(
                    client_id,
                    &ServerMessage::Detached {
                        reason: DetachReason::SessionClosed,
                    },
                )?;
            }
        }

        Ok(())
    }

    /// Handle kill session request.
    fn handle_kill_session(&mut self, client_id: ClientId, name: &str) -> ServerResult<()> {
        let normalized_name = match SessionManager::normalize_session_name(name.to_string()) {
            Ok(name) => name,
            Err(err) => {
                self.send_to_client(
                    client_id,
                    &ServerMessage::Error {
                        message: err.to_string(),
                    },
                )?;
                return Ok(());
            }
        };

        // Find session ID by name
        let session_id = self.sessions.id_for_name(&normalized_name);

        if let Some(session_id) = session_id {
            // Notify all attached clients that session is being killed
            let detach_msg = ServerMessage::Detached {
                reason: DetachReason::SessionClosed,
            };

            // Find clients attached to this session
            let attached_clients: Vec<ClientId> = self
                .clients
                .iter()
                .filter_map(|(&cid, client)| {
                    if let ClientState::Attached(sid) = client.state {
                        if sid == session_id {
                            return Some(cid);
                        }
                    }
                    None
                })
                .collect();

            // Send detach notification and update client state
            for cid in attached_clients {
                let _ = self.send_to_client(cid, &detach_msg);
                if let Some(client) = self.clients.get_mut(&cid) {
                    client.state = ClientState::Ready;
                }
            }

            // Remove PTY tokens for this session
            self.token_to_pty.retain(|_, (sid, _)| *sid != session_id);

            // Close the session
            self.sessions.close_session_by_name(&normalized_name);
            log::info!(
                "Session '{}' killed by client {:?}",
                normalized_name,
                client_id
            );
        } else {
            self.send_to_client(
                client_id,
                &ServerMessage::Error {
                    message: format!("Session '{}' not found", normalized_name),
                },
            )?;
        }
        Ok(())
    }

    /// Handle rename session request.
    fn handle_rename_session(&mut self, client_id: ClientId, new_name: String) -> ServerResult<()> {
        // Get the session this client is attached to
        let session_id = {
            let client = match self.clients.get(&client_id) {
                Some(c) => c,
                None => return Ok(()),
            };

            match client.state {
                ClientState::Attached(id) => id,
                _ => {
                    self.send_to_client(
                        client_id,
                        &ServerMessage::Error {
                            message: "Not attached to a session".to_string(),
                        },
                    )?;
                    return Ok(());
                }
            }
        };

        match self.sessions.rename_session(session_id, new_name) {
            Ok(()) => {}
            Err(e) => {
                self.send_to_client(
                    client_id,
                    &ServerMessage::Error {
                        message: e.to_string(),
                    },
                )?;
            }
        }

        Ok(())
    }

    /// Handle PTY event (output from shell).
    /// Composites the pane content at its correct screen position.
    fn handle_pty_event(&mut self, token: Token) -> ServerResult<()> {
        log::trace!("handle_pty_event: token={:?}", token);

        // Look up which session/pane this PTY belongs to
        let (session_id, pane_id) = match self.token_to_pty.get(&token) {
            Some(&ids) => ids,
            None => {
                log::warn!("No session/pane found for PTY token {:?}", token);
                return Ok(());
            }
        };

        log::trace!("PTY event for session={:?}, pane={:?}", session_id, pane_id);

        // Collect all data we need while holding mutable session borrow
        // Then drop the borrow before sending messages
        struct PtyEventData {
            mouse_mode_changed: Option<bool>,
            pane_update: Option<ServerMessage>,
        }

        let detect_plain_urls = self.config.detect_plain_urls;

        let event_data = {
            let mut buf = [0u8; 4096];
            let session = match self.sessions.get_mut(session_id) {
                Some(s) => s,
                None => {
                    log::warn!("Session {:?} not found for PTY event", session_id);
                    return Ok(());
                }
            };

            let focused_pane_id = session.window_manager.focused_pane_id();

            // Find the pane and read from its PTY
            let pane = match session.window_manager.find_pane_mut(pane_id) {
                Some(p) => p,
                None => {
                    log::warn!("Pane {:?} not found in session {:?}", pane_id, session_id);
                    return Ok(());
                }
            };

            // Get pane's screen position before reading
            let pane_rect = pane.rect;

            // Read available data (non-blocking - returns 0 if no data)
            let bytes_read = match pane.pty.read(&mut buf) {
                Ok(n) if n > 0 => n,
                Ok(_) => return Ok(()), // No data available
                Err(e) => {
                    log::warn!("PTY read error for {:?}/{:?}: {}", session_id, pane_id, e);
                    return Ok(());
                }
            };

            log::debug!(
                "PTY read {} bytes from session {:?} pane {:?}",
                bytes_read,
                session_id,
                pane_id
            );

            // Feed bytes through the terminal emulator (VTE parser)
            pane.parser.advance(&mut pane.terminal, &buf[..bytes_read]);

            // Check if mouse mode changed for the focused pane
            let current_mouse_mode = pane.terminal.mouse_mode();
            let mouse_mode_changed =
                if pane_id == focused_pane_id && current_mouse_mode != pane.last_mouse_mode {
                    log::info!(
                        "Mouse mode changed: {} -> {} for pane {:?}",
                        pane.last_mouse_mode,
                        current_mouse_mode,
                        pane_id
                    );
                    pane.last_mouse_mode = current_mouse_mode;
                    Some(current_mouse_mode != 0)
                } else {
                    None
                };

            // Get dirty rows from the terminal's grid
            let dirty_rows = pane.terminal.take_dirty_rows();

            if dirty_rows.is_empty() {
                log::trace!("No dirty rows after PTY read");
                return Ok(());
            }

            log::debug!(
                "PTY event: {} dirty rows for pane at ({}, {})",
                dirty_rows.len(),
                pane_rect.x,
                pane_rect.y
            );

            // Get cursor state from terminal
            let cursor = pane.terminal.cursor();

            // Cursor state in pane-local coordinates
            let pane_cursor = if pane_id == focused_pane_id {
                Some(CursorState {
                    row: cursor.row as u16,
                    col: cursor.col as u16,
                    visible: cursor.visible,
                })
            } else {
                None
            };

            // Resolve hyperlinks for the dirty rows. This can pull in extra rows:
            // when a link wraps, every row it covers has to be repainted or the
            // host terminal keeps the fragment it was given before the line grew.
            let mut links = pane
                .terminal
                .resolve_links(pane_id.0, detect_plain_urls, &dirty_rows);
            let rows_to_send = rows_with_links(&dirty_rows, &links);

            // Pane-local rows (cells without screen offset)
            let changed_rows: Vec<PaneRow> = rows_to_send
                .iter()
                .map(|&row_idx| {
                    let view = pane.terminal.view_row(row_idx);
                    let row_links = links
                        .remove(&row_idx)
                        .unwrap_or_default()
                        .into_iter()
                        .map(Into::into)
                        .collect();
                    PaneRow::with_links(row_idx, view.cells, row_links).wrapped(view.wrapped)
                })
                .collect();

            let pane_update = ServerMessage::PaneUpdate {
                pane_id: pane_id.0,
                changed_rows,
                cursor: pane_cursor,
            };

            PtyEventData {
                mouse_mode_changed,
                pane_update: Some(pane_update),
            }
        }; // session borrow ends here

        // Broadcast mouse mode change first if it changed
        if let Some(enabled) = event_data.mouse_mode_changed {
            let mouse_msg = ServerMessage::MouseMode { enabled };
            self.broadcast_to_session(session_id, &mouse_msg)?;
        }

        if let Some(ref pane_update) = event_data.pane_update {
            let attached: Vec<ClientId> = self
                .clients
                .iter()
                .filter(|(_, client)| client.state == ClientState::Attached(session_id))
                .map(|(&id, _)| id)
                .collect();

            for client_id in attached {
                self.send_to_client(client_id, pane_update)?;
            }
        }

        Ok(())
    }

    /// Register all PTYs for a session with mio.
    fn register_session_ptys(&mut self, session_id: SessionId) -> ServerResult<()> {
        let session = match self.sessions.get(session_id) {
            Some(s) => s,
            None => return Ok(()),
        };

        // Get all pane IDs and their PTY fds
        let pane_info: Vec<(PaneId, i32)> = session
            .window_manager
            .all_panes()
            .iter()
            .map(|pane| (pane.id, pane.pty.as_raw_fd()))
            .collect();

        for (pane_id, fd) in pane_info {
            let token = Token(PTY_TOKEN_BASE + session_id.0 as usize * 1000 + pane_id.0 as usize);

            // Register the PTY fd with mio
            self.poll
                .registry()
                .register(&mut SourceFd(&fd), token, Interest::READABLE)?;

            self.token_to_pty.insert(token, (session_id, pane_id));
            log::debug!(
                "Registered PTY for session {:?} pane {:?} with token {:?}",
                session_id,
                pane_id,
                token
            );
        }

        Ok(())
    }

    /// Register a single new pane's PTY with mio.
    fn register_new_pane_pty(
        &mut self,
        session_id: SessionId,
        pane_id: PaneId,
    ) -> ServerResult<()> {
        let session = match self.sessions.get(session_id) {
            Some(s) => s,
            None => return Ok(()),
        };

        // Find the pane and get its PTY fd
        let fd = session
            .window_manager
            .all_panes()
            .iter()
            .find(|p| p.id == pane_id)
            .map(|p| p.pty.as_raw_fd());

        if let Some(fd) = fd {
            let token = Token(PTY_TOKEN_BASE + session_id.0 as usize * 1000 + pane_id.0 as usize);

            self.poll
                .registry()
                .register(&mut SourceFd(&fd), token, Interest::READABLE)?;

            self.token_to_pty.insert(token, (session_id, pane_id));

            log::debug!(
                "Registered new PTY for session {:?} pane {:?} with token {:?}",
                session_id,
                pane_id,
                token
            );
        }

        Ok(())
    }

    /// Deregister a pane's PTY from mio.
    fn deregister_pane_pty(&mut self, session_id: SessionId, pane_id: PaneId) {
        let token = Token(PTY_TOKEN_BASE + session_id.0 as usize * 1000 + pane_id.0 as usize);

        // Remove from tracking
        self.token_to_pty.remove(&token);

        // Note: mio deregistration happens automatically when the fd is closed
        log::debug!(
            "Deregistered PTY for session {:?} pane {:?}",
            session_id,
            pane_id
        );
    }

    /// Broadcast a message to all clients attached to a session.
    fn broadcast_to_session(
        &mut self,
        session_id: SessionId,
        message: &ServerMessage,
    ) -> ServerResult<()> {
        // Collect client IDs attached to this session
        let client_ids: Vec<ClientId> = self
            .clients
            .iter()
            .filter_map(|(&id, client)| {
                if let ClientState::Attached(sid) = client.state {
                    if sid == session_id {
                        return Some(id);
                    }
                }
                None
            })
            .collect();

        // Send to each client
        for client_id in client_ids {
            self.send_to_client(client_id, message)?;
        }

        Ok(())
    }

    /// Broadcast a full screen update to all clients attached to a session.
    fn broadcast_full_screen(&mut self, session_id: SessionId) -> ServerResult<()> {
        let clients: Vec<ClientId> = self
            .clients
            .iter()
            .filter(|(_, client)| client.state == ClientState::Attached(session_id))
            .map(|(&id, _)| id)
            .collect();

        if clients.is_empty() {
            return Ok(());
        }

        if let Some(layout) = self.build_window_layout(session_id) {
            let layout_msg = ServerMessage::LayoutChanged { layout };
            for &client_id in &clients {
                self.send_to_client(client_id, &layout_msg)?;
            }

            self.send_all_pane_updates(session_id, &clients)?;
        }

        Ok(())
    }

    /// Build a WindowLayout from the current session state.
    fn build_window_layout(&self, session_id: SessionId) -> Option<WindowLayout> {
        let session = self.sessions.get(session_id)?;
        let wm = &session.window_manager;
        let active_window = wm.active_window();
        let focused_pane_id = wm.focused_pane_id();

        let panes: Vec<PaneLayout> = active_window
            .pane_manager
            .all_panes()
            .iter()
            .map(|pane| PaneLayout {
                pane_id: pane.id.0,
                x: pane.rect.x,
                y: pane.rect.y,
                width: pane.rect.width,
                height: pane.rect.height,
                focused: pane.id == focused_pane_id,
            })
            .collect();

        Some(WindowLayout {
            panes,
            screen_cols: wm.cols(),
            screen_rows: wm.rows(),
        })
    }

    /// Send full pane content updates to the given clients.
    fn send_all_pane_updates(
        &mut self,
        session_id: SessionId,
        client_ids: &[ClientId],
    ) -> ServerResult<()> {
        let detect_plain_urls = self.config.detect_plain_urls;

        // Collect all pane updates first while holding immutable session borrow
        let updates: Vec<ServerMessage> = {
            let session = match self.sessions.get(session_id) {
                Some(s) => s,
                None => return Ok(()),
            };

            let wm = &session.window_manager;
            let focused_pane_id = wm.focused_pane_id();
            let active_window = wm.active_window();

            active_window
                .pane_manager
                .all_panes()
                .iter()
                .map(|pane| {
                    let term_rows = pane.terminal.rows();
                    let rows_to_send = std::cmp::min(pane.rect.height as usize, term_rows);
                    let all_rows: Vec<u16> = (0..rows_to_send as u16).collect();

                    let mut links =
                        pane.terminal
                            .resolve_links(pane.id.0, detect_plain_urls, &all_rows);

                    let changed_rows: Vec<PaneRow> = all_rows
                        .iter()
                        .map(|&row_idx| {
                            let view = pane.terminal.view_row(row_idx);
                            let row_links = links
                                .remove(&row_idx)
                                .unwrap_or_default()
                                .into_iter()
                                .map(Into::into)
                                .collect();
                            PaneRow::with_links(row_idx, view.cells, row_links)
                                .wrapped(view.wrapped)
                        })
                        .collect();

                    // Only include cursor for focused pane
                    let cursor = if pane.id == focused_pane_id {
                        let c = pane.terminal.cursor();
                        Some(CursorState {
                            row: c.row as u16,
                            col: c.col as u16,
                            visible: c.visible,
                        })
                    } else {
                        None
                    };

                    ServerMessage::PaneUpdate {
                        pane_id: pane.id.0,
                        changed_rows,
                        cursor,
                    }
                })
                .collect()
        }; // session borrow ends here

        // Now send all updates to all clients
        for update in &updates {
            for &client_id in client_ids {
                self.send_to_client(client_id, update)?;
            }
        }

        Ok(())
    }

    /// Send a message to a specific client.
    fn send_to_client(&mut self, client_id: ClientId, message: &ServerMessage) -> ServerResult<()> {
        log::debug!(
            "send_to_client: {:?} -> {}",
            client_id,
            message_type(message)
        );
        if let Some(client) = self.clients.get_mut(&client_id) {
            if let Err(e) = client.send_message(message) {
                log::warn!("Failed to send to client {:?}: {}", client_id, e);
                // Don't remove client here - let cleanup_dead_clients handle it
            } else {
                log::trace!("Message sent successfully to {:?}", client_id);
            }
        } else {
            log::warn!("send_to_client: client {:?} not found", client_id);
        }
        Ok(())
    }

    /// Remove a client and clean up.
    fn remove_client(&mut self, client_id: ClientId) {
        // Detach from any session
        if let Some(client) = self.clients.get(&client_id) {
            if let ClientState::Attached(session_id) = client.state {
                if let Some(session) = self.sessions.get_mut(session_id) {
                    session.detach_client(client_id);
                }
            }
        }

        // Remove from token mapping
        let token = Token(CLIENT_TOKEN_BASE + client_id.0 as usize);
        self.token_to_client.remove(&token);

        // Remove client size
        self.client_sizes.remove(&client_id);

        // Remove the client (socket is closed on drop)
        self.clients.remove(&client_id);

        log::info!("Removed client {:?}", client_id);
    }

    /// Clean up dead/disconnected clients.
    fn cleanup_dead_clients(&mut self) {
        let dead_clients: Vec<ClientId> = self
            .clients
            .iter()
            .filter(|(_, client)| !client.is_alive())
            .map(|(&id, _)| id)
            .collect();

        for client_id in dead_clients {
            self.remove_client(client_id);
        }
    }

    /// Signal the server to stop.
    pub fn stop(&mut self) {
        self.running = false;
    }

    /// Check if the server is still running.
    /// Returns false if the server stopped itself (e.g., via auto-shutdown).
    pub fn is_running(&self) -> bool {
        self.running
    }

    /// Get the socket path.
    pub fn socket_path(&self) -> &PathBuf {
        &self.config.socket_path
    }

    /// Get the number of connected clients.
    pub fn client_count(&self) -> usize {
        self.clients.len()
    }

    /// Get the number of sessions.
    pub fn session_count(&self) -> usize {
        self.sessions.count()
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        // Notify all clients of shutdown
        for client_id in self.clients.keys().copied().collect::<Vec<_>>() {
            let _ = self.send_to_client(client_id, &ServerMessage::Shutdown);
        }

        // Socket file is cleaned up by SocketListener drop
    }
}

/// The rows a pane update has to carry: the dirty rows plus any extra row a
/// resolved link reaches onto.
fn rows_with_links(
    dirty_rows: &[u16],
    links: &std::collections::HashMap<u16, Vec<crate::urls::LinkRun>>,
) -> Vec<u16> {
    let mut rows: Vec<u16> = dirty_rows.to_vec();
    for &row in links.keys() {
        if !rows.contains(&row) {
            rows.push(row);
        }
    }
    rows.sort_unstable();
    rows.dedup();
    rows
}

/// Helper to get a short description of a server message for logging.
fn message_type(msg: &ServerMessage) -> &'static str {
    match msg {
        ServerMessage::HelloAck { .. } => "HelloAck",
        ServerMessage::Attached { .. } => "Attached",
        ServerMessage::Detached { .. } => "Detached",
        ServerMessage::SessionList(_) => "SessionList",
        ServerMessage::Error { .. } => "Error",
        ServerMessage::Pong => "Pong",
        ServerMessage::Shutdown => "Shutdown",
        ServerMessage::MouseMode { .. } => "MouseMode",
        ServerMessage::LayoutChanged { .. } => "LayoutChanged",
        ServerMessage::PaneUpdate { .. } => "PaneUpdate",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::{read_message, write_message};
    use std::os::unix::net::UnixStream;
    use std::thread;
    use std::time::Duration;

    fn temp_socket_path() -> PathBuf {
        let uid = unsafe { libc::getuid() };
        let id = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        PathBuf::from(format!("/tmp/clux-test-{}-{}.sock", uid, id))
    }

    #[test]
    fn test_server_config_default() {
        let config = ServerConfig::default();
        assert!(!config.shell.is_empty());
        assert!(config.socket_path.to_string_lossy().contains("clux"));
    }

    #[test]
    fn test_default_socket_path() {
        let path = default_socket_path();
        assert!(path.to_string_lossy().contains("clux"));
        assert!(path.to_string_lossy().contains(".sock"));
    }

    #[test]
    fn test_server_creation() {
        let socket_path = temp_socket_path();
        let config = ServerConfig {
            socket_path: socket_path.clone(),
            ..Default::default()
        };

        let server = Server::new(config);
        assert!(server.is_ok());

        let server = server.unwrap();
        assert_eq!(server.client_count(), 0);
        assert_eq!(server.session_count(), 0);

        // Clean up
        let _ = std::fs::remove_file(&socket_path);
    }

    #[test]
    fn test_server_accepts_connection() {
        let socket_path = temp_socket_path();
        let config = ServerConfig {
            socket_path: socket_path.clone(),
            ..Default::default()
        };

        let mut server = Server::new(config).unwrap();

        // Connect a client
        let _client = UnixStream::connect(&socket_path).unwrap();

        // Process the accept
        server.accept_client().unwrap();

        assert_eq!(server.client_count(), 1);

        // Clean up
        let _ = std::fs::remove_file(&socket_path);
    }

    #[test]
    fn test_client_hello_handshake() {
        let socket_path = temp_socket_path();
        let config = ServerConfig {
            socket_path: socket_path.clone(),
            ..Default::default()
        };

        let mut server = Server::new(config).unwrap();

        // Connect a client
        let mut client_stream = UnixStream::connect(&socket_path).unwrap();
        client_stream.set_nonblocking(false).unwrap();

        // Accept the connection
        server.accept_client().unwrap();
        assert_eq!(server.client_count(), 1);

        // Send Hello
        let hello = ClientMessage::Hello {
            version: PROTOCOL_VERSION,
            term_cols: 80,
            term_rows: 24,
            term_type: "xterm-256color".to_string(),
        };
        write_message(&mut client_stream, &hello).unwrap();

        // Process the message
        let client_id = ClientId(0);

        // Need to wait a bit for data to arrive
        thread::sleep(Duration::from_millis(10));

        server.handle_client_event(client_id).unwrap();

        // Read the response
        let response: ServerMessage = read_message(&mut client_stream).unwrap();

        match response {
            ServerMessage::HelloAck {
                version,
                server_pid,
            } => {
                assert_eq!(version, PROTOCOL_VERSION);
                assert!(server_pid > 0);
            }
            _ => panic!("Expected HelloAck, got {:?}", response),
        }

        // Clean up
        let _ = std::fs::remove_file(&socket_path);
    }

    #[test]
    fn test_client_list_sessions() {
        let socket_path = temp_socket_path();
        let config = ServerConfig {
            socket_path: socket_path.clone(),
            ..Default::default()
        };

        let mut server = Server::new(config).unwrap();

        // Create a session
        server
            .sessions
            .create_session(Some("test-session".to_string()), 80, 24)
            .unwrap();

        // Connect a client
        let mut client_stream = UnixStream::connect(&socket_path).unwrap();
        client_stream.set_nonblocking(false).unwrap();

        // Accept and do handshake
        server.accept_client().unwrap();
        let hello = ClientMessage::Hello {
            version: PROTOCOL_VERSION,
            term_cols: 80,
            term_rows: 24,
            term_type: "xterm".to_string(),
        };
        write_message(&mut client_stream, &hello).unwrap();
        thread::sleep(Duration::from_millis(10));
        server.handle_client_event(ClientId(0)).unwrap();
        let _: ServerMessage = read_message(&mut client_stream).unwrap();

        // Request session list
        write_message(&mut client_stream, &ClientMessage::ListSessions).unwrap();
        thread::sleep(Duration::from_millis(10));
        server.handle_client_event(ClientId(0)).unwrap();

        let response: ServerMessage = read_message(&mut client_stream).unwrap();
        match response {
            ServerMessage::SessionList(sessions) => {
                assert_eq!(sessions.len(), 1);
                assert_eq!(sessions[0].name, "test-session");
            }
            _ => panic!("Expected SessionList, got {:?}", response),
        }

        // Clean up
        let _ = std::fs::remove_file(&socket_path);
    }
}
