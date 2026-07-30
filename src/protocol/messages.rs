//! Messages exchanged between client and server.

use serde::{Deserialize, Serialize};

use super::{CursorState, PaneRow, SessionInfo, WindowLayout};

/// Messages sent from client to server.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ClientMessage {
    /// Initial handshake with client information.
    Hello {
        /// Protocol version for compatibility checking.
        version: u32,
        /// Terminal width in columns.
        term_cols: u16,
        /// Terminal height in rows.
        term_rows: u16,
        /// Terminal type ($TERM environment variable).
        term_type: String,
    },

    /// Attach to a session.
    Attach {
        /// Session name (None for default session).
        session_name: Option<String>,
        /// Create the session if it doesn't exist.
        create: bool,
    },

    /// Detach from the current session.
    Detach,

    /// Keyboard/mouse input bytes to forward to the PTY.
    Input(Vec<u8>),

    /// Terminal resize notification.
    Resize {
        /// New terminal width in columns.
        cols: u16,
        /// New terminal height in rows.
        rows: u16,
    },

    /// Execute a command-mode action.
    Command(CommandAction),

    /// List all sessions.
    ListSessions,

    /// Kill a specific session.
    KillSession {
        /// Name of the session to kill.
        name: String,
    },

    /// Rename the current session.
    RenameSession {
        /// New name for the session.
        new_name: String,
    },

    /// Scroll the focused pane through its scrollback.
    Scroll {
        /// Lines to move: positive goes back in history, negative comes forward.
        /// Zero returns to the live view.
        lines: i32,
    },

    /// Heartbeat to check connection health.
    Ping,

    /// Request that the server shut down cleanly.
    ShutdownServer,
}

/// Command-mode actions that can be sent from client to server.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum CommandAction {
    /// Split the current pane horizontally (new pane below).
    SplitHorizontal,
    /// Split the current pane vertically (new pane to the right).
    SplitVertical,
    /// Close the focused pane.
    ClosePane,
    /// Navigate to an adjacent pane.
    NavigatePane(Direction),
    /// Create a new window.
    NewWindow,
    /// Close the current window.
    CloseWindow,
    /// Switch to the next window.
    NextWindow,
    /// Switch to the previous window.
    PrevWindow,
    /// Select a window by index (0-9).
    SelectWindow(usize),
    /// Quit the session entirely.
    Quit,
}

/// Direction for pane navigation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Direction {
    Up,
    Down,
    Left,
    Right,
}

// ============================================================================
// Server to Client Messages
// ============================================================================

/// Messages sent from server to client.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ServerMessage {
    /// Response to Hello handshake.
    HelloAck {
        /// Server's protocol version.
        version: u32,
        /// Server process ID.
        server_pid: u32,
    },

    /// Successfully attached to a session.
    Attached {
        /// Session identifier.
        session_id: u32,
        /// Session name.
        session_name: String,
    },

    /// Detached from session.
    Detached {
        /// Reason for detachment.
        reason: DetachReason,
    },

    /// List of all sessions.
    SessionList(Vec<SessionInfo>),

    /// Error response.
    Error {
        /// Error message.
        message: String,
    },

    /// Response to Ping.
    Pong,

    /// Server is shutting down.
    Shutdown,

    /// Mouse mode changed in the focused pane.
    MouseMode {
        /// Whether the focused pane wants mouse events.
        enabled: bool,
    },

    /// Layout changed (panes split/closed/resized).
    LayoutChanged {
        /// Complete layout of the current window.
        layout: WindowLayout,
    },

    /// Pane-local content update.
    /// Contains pane-local coordinates, client composites into screen buffer.
    PaneUpdate {
        /// ID of the pane that was updated.
        pane_id: u32,
        /// Changed rows in pane-local coordinates.
        changed_rows: Vec<PaneRow>,
        /// Cursor state in pane-local coordinates.
        cursor: Option<CursorState>,
    },
}

/// Reason for session detachment.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum DetachReason {
    /// Client requested detachment.
    ClientRequested,
    /// Session was closed.
    SessionClosed,
    /// Server is shutting down.
    ServerShutdown,
    /// Another client replaced this one.
    Replaced,
}
