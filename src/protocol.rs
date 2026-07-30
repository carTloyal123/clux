//! Client-server protocol for clux.
//!
//! This module defines the message types exchanged between clux clients and the
//! server over Unix domain sockets. Messages are serialized using bincode with
//! length-prefixed framing.
//!
//! Wire format:
//! ```text
//! ┌─────────────────────────────────────────┐
//! │  Length (4 bytes, little-endian u32)    │
//! ├─────────────────────────────────────────┤
//! │  Payload (bincode-serialized message)   │
//! └─────────────────────────────────────────┘
//! ```

use serde::{Deserialize, Serialize};
use std::io::{self, Read, Write};

use crate::cell::Cell;

/// Protocol version for compatibility checking.
/// Version 2 adds PaneUpdate/LayoutChanged messages for hybrid rendering.
/// Version 3 adds ShutdownServer for clean server termination.
/// Version 4 adds hyperlink runs to PaneRow (bincode is not self-describing, so
/// adding the field is a wire break).
/// Version 5 drops the pre-PaneUpdate rendering path: no more FullScreen/Update
/// messages, no capability negotiation - every client composites pane updates.
/// Version 6 adds the soft-wrap flag to PaneRow, so the client can join wrapped
/// rows when extracting selected text.
/// Version 7 drops speculative fields nothing implemented: cursor shape (always
/// Block, never parsed from DECSCUSR) and Attached::needs_full_redraw (always
/// true, ignored by the client).
/// Version 8 adds Scroll for scrollback navigation.
pub const PROTOCOL_VERSION: u32 = 8;

/// Maximum message size (16 MB) to prevent memory exhaustion.
pub const MAX_MESSAGE_SIZE: u32 = 16 * 1024 * 1024;

// ============================================================================
// Client to Server Messages
// ============================================================================

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

/// Cursor state for rendering.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CursorState {
    /// Row position (0-indexed).
    pub row: u16,
    /// Column position (0-indexed).
    pub col: u16,
    /// Whether the cursor is visible.
    pub visible: bool,
}

impl Default for CursorState {
    fn default() -> Self {
        Self {
            row: 0,
            col: 0,
            visible: true,
        }
    }
}

/// Information about a session for listing.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SessionInfo {
    /// Session identifier.
    pub id: u32,
    /// Session name.
    pub name: String,
    /// Unix timestamp of session creation.
    pub created_at: u64,
    /// Number of windows in the session.
    pub windows: usize,
    /// Number of attached clients.
    pub attached_clients: usize,
}

// ============================================================================
// Pane Rendering Types (hybrid client-server rendering)
// ============================================================================

/// Layout information for a single pane.
/// Used by clients to composite pane content into the screen buffer.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PaneLayout {
    /// Unique identifier for this pane.
    pub pane_id: u32,
    /// X position (column) of the pane's top-left corner in screen coordinates.
    pub x: u16,
    /// Y position (row) of the pane's top-left corner in screen coordinates.
    pub y: u16,
    /// Width of the pane in columns.
    pub width: u16,
    /// Height of the pane in rows.
    pub height: u16,
    /// Whether this pane currently has focus.
    pub focused: bool,
}

/// Layout of all panes in the active window.
/// Sent when layout changes (split, close, resize).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WindowLayout {
    /// Layout information for each pane.
    pub panes: Vec<PaneLayout>,
    /// Total screen width in columns.
    pub screen_cols: u16,
    /// Total screen height in rows (excluding status line).
    pub screen_rows: u16,
}

/// A single row of pane content with styled cells.
/// Row index is relative to the pane's top-left corner.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PaneRow {
    /// Row index within the pane (0 = top row of pane).
    pub row_idx: u16,
    /// Styled cells for this row.
    pub cells: Vec<Cell>,
    /// Hyperlink runs covering cells in this row, in column order.
    ///
    /// Resolved server-side (explicit OSC 8 plus detected URLs) because only the
    /// server knows where a pane's logical lines end. Carried per row rather than
    /// as an id table so a client never has to hold state that can go stale.
    pub links: Vec<RowLink>,
    /// Whether this row's content continues onto the next row (soft wrap).
    ///
    /// The client needs this to extract selected text: a wrapped row must be
    /// joined to the next one, or copying a long path or URL inserts a newline
    /// in the middle of it. Only the server knows where a logical line ends.
    pub wrapped: bool,
}

