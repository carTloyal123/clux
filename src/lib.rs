//! Clux - A terminal multiplexer focused on UX
//!
//! Clux aims to solve common UX pain points in terminal multiplexers:
//! - Reliable hyperlink support (OSC 8)
//! - Smooth scrollback with easy copy/paste
//! - Fast rendering at up to 120Hz
//!
//! # Architecture
//!
//! Clux is always client/server: the `clux` client spawns a local `clux-server`
//! on first use, the server owns every session's PTYs and terminal state, and the
//! client composites pane updates and paints the host terminal. There is no
//! single-process mode - see AGENTS.md.
//!
//! Server side:
//! - `pty`: PTY management and shell spawning
//! - `terminal`: VTE escape sequence handling
//! - `cell`: one character with its styling
//! - `buffer`: all terminal content - history and the active screen, in one place
//! - `pane`: Pane management and layout tree
//! - `window`: Window management for z-stacking
//! - `session`: Sessions and attached clients
//! - `server`: Event loop, client connections, session lifetime
//! - `hyperlink` / `urls`: Link resolution for the host terminal
//!
//! Client side:
//! - `client`: Connection, screen compositing, ANSI output, mouse encoding
//!
//! Shared:
//! - `protocol`: Client-server communication protocol
//! - `config`: Configuration file parsing and keybinding management
//!
//! Client-side selection and copy:
//! - `selection`: Text selection geometry
//! - `clipboard`: Clipboard writes via OSC 52

pub mod buffer;
pub mod cell;
pub mod client;
pub mod clipboard;
pub mod config;
pub mod hyperlink;
pub mod pane;
pub mod protocol;
pub mod pty;
pub mod selection;
pub mod server;
pub mod session;
pub mod terminal;
pub mod urls;
pub mod window;

pub use cell::{Cell, CellFlags, Color, ColorKind, HyperlinkId};
pub use clipboard::{copy_to_host, ClipboardError};
pub use config::{Config, ParsedKey};
pub use pane::{Direction, Pane, PaneId, PaneManager, Rect, SplitDirection};
pub use protocol::{
    ClientMessage, CommandAction, CursorState, DetachReason, PaneRow, ProtocolError, RowLink,
    ServerMessage, SessionInfo,
};
pub use pty::{Pty, PtyError, PtySize};
pub use selection::{Point, Selection, SelectionMode};
pub use session::{ClientId, Session, SessionError, SessionId, SessionManager};
pub use terminal::{Cursor, Terminal};
pub use window::{Window, WindowId, WindowManager};
