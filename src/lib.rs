//! Clux - A terminal multiplexer focused on UX
//!
//! Clux aims to solve common UX pain points in terminal multiplexers:
//! - Reliable hyperlink support (OSC 8)
//! - Smooth scrollback with easy copy/paste
//! - Fast rendering at up to 120Hz
//!
//! # Architecture
//!
//! - `cell`: Terminal cell representation (character + colors + attributes)
//! - `grid`: 2D grid storage for the terminal
//! - `pty`: PTY management and shell spawning
//! - `terminal`: VTE escape sequence handling
//! - `render`: Output to host terminal with damage tracking
//! - `event`: Input event processing
//! - `clipboard`: System clipboard integration
//! - `selection`: Text selection for copy/paste
//! - `pane`: Pane management and layout tree
//! - `window`: Window management for z-stacking
//! - `config`: Configuration file parsing and keybinding management
//! - `protocol`: Client-server communication protocol

pub mod cell;
pub mod client;
pub mod clipboard;
pub mod config;
pub mod event;
pub mod grid;
pub mod hyperlink;
pub mod pane;
pub mod protocol;
pub mod pty;
pub mod render;
pub mod scrollback;
pub mod selection;
pub mod server;
pub mod session;
pub mod terminal;
pub mod window;

pub use cell::{Cell, CellFlags, Color, ColorKind, HyperlinkId};
pub use clipboard::{copy, init as init_clipboard, paste};
pub use config::{Config, ParsedKey};
pub use grid::{Grid, Row};
pub use pane::{Direction, Pane, PaneId, PaneManager, Rect, SplitDirection};
pub use protocol::{
    ClientMessage, CommandAction, CursorShape, CursorState, DetachReason, ProtocolError,
    RenderedRow, ServerMessage, SessionInfo,
};
pub use pty::{Pty, PtyError, PtySize};
pub use render::Renderer;
pub use scrollback::Scrollback;
pub use selection::{Point, Selection, SelectionMode};
pub use session::{ClientId, Session, SessionError, SessionId, SessionManager};
pub use terminal::{Cursor, Terminal};
pub use window::{Window, WindowId, WindowManager};