impl PaneRow {
    /// Create a new pane row with no hyperlinks.
    pub fn new(row_idx: u16, cells: Vec<Cell>) -> Self {
        Self {
            row_idx,
            cells,
            links: Vec::new(),
            wrapped: false,
        }
    }

    /// Create a new pane row with hyperlink runs.
    pub fn with_links(row_idx: u16, cells: Vec<Cell>, links: Vec<RowLink>) -> Self {
        Self {
            row_idx,
            cells,
            links,
            wrapped: false,
        }
    }

    /// Mark whether this row soft-wraps onto the next one.
    pub fn wrapped(mut self, wrapped: bool) -> Self {
        self.wrapped = wrapped;
        self
    }
}

/// A hyperlink run within a single row of a pane.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RowLink {
    /// First column of the run (pane-local).
    pub start_col: u16,
    /// One past the last column of the run (pane-local).
    pub end_col: u16,
    /// OSC 8 id shared by every run of the same logical link, so the outer
    /// terminal treats fragments split across rows as one link.
    pub id: u32,
    /// Target URL.
    pub url: String,
    /// Whether clux found this link itself rather than the application asking
    /// for it. Detected links get an underline so they read as links; an
    /// application's own OSC 8 links keep exactly the styling it printed.
    pub detected: bool,
}

impl From<crate::urls::LinkRun> for RowLink {
    fn from(run: crate::urls::LinkRun) -> Self {
        Self {
            start_col: run.start_col,
            end_col: run.end_col,
            id: run.id,
            url: run.url,
            detected: run.detected,
        }
    }
}

// ============================================================================
// Wire Protocol Helpers
// ============================================================================

