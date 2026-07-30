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

mod framing;
#[cfg(test)]
mod framing_tests;
#[cfg(test)]
mod message_tests;
mod messages;
mod rows;
#[cfg(test)]
mod tests;

pub use framing::{
    read_message, write_message, MessageReader, MessageWriter, ProtocolError, ProtocolResult,
};
pub use messages::{ClientMessage, CommandAction, DetachReason, Direction, ServerMessage};
pub use rows::{CursorState, PaneLayout, PaneRow, RowLink, SessionInfo, WindowLayout};