/// Error type for protocol operations.
#[derive(Debug, thiserror::Error)]
pub enum ProtocolError {
    #[error("IO error: {0}")]
    Io(#[from] io::Error),

    #[error("Serialization error: {0}")]
    Serialization(#[from] bincode::Error),

    #[error("Message too large: {size} bytes (max {MAX_MESSAGE_SIZE})")]
    MessageTooLarge { size: u32 },

    #[error("Connection closed")]
    ConnectionClosed,

    #[error("Protocol version mismatch: client={client}, server={server}")]
    VersionMismatch { client: u32, server: u32 },
}

/// Result type for protocol operations.
pub type ProtocolResult<T> = Result<T, ProtocolError>;

/// Write a message to a writer with length-prefixed framing.
pub fn write_message<W: Write, M: Serialize>(writer: &mut W, message: &M) -> ProtocolResult<()> {
    let payload = bincode::serialize(message)?;
    let len = payload.len() as u32;

    if len > MAX_MESSAGE_SIZE {
        return Err(ProtocolError::MessageTooLarge { size: len });
    }

    writer.write_all(&len.to_le_bytes())?;
    writer.write_all(&payload)?;
    writer.flush()?;

    Ok(())
}

/// Read a message from a reader with length-prefixed framing.
pub fn read_message<R: Read, M: for<'de> Deserialize<'de>>(reader: &mut R) -> ProtocolResult<M> {
    let mut len_buf = [0u8; 4];

    match reader.read_exact(&mut len_buf) {
        Ok(()) => {}
        Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => {
            return Err(ProtocolError::ConnectionClosed);
        }
        Err(e) => return Err(ProtocolError::Io(e)),
    }

    let len = u32::from_le_bytes(len_buf);

    if len > MAX_MESSAGE_SIZE {
        return Err(ProtocolError::MessageTooLarge { size: len });
    }

    let mut payload = vec![0u8; len as usize];
    match reader.read_exact(&mut payload) {
        Ok(()) => {}
        Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => {
            return Err(ProtocolError::ConnectionClosed);
        }
        Err(e) => return Err(ProtocolError::Io(e)),
    }

    let message = bincode::deserialize(&payload)?;
    Ok(message)
}

/// Non-blocking message reader that handles partial reads.
/// Returns None if not enough data is available yet.
pub struct MessageReader {
    /// Buffer for accumulating data.
    buffer: Vec<u8>,
    /// Expected message length (None if reading length prefix).
    expected_len: Option<u32>,
}

impl MessageReader {
    /// Create a new message reader.
    pub fn new() -> Self {
        Self {
            buffer: Vec::new(),
            expected_len: None,
        }
    }

    /// Feed bytes into the reader and try to parse a complete message.
    /// Returns Ok(Some(message)) if a complete message was parsed,
    /// Ok(None) if more data is needed, or Err on protocol errors.
    pub fn feed<M: for<'de> Deserialize<'de>>(&mut self, data: &[u8]) -> ProtocolResult<Option<M>> {
        self.buffer.extend_from_slice(data);

        // Try to read length prefix if we don't have it yet
        if self.expected_len.is_none() && self.buffer.len() >= 4 {
            let mut len_bytes = [0u8; 4];
            len_bytes.copy_from_slice(&self.buffer[..4]);
            let len = u32::from_le_bytes(len_bytes);

            if len > MAX_MESSAGE_SIZE {
                return Err(ProtocolError::MessageTooLarge { size: len });
            }

            self.expected_len = Some(len);
        }

        // Try to read the message payload
        if let Some(len) = self.expected_len {
            let total_needed = 4 + len as usize;

            if self.buffer.len() >= total_needed {
                // We have a complete message
                let payload = &self.buffer[4..total_needed];
                let message = bincode::deserialize(payload);

                // Consume the complete frame regardless of decode success so callers
                // can continue reading subsequent frames from the stream.
                self.buffer.drain(..total_needed);
                self.expected_len = None;

                return match message {
                    Ok(message) => Ok(Some(message)),
                    Err(e) => Err(ProtocolError::Serialization(e)),
                };
            }
        }

        // Need more data
        Ok(None)
    }

    /// Check if the buffer is empty.
    pub fn is_empty(&self) -> bool {
        self.buffer.is_empty()
    }

    /// Get the number of bytes currently buffered.
    pub fn buffered_len(&self) -> usize {
        self.buffer.len()
    }
}

impl Default for MessageReader {
    fn default() -> Self {
        Self::new()
    }
}

/// Non-blocking message writer that handles partial writes.
pub struct MessageWriter {
    /// Buffer for pending data.
    buffer: Vec<u8>,
    /// Number of bytes already written from the buffer.
    written: usize,
}

impl MessageWriter {
    /// Create a new message writer.
    pub fn new() -> Self {
        Self {
            buffer: Vec::new(),
            written: 0,
        }
    }

    /// Queue a message for writing.
    pub fn queue<M: Serialize>(&mut self, message: &M) -> ProtocolResult<()> {
        let payload = bincode::serialize(message)?;
        let len = payload.len() as u32;

        if len > MAX_MESSAGE_SIZE {
            return Err(ProtocolError::MessageTooLarge { size: len });
        }

        self.buffer.extend_from_slice(&len.to_le_bytes());
        self.buffer.extend_from_slice(&payload);

        Ok(())
    }

    /// Try to write pending data to the writer.
    /// Returns Ok(true) if all data was written, Ok(false) if more writes needed.
    pub fn flush<W: Write>(&mut self, writer: &mut W) -> ProtocolResult<bool> {
        while self.written < self.buffer.len() {
            match writer.write(&self.buffer[self.written..]) {
                Ok(0) => {
                    // Zero write means the stream is closed.
                    return Err(ProtocolError::ConnectionClosed);
                }
                Ok(n) => {
                    self.written += n;
                }
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                    return Ok(false);
                }
                Err(e) => return Err(ProtocolError::Io(e)),
            }
        }

        // All data written, clear buffer
        self.buffer.clear();
        self.written = 0;

        Ok(true)
    }

    /// Check if there's pending data to write.
    pub fn has_pending(&self) -> bool {
        self.written < self.buffer.len()
    }

    /// Get the number of bytes pending to be written.
    pub fn pending_len(&self) -> usize {
        self.buffer.len() - self.written
    }
}

impl Default for MessageWriter {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{self, Cursor, Write};

    // ------------------------------------------------------------------------
    // Serialization Round-Trip Tests
    // ------------------------------------------------------------------------

    #[test]
    fn test_client_message_hello_roundtrip() {
        let msg = ClientMessage::Hello {
            version: PROTOCOL_VERSION,
            term_cols: 80,
            term_rows: 24,
            term_type: "xterm-256color".to_string(),
        };

        let serialized = bincode::serialize(&msg).unwrap();
        let deserialized: ClientMessage = bincode::deserialize(&serialized).unwrap();

        assert_eq!(msg, deserialized);
    }

    #[test]
    fn test_client_message_attach_roundtrip() {
        let msg = ClientMessage::Attach {
            session_name: Some("work".to_string()),
            create: true,
        };

        let serialized = bincode::serialize(&msg).unwrap();
        let deserialized: ClientMessage = bincode::deserialize(&serialized).unwrap();

        assert_eq!(msg, deserialized);
    }

    #[test]
    fn test_client_message_input_roundtrip() {
        let msg = ClientMessage::Input(vec![0x1b, b'[', b'A']); // Up arrow

        let serialized = bincode::serialize(&msg).unwrap();
        let deserialized: ClientMessage = bincode::deserialize(&serialized).unwrap();

        assert_eq!(msg, deserialized);
    }

    #[test]
    fn test_client_message_command_roundtrip() {
        let commands = vec![
            CommandAction::SplitHorizontal,
            CommandAction::SplitVertical,
            CommandAction::ClosePane,
            CommandAction::NavigatePane(Direction::Up),
            CommandAction::NavigatePane(Direction::Down),
            CommandAction::NavigatePane(Direction::Left),
            CommandAction::NavigatePane(Direction::Right),
            CommandAction::NewWindow,
            CommandAction::CloseWindow,
            CommandAction::NextWindow,
            CommandAction::PrevWindow,
            CommandAction::SelectWindow(5),
            CommandAction::Quit,
        ];

        for cmd in commands {
            let msg = ClientMessage::Command(cmd.clone());
            let serialized = bincode::serialize(&msg).unwrap();
            let deserialized: ClientMessage = bincode::deserialize(&serialized).unwrap();
            assert_eq!(msg, deserialized);
        }
    }

    #[test]
    fn test_client_message_shutdown_server_roundtrip() {
        let msg = ClientMessage::ShutdownServer;

        let serialized = bincode::serialize(&msg).unwrap();
        let deserialized: ClientMessage = bincode::deserialize(&serialized).unwrap();

        assert_eq!(msg, deserialized);
    }

    #[test]
    fn test_server_message_hello_ack_roundtrip() {
        let msg = ServerMessage::HelloAck {
            version: PROTOCOL_VERSION,
            server_pid: 12345,
        };

        let serialized = bincode::serialize(&msg).unwrap();
        let deserialized: ServerMessage = bincode::deserialize(&serialized).unwrap();

        assert_eq!(msg, deserialized);
    }

    #[test]
    fn test_server_message_attached_roundtrip() {
        let msg = ServerMessage::Attached {
            session_id: 1,
            session_name: "default".to_string(),
        };

        let serialized = bincode::serialize(&msg).unwrap();
        let deserialized: ServerMessage = bincode::deserialize(&serialized).unwrap();

        assert_eq!(msg, deserialized);
    }

    #[test]
    fn test_server_message_session_list_roundtrip() {
        let msg = ServerMessage::SessionList(vec![
            SessionInfo {
                id: 1,
                name: "default".to_string(),
                created_at: 1700000000,
                windows: 2,
                attached_clients: 1,
            },
            SessionInfo {
                id: 2,
                name: "work".to_string(),
                created_at: 1700001000,
                windows: 3,
                attached_clients: 0,
            },
        ]);

        let serialized = bincode::serialize(&msg).unwrap();
        let deserialized: ServerMessage = bincode::deserialize(&serialized).unwrap();

        assert_eq!(msg, deserialized);
    }

    #[test]
    fn test_detach_reasons_roundtrip() {
        let reasons = vec![
            DetachReason::ClientRequested,
            DetachReason::SessionClosed,
            DetachReason::ServerShutdown,
            DetachReason::Replaced,
        ];

        for reason in reasons {
            let msg = ServerMessage::Detached {
                reason: reason.clone(),
            };
            let serialized = bincode::serialize(&msg).unwrap();
            let deserialized: ServerMessage = bincode::deserialize(&serialized).unwrap();
            assert_eq!(msg, deserialized);
        }
    }

    // ------------------------------------------------------------------------
    // Wire Protocol Tests
    // ------------------------------------------------------------------------

    #[test]
    fn test_write_read_message() {
        let mut buffer = Vec::new();

        let msg = ClientMessage::Hello {
            version: PROTOCOL_VERSION,
            term_cols: 120,
            term_rows: 40,
            term_type: "screen-256color".to_string(),
        };

        write_message(&mut buffer, &msg).unwrap();

        let mut cursor = Cursor::new(buffer);
        let received: ClientMessage = read_message(&mut cursor).unwrap();

        assert_eq!(msg, received);
    }

    #[test]
    fn test_write_read_multiple_messages() {
        let mut buffer = Vec::new();

        let messages = vec![
            ClientMessage::Ping,
            ClientMessage::ShutdownServer,
            ClientMessage::Input(vec![b'a', b'b', b'c']),
            ClientMessage::Resize {
                cols: 100,
                rows: 50,
            },
            ClientMessage::Detach,
        ];

        for msg in &messages {
            write_message(&mut buffer, msg).unwrap();
        }

        let mut cursor = Cursor::new(buffer);

        for expected in &messages {
            let received: ClientMessage = read_message(&mut cursor).unwrap();
            assert_eq!(expected, &received);
        }
    }

    #[test]
    fn test_message_reader_complete_message() {
        let mut writer = Vec::new();
        let msg = ClientMessage::Ping;
        write_message(&mut writer, &msg).unwrap();

        let mut reader = MessageReader::new();
        let result: Option<ClientMessage> = reader.feed(&writer).unwrap();

        assert_eq!(result, Some(msg));
        assert!(reader.is_empty());
    }

    #[test]
    fn test_message_reader_partial_length() {
        let mut writer = Vec::new();
        let msg = ClientMessage::Ping;
        write_message(&mut writer, &msg).unwrap();

        let mut reader = MessageReader::new();

        // Feed only 2 bytes (partial length)
        let result: Option<ClientMessage> = reader.feed(&writer[..2]).unwrap();
        assert_eq!(result, None);
        assert_eq!(reader.buffered_len(), 2);

        // Feed the rest
        let result: Option<ClientMessage> = reader.feed(&writer[2..]).unwrap();
        assert_eq!(result, Some(msg));
        assert!(reader.is_empty());
    }

    #[test]
    fn test_message_reader_partial_payload() {
        let mut writer = Vec::new();
        let msg = ClientMessage::Input(vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 10]);
        write_message(&mut writer, &msg).unwrap();

        let mut reader = MessageReader::new();

        // Feed length + partial payload
        let result: Option<ClientMessage> = reader.feed(&writer[..6]).unwrap();
        assert_eq!(result, None);

        // Feed more
        let result: Option<ClientMessage> = reader.feed(&writer[6..10]).unwrap();
        assert_eq!(result, None);

        // Feed the rest
        let result: Option<ClientMessage> = reader.feed(&writer[10..]).unwrap();
        assert_eq!(result, Some(msg));
    }

    #[test]
    fn test_message_reader_multiple_messages() {
        let mut writer = Vec::new();
        let msg1 = ClientMessage::Ping;
        let msg2 = ClientMessage::Detach;
        write_message(&mut writer, &msg1).unwrap();
        write_message(&mut writer, &msg2).unwrap();

        let mut reader = MessageReader::new();

        // Feed both messages at once
        let result1: Option<ClientMessage> = reader.feed(&writer).unwrap();
        assert_eq!(result1, Some(msg1));

        // Second message should still be in buffer
        let result2: Option<ClientMessage> = reader.feed(&[]).unwrap();
        assert_eq!(result2, Some(msg2));

        assert!(reader.is_empty());
    }

    #[test]
    fn test_message_writer_queue_and_flush() {
        let mut writer = MessageWriter::new();

        writer.queue(&ClientMessage::Ping).unwrap();
        writer.queue(&ClientMessage::Detach).unwrap();

        assert!(writer.has_pending());

        let mut output = Vec::new();
        let complete = writer.flush(&mut output).unwrap();

        assert!(complete);
        assert!(!writer.has_pending());

        // Verify we can read the messages back
        let mut cursor = Cursor::new(output);
        let msg1: ClientMessage = read_message(&mut cursor).unwrap();
        let msg2: ClientMessage = read_message(&mut cursor).unwrap();

        assert_eq!(msg1, ClientMessage::Ping);
        assert_eq!(msg2, ClientMessage::Detach);
    }

    #[test]
    fn test_message_reader_recovers_after_invalid_frame() {
        let mut bytes = Vec::new();

        // Invalid ClientMessage payload (length=1, payload cannot decode to valid enum)
        bytes.extend_from_slice(&1u32.to_le_bytes());
        bytes.push(0xff);

        // Followed by a valid frame
        write_message(&mut bytes, &ClientMessage::Ping).unwrap();

        let mut reader = MessageReader::new();
        let first: ProtocolResult<Option<ClientMessage>> = reader.feed(&bytes);
        assert!(matches!(first, Err(ProtocolError::Serialization(_))));

        // Reader should have consumed the invalid frame and be able to parse the next one
        let second: ProtocolResult<Option<ClientMessage>> = reader.feed(&[]);
        assert_eq!(second.unwrap(), Some(ClientMessage::Ping));
        assert!(reader.is_empty());
    }

    #[test]
    fn test_message_reader_oversized_length_prefix() {
        let mut reader = MessageReader::new();
        let len = MAX_MESSAGE_SIZE + 1;
        let result: ProtocolResult<Option<ClientMessage>> = reader.feed(&len.to_le_bytes());
        assert!(matches!(result, Err(ProtocolError::MessageTooLarge { .. })));
    }

    struct ZeroWriter;

    impl Write for ZeroWriter {
        fn write(&mut self, _buf: &[u8]) -> io::Result<usize> {
            Ok(0)
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn test_message_writer_zero_write_is_connection_closed() {
        let mut writer = MessageWriter::new();
        writer.queue(&ClientMessage::Ping).unwrap();

        let mut zero = ZeroWriter;
        let result = writer.flush(&mut zero);
        assert!(matches!(result, Err(ProtocolError::ConnectionClosed)));
        assert!(writer.has_pending());
    }

    #[test]
    fn test_message_too_large_error() {
        // Create a message that claims to be too large
        let fake_len: u32 = MAX_MESSAGE_SIZE + 1;
        let mut buffer = Vec::new();
        buffer.extend_from_slice(&fake_len.to_le_bytes());
        buffer.extend_from_slice(&[0u8; 10]); // Some payload

        let mut cursor = Cursor::new(buffer);
        let result: ProtocolResult<ClientMessage> = read_message(&mut cursor);

        assert!(matches!(result, Err(ProtocolError::MessageTooLarge { .. })));
    }

    #[test]
    fn test_connection_closed_error() {
        let buffer: Vec<u8> = Vec::new(); // Empty buffer
        let mut cursor = Cursor::new(buffer);

        let result: ProtocolResult<ClientMessage> = read_message(&mut cursor);

        assert!(matches!(result, Err(ProtocolError::ConnectionClosed)));
    }

    #[test]
    fn test_connection_closed_mid_payload_maps_to_connection_closed() {
        let len = 8u32;
        let mut buffer = Vec::new();
        buffer.extend_from_slice(&len.to_le_bytes());
        buffer.extend_from_slice(&[1, 2, 3]); // Truncated payload

        let mut cursor = Cursor::new(buffer);
        let result: ProtocolResult<ClientMessage> = read_message(&mut cursor);
        assert!(matches!(result, Err(ProtocolError::ConnectionClosed)));
    }

    // ------------------------------------------------------------------------
    // Serialization Size Tests (for performance awareness)
    // ------------------------------------------------------------------------

    #[test]
    fn test_serialization_sizes() {
        // These tests document expected sizes for performance monitoring

        let ping = ClientMessage::Ping;
        let ping_size = bincode::serialize(&ping).unwrap().len();
        assert!(
            ping_size < 10,
            "Ping should be tiny, got {} bytes",
            ping_size
        );

        let input = ClientMessage::Input(vec![b'a']);
        let input_size = bincode::serialize(&input).unwrap().len();
        assert!(
            input_size < 20,
            "Single char input should be small, got {} bytes",
            input_size
        );

        // A full 80x24 pane repaint
        let full_pane = ServerMessage::PaneUpdate {
            pane_id: 0,
            changed_rows: (0..24)
                .map(|row| PaneRow::new(row, vec![Cell::new('x'); 80]))
                .collect(),
            cursor: Some(CursorState::default()),
        };
        let full_pane_size = bincode::serialize(&full_pane).unwrap().len();
        assert!(
            full_pane_size < 64 * 1024,
            "Full pane repaint should be < 64KB, got {} bytes",
            full_pane_size
        );

        // Single row update should be much smaller
        let update = ServerMessage::PaneUpdate {
            pane_id: 0,
            changed_rows: vec![PaneRow::new(5, vec![Cell::new('x'); 80])],
            cursor: Some(CursorState::default()),
        };
        let update_size = bincode::serialize(&update).unwrap().len();
        assert!(
            update_size < 4096,
            "Single row update should be < 4KB, got {} bytes",
            update_size
        );

        // Hyperlinks add one run per row, not a URL per cell.
        let linked = ServerMessage::PaneUpdate {
            pane_id: 0,
            changed_rows: vec![PaneRow::with_links(
                5,
                vec![Cell::new('x'); 80],
                vec![RowLink {
                    start_col: 0,
                    end_col: 40,
                    id: 7,
                    url: "https://example.com/some/path".to_string(),
                    detected: true,
                }],
            )],
            cursor: None,
        };
        let linked_size = bincode::serialize(&linked).unwrap().len();
        assert!(
            linked_size < update_size + 128,
            "A link should cost ~its URL, got {} bytes over {}",
            linked_size - update_size,
            update_size
        );
    }

    // ------------------------------------------------------------------------
    // Edge Case Tests
    // ------------------------------------------------------------------------

    #[test]
    fn test_empty_session_name() {
        let msg = ClientMessage::Attach {
            session_name: Some("".to_string()),
            create: false,
        };

        let serialized = bincode::serialize(&msg).unwrap();
        let deserialized: ClientMessage = bincode::deserialize(&serialized).unwrap();

        assert_eq!(msg, deserialized);
    }

    #[test]
    fn test_unicode_in_pane_row() {
        let msg = ServerMessage::PaneUpdate {
            pane_id: 0,
            changed_rows: vec![PaneRow::new(
                0,
                "Hello 世界 🦀 émojis".chars().map(Cell::new).collect(),
            )],
            cursor: None,
        };

        let serialized = bincode::serialize(&msg).unwrap();
        let deserialized: ServerMessage = bincode::deserialize(&serialized).unwrap();

        assert_eq!(msg, deserialized);
    }

    #[test]
    fn test_large_input_buffer() {
        // Test with 64KB of input (e.g., large paste)
        let large_input = vec![b'x'; 64 * 1024];
        let msg = ClientMessage::Input(large_input.clone());

        let serialized = bincode::serialize(&msg).unwrap();
        let deserialized: ClientMessage = bincode::deserialize(&serialized).unwrap();

        assert_eq!(msg, deserialized);
    }

    // ------------------------------------------------------------------------
    // Pane Rendering Tests (hybrid client-server rendering)
    // ------------------------------------------------------------------------

    #[test]
    fn test_cell_serialization_roundtrip() {
        use crate::cell::{Cell, CellFlags, Color};

        let cell = Cell::styled(
            'A',
            Color::rgb(255, 0, 0),
            Color::indexed(4),
            CellFlags::BOLD | CellFlags::UNDERLINE,
        );

        let serialized = bincode::serialize(&cell).unwrap();
        let deserialized: Cell = bincode::deserialize(&serialized).unwrap();

        assert_eq!(cell, deserialized);
    }

    #[test]
    fn test_pane_layout_roundtrip() {
        let layout = PaneLayout {
            pane_id: 42,
            x: 10,
            y: 5,
            width: 80,
            height: 24,
            focused: true,
        };

        let serialized = bincode::serialize(&layout).unwrap();
        let deserialized: PaneLayout = bincode::deserialize(&serialized).unwrap();

        assert_eq!(layout, deserialized);
    }

    #[test]
    fn test_window_layout_roundtrip() {
        let layout = WindowLayout {
            panes: vec![
                PaneLayout {
                    pane_id: 0,
                    x: 0,
                    y: 0,
                    width: 40,
                    height: 24,
                    focused: true,
                },
                PaneLayout {
                    pane_id: 1,
                    x: 41,
                    y: 0,
                    width: 39,
                    height: 24,
                    focused: false,
                },
            ],
            screen_cols: 80,
            screen_rows: 24,
        };

        let serialized = bincode::serialize(&layout).unwrap();
        let deserialized: WindowLayout = bincode::deserialize(&serialized).unwrap();

        assert_eq!(layout, deserialized);
    }

    #[test]
    fn test_pane_row_roundtrip() {
        use crate::cell::{Cell, CellFlags, Color};

        let row = PaneRow::new(
            5,
            vec![
                Cell::styled(
                    'H',
                    Color::rgb(255, 0, 0),
                    Color::default_color(),
                    CellFlags::BOLD,
                ),
                Cell::styled(
                    'i',
                    Color::indexed(2),
                    Color::default_color(),
                    CellFlags::empty(),
                ),
                Cell::default(),
            ],
        );

        let serialized = bincode::serialize(&row).unwrap();
        let deserialized: PaneRow = bincode::deserialize(&serialized).unwrap();

        assert_eq!(row, deserialized);
    }

    #[test]
    fn test_layout_changed_message_roundtrip() {
        let msg = ServerMessage::LayoutChanged {
            layout: WindowLayout {
                panes: vec![PaneLayout {
                    pane_id: 0,
                    x: 0,
                    y: 0,
                    width: 80,
                    height: 24,
                    focused: true,
                }],
                screen_cols: 80,
                screen_rows: 24,
            },
        };

        let serialized = bincode::serialize(&msg).unwrap();
        let deserialized: ServerMessage = bincode::deserialize(&serialized).unwrap();

        assert_eq!(msg, deserialized);
    }

    #[test]
    fn test_pane_update_message_roundtrip() {
        use crate::cell::{Cell, Color};

        let msg = ServerMessage::PaneUpdate {
            pane_id: 1,
            changed_rows: vec![
                PaneRow::new(0, vec![Cell::new('$'), Cell::new(' ')]),
                PaneRow::new(
                    1,
                    vec![Cell::styled(
                        'X',
                        Color::rgb(0, 255, 0),
                        Color::default_color(),
                        crate::cell::CellFlags::ITALIC,
                    )],
                ),
            ],
            cursor: Some(CursorState {
                row: 1,
                col: 5,
                visible: true,
            }),
        };

        let serialized = bincode::serialize(&msg).unwrap();
        let deserialized: ServerMessage = bincode::deserialize(&serialized).unwrap();

        assert_eq!(msg, deserialized);
    }

    #[test]
    fn test_pane_update_without_cursor() {
        let msg = ServerMessage::PaneUpdate {
            pane_id: 0,
            changed_rows: vec![PaneRow::new(0, vec![Cell::default()])],
            cursor: None,
        };

        let serialized = bincode::serialize(&msg).unwrap();
        let deserialized: ServerMessage = bincode::deserialize(&serialized).unwrap();

        assert_eq!(msg, deserialized);
    }

    #[test]
    fn test_pane_row_links_roundtrip() {
        let msg = ServerMessage::PaneUpdate {
            pane_id: 3,
            changed_rows: vec![PaneRow::with_links(
                2,
                "https://example.com/x".chars().map(Cell::new).collect(),
                vec![
                    RowLink {
                        start_col: 0,
                        end_col: 21,
                        id: 4242,
                        url: "https://example.com/x".to_string(),
                        detected: true,
                    },
                    RowLink {
                        start_col: 30,
                        end_col: 37,
                        id: 99,
                        url: "https://example.com/osc8".to_string(),
                        detected: false,
                    },
                ],
            )],
            cursor: None,
        };

        let serialized = bincode::serialize(&msg).unwrap();
        let deserialized: ServerMessage = bincode::deserialize(&serialized).unwrap();

        assert_eq!(msg, deserialized);
    }

    #[test]
    fn test_pane_update_message_sizes() {
        use crate::cell::Cell;

        // A typical pane update should be reasonably sized
        let pane_update = ServerMessage::PaneUpdate {
            pane_id: 0,
            changed_rows: vec![PaneRow::new(0, (0..80).map(|_| Cell::new('x')).collect())],
            cursor: Some(CursorState::default()),
        };
        let pane_update_size = bincode::serialize(&pane_update).unwrap().len();
        // 80 cells * ~20 bytes per cell + overhead should be < 2KB
        assert!(
            pane_update_size < 2048,
            "PaneUpdate for 80 cells should be < 2KB, got {} bytes",
            pane_update_size
        );

        // Layout change should be tiny
        let layout_changed = ServerMessage::LayoutChanged {
            layout: WindowLayout {
                panes: vec![
                    PaneLayout {
                        pane_id: 0,
                        x: 0,
                        y: 0,
                        width: 40,
                        height: 24,
                        focused: true,
                    },
                    PaneLayout {
                        pane_id: 1,
                        x: 41,
                        y: 0,
                        width: 39,
                        height: 24,
                        focused: false,
                    },
                ],
                screen_cols: 80,
                screen_rows: 24,
            },
        };
        let layout_size = bincode::serialize(&layout_changed).unwrap().len();
        assert!(
            layout_size < 100,
            "LayoutChanged for 2 panes should be < 100 bytes, got {} bytes",
            layout_size
        );
    }
}
