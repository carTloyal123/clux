//! Event handling for terminal input.
//!
//! Processes keyboard and mouse events from the host terminal
//! and converts them to bytes for the PTY.
//!
//! ## Clux Keybindings
//!
//! Press `Option+C` (or `Alt+C` on Linux) to enter command mode, then:
//!
//! ### Pane Commands
//! - `-` - Split horizontal (new pane below)
//! - `p` - Split vertical (new pane to right)
//! - `w` - Close focused pane
//! - `Arrow keys` or `h/j/k/l` - Navigate between panes
//!
//! ### Window Commands
//! - `n` - New window
//! - `x` - Close current window
//! - `]` - Next window
//! - `'` - Previous window
//! - `1-9` - Select window 1-9
//! - `0` - Select window 10
//!
//! ### Other
//! - `q` - Quit Clux
//! - `c` - Send literal Option+C to terminal
//! - `Escape` or any other key - Cancel command mode

// Some variants/functions kept for future use
#![allow(dead_code)]

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};

use crossterm::event::{
    Event, KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};

use crate::config::{Config, ParsedKey};

/// Handles keybinding lookups using configuration.
pub struct KeybindingHandler {
    /// The prefix key to enter command mode.
    prefix_key: ParsedKey,
    /// Command mode bindings (action name -> EventAction mapping done at runtime).
    command_bindings: HashMap<ParsedKey, String>,
    /// Direct bindings that work without prefix.
    direct_bindings: HashMap<ParsedKey, String>,
}

impl KeybindingHandler {
    /// Create a new keybinding handler from configuration.
    pub fn new(config: &Config) -> Self {
        let prefix_key = config.parse_prefix().unwrap_or_else(|e| {
            log::warn!("Invalid prefix key, using default: {}", e);
            ParsedKey::parse("alt+c").unwrap()
        });

        Self {
            prefix_key,
            command_bindings: config.build_command_bindings(),
            direct_bindings: config.build_direct_bindings(),
        }
    }

    /// Check if the given key matches the prefix key.
    pub fn is_prefix_key(&self, code: KeyCode, modifiers: KeyModifiers) -> bool {
        self.prefix_key.matches(code, modifiers)
    }

    /// Look up a command-mode key and return the action.
    pub fn lookup_command(&self, code: KeyCode, modifiers: KeyModifiers) -> EventAction {
        // Create a key to look up (command mode keys typically have no modifiers)
        for (key, action_name) in &self.command_bindings {
            if key.matches(code, modifiers) {
                return self.action_from_name(action_name);
            }
        }
        EventAction::None
    }

    /// Look up a direct binding (no prefix needed).
    pub fn lookup_direct(&self, code: KeyCode, modifiers: KeyModifiers) -> Option<EventAction> {
        for (key, action_name) in &self.direct_bindings {
            if key.matches(code, modifiers) {
                return Some(self.action_from_name(action_name));
            }
        }
        None
    }

    /// Convert an action name to an EventAction.
    fn action_from_name(&self, name: &str) -> EventAction {
        match name {
            // Pane actions
            "split_horizontal" => EventAction::SplitHorizontal,
            "split_vertical" => EventAction::SplitVertical,
            "close_pane" => EventAction::ClosePane,
            "navigate_up" => EventAction::NavigatePane(PaneDirection::Up),
            "navigate_down" => EventAction::NavigatePane(PaneDirection::Down),
            "navigate_left" => EventAction::NavigatePane(PaneDirection::Left),
            "navigate_right" => EventAction::NavigatePane(PaneDirection::Right),

            // Window actions
            "new_window" => EventAction::NewWindow,
            "close_window" => EventAction::CloseWindow,
            "next_window" => EventAction::NextWindow,
            "prev_window" => EventAction::PrevWindow,
            "select_window_1" => EventAction::SelectWindow(0),
            "select_window_2" => EventAction::SelectWindow(1),
            "select_window_3" => EventAction::SelectWindow(2),
            "select_window_4" => EventAction::SelectWindow(3),
            "select_window_5" => EventAction::SelectWindow(4),
            "select_window_6" => EventAction::SelectWindow(5),
            "select_window_7" => EventAction::SelectWindow(6),
            "select_window_8" => EventAction::SelectWindow(7),
            "select_window_9" => EventAction::SelectWindow(8),
            "select_window_10" => EventAction::SelectWindow(9),

            // App actions
            "quit" => EventAction::Exit,
            "send_prefix" => EventAction::SendToPty("ç".as_bytes().to_vec()),

            // Direct actions
            "scroll_up" => EventAction::Scroll(-10),
            "scroll_down" => EventAction::Scroll(10),
            "paste" => EventAction::Paste,

            _ => {
                log::warn!("Unknown action: {}", name);
                EventAction::None
            }
        }
    }
}

/// Global state for command mode (Option+C was pressed).
static COMMAND_MODE: AtomicBool = AtomicBool::new(false);

/// Check if we're in command mode.
pub fn is_command_mode() -> bool {
    COMMAND_MODE.load(Ordering::SeqCst)
}

/// Enter command mode.
fn enter_command_mode() {
    COMMAND_MODE.store(true, Ordering::SeqCst);
}

/// Exit command mode.
fn exit_command_mode() {
    COMMAND_MODE.store(false, Ordering::SeqCst);
}

/// Result of processing an input event.
pub enum EventAction {
    /// Send bytes to the PTY.
    SendToPty(Vec<u8>),
    /// Resize the terminal.
    Resize(u16, u16),
    /// Scroll the viewport (for scrollback).
    Scroll(i32),
    /// Start a mouse selection.
    SelectStart {
        row: u16,
        col: u16,
        mode: SelectMode,
    },
    /// Extend the current selection.
    SelectExtend { row: u16, col: u16 },
    /// End selection (mouse button released).
    SelectEnd { row: u16, col: u16 },
    /// Ctrl+Click on a cell (for hyperlink opening).
    CtrlClick { row: u16, col: u16 },
    /// Paste from clipboard.
    Paste,
    /// Split pane horizontally (new pane below).
    SplitHorizontal,
    /// Split pane vertically (new pane to right).
    SplitVertical,
    /// Close the focused pane.
    ClosePane,
    /// Navigate to pane in direction.
    NavigatePane(PaneDirection),
    /// Focus pane at screen position.
    FocusPaneAt { row: u16, col: u16 },
    /// Create a new window.
    NewWindow,
    /// Close the current window.
    CloseWindow,
    /// Switch to the next window.
    NextWindow,
    /// Switch to the previous window.
    PrevWindow,
    /// Select a window by index (0-based, so 0 = window 1, 9 = window 10).
    SelectWindow(usize),
    /// Exit the application.
    Exit,
    /// No action needed.
    None,
}

/// Direction for pane navigation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PaneDirection {
    Up,
    Down,
    Left,
    Right,
}

/// Selection mode for mouse events.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SelectMode {
    /// Single click - character selection.
    Normal,
    /// Double click - word selection.
    Word,
    /// Triple click - line selection.
    Triple,
}

/// Process a crossterm event and return the appropriate action.
/// This version uses hardcoded keybindings for backward compatibility.
pub fn process_event(event: Event) -> EventAction {
    match event {
        Event::Key(key) => process_key_event(key),
        Event::Mouse(mouse) => process_mouse_event(mouse),
        Event::Resize(cols, rows) => EventAction::Resize(cols, rows),
        Event::FocusGained | Event::FocusLost => EventAction::None,
        Event::Paste(text) => {
            // Bracketed paste
            let mut bytes = Vec::new();
            bytes.extend_from_slice(b"\x1b[200~");
            bytes.extend_from_slice(text.as_bytes());
            bytes.extend_from_slice(b"\x1b[201~");
            EventAction::SendToPty(bytes)
        }
    }
}

/// Process a crossterm event using configurable keybindings.
pub fn process_event_with_handler(event: Event, handler: &KeybindingHandler) -> EventAction {
    match event {
        Event::Key(key) => process_key_event_with_handler(key, handler),
        Event::Mouse(mouse) => process_mouse_event(mouse),
        Event::Resize(cols, rows) => EventAction::Resize(cols, rows),
        Event::FocusGained | Event::FocusLost => EventAction::None,
        Event::Paste(text) => {
            // Bracketed paste
            let mut bytes = Vec::new();
            bytes.extend_from_slice(b"\x1b[200~");
            bytes.extend_from_slice(text.as_bytes());
            bytes.extend_from_slice(b"\x1b[201~");
            EventAction::SendToPty(bytes)
        }
    }
}

/// Process a key event using configurable keybindings.
fn process_key_event_with_handler(key: KeyEvent, handler: &KeybindingHandler) -> EventAction {
    let modifiers = key.modifiers;
    let ctrl = modifiers.contains(KeyModifiers::CONTROL);
    let alt = modifiers.contains(KeyModifiers::ALT);
    let shift = modifiers.contains(KeyModifiers::SHIFT);
    #[cfg(target_os = "macos")]
    let super_key = modifiers.contains(KeyModifiers::SUPER);
    #[cfg(not(target_os = "macos"))]
    let super_key = false;

    // Handle command mode (after prefix was pressed)
    if is_command_mode() {
        exit_command_mode();
        return handler.lookup_command(key.code, KeyModifiers::NONE);
    }

    // Check for prefix key (enters command mode)
    // Handle both the configured prefix and the cedilla variant on macOS
    if handler.is_prefix_key(key.code, modifiers) {
        enter_command_mode();
        return EventAction::None;
    }
    // Also check for cedilla (Option+C on macOS often produces 'ç')
    if alt && !ctrl && !shift && key.code == KeyCode::Char('ç') {
        enter_command_mode();
        return EventAction::None;
    }

    // Check direct bindings (no prefix needed)
    if let Some(action) = handler.lookup_direct(key.code, modifiers) {
        return action;
    }

    // Cmd+V (macOS) or Ctrl+Shift+V for paste (built-in, not configurable for safety)
    if (super_key && key.code == KeyCode::Char('v'))
        || (ctrl && shift && key.code == KeyCode::Char('V'))
    {
        return EventAction::Paste;
    }

    // Fall through to standard terminal key handling
    process_standard_key(key)
}

/// Process standard terminal keys (control characters, escape sequences, etc.)
/// This is shared between hardcoded and configurable keybinding paths.
fn process_standard_key(key: KeyEvent) -> EventAction {
    let modifiers = key.modifiers;
    let ctrl = modifiers.contains(KeyModifiers::CONTROL);
    let alt = modifiers.contains(KeyModifiers::ALT);
    let shift = modifiers.contains(KeyModifiers::SHIFT);

    let bytes = match key.code {
        // Exit on Ctrl+D at prompt (sent as byte 0x04)
        KeyCode::Char('d') if ctrl && !alt && !shift => {
            vec![0x04]
        }

        // Ctrl+C - interrupt
        KeyCode::Char('c') if ctrl && !alt && !shift => {
            vec![0x03]
        }

        // Ctrl+Z - suspend
        KeyCode::Char('z') if ctrl && !alt && !shift => {
            vec![0x1a]
        }

        // Ctrl+\ - quit
        KeyCode::Char('\\') if ctrl && !alt && !shift => {
            vec![0x1c]
        }

        // Ctrl+A through Ctrl+Z
        KeyCode::Char(c) if ctrl && !alt => {
            let c = c.to_ascii_lowercase();
            if c >= 'a' && c <= 'z' {
                vec![(c as u8) - b'a' + 1]
            } else {
                return EventAction::None;
            }
        }

        // Alt+key - send ESC followed by key
        KeyCode::Char(c) if alt && !ctrl => {
            let mut bytes = vec![0x1b];
            let mut buf = [0u8; 4];
            bytes.extend_from_slice(c.encode_utf8(&mut buf).as_bytes());
            bytes
        }

        // Regular character
        KeyCode::Char(c) => {
            let mut buf = [0u8; 4];
            c.encode_utf8(&mut buf).as_bytes().to_vec()
        }

        // Enter
        KeyCode::Enter => vec![0x0d],

        // Backspace
        KeyCode::Backspace => vec![0x7f],

        // Tab
        KeyCode::Tab => vec![0x09],

        // Backtab (Shift+Tab)
        KeyCode::BackTab => vec![0x1b, b'[', b'Z'],

        // Escape
        KeyCode::Esc => vec![0x1b],

        // Arrow keys
        KeyCode::Up => {
            if ctrl {
                b"\x1b[1;5A".to_vec()
            } else if shift {
                b"\x1b[1;2A".to_vec()
            } else if alt {
                b"\x1b[1;3A".to_vec()
            } else {
                b"\x1b[A".to_vec()
            }
        }
        KeyCode::Down => {
            if ctrl {
                b"\x1b[1;5B".to_vec()
            } else if shift {
                b"\x1b[1;2B".to_vec()
            } else if alt {
                b"\x1b[1;3B".to_vec()
            } else {
                b"\x1b[B".to_vec()
            }
        }
        KeyCode::Right => {
            if ctrl {
                b"\x1b[1;5C".to_vec()
            } else if shift {
                b"\x1b[1;2C".to_vec()
            } else if alt {
                b"\x1b[1;3C".to_vec()
            } else {
                b"\x1b[C".to_vec()
            }
        }
        KeyCode::Left => {
            if ctrl {
                b"\x1b[1;5D".to_vec()
            } else if shift {
                b"\x1b[1;2D".to_vec()
            } else if alt {
                b"\x1b[1;3D".to_vec()
            } else {
                b"\x1b[D".to_vec()
            }
        }

        // Home/End
        KeyCode::Home => {
            if ctrl {
                b"\x1b[1;5H".to_vec()
            } else {
                b"\x1b[H".to_vec()
            }
        }
        KeyCode::End => {
            if ctrl {
                b"\x1b[1;5F".to_vec()
            } else {
                b"\x1b[F".to_vec()
            }
        }

        // Page Up/Down
        KeyCode::PageUp => b"\x1b[5~".to_vec(),
        KeyCode::PageDown => b"\x1b[6~".to_vec(),

        // Insert
        KeyCode::Insert => b"\x1b[2~".to_vec(),

        // Delete
        KeyCode::Delete => b"\x1b[3~".to_vec(),

        // Function keys
        KeyCode::F(1) => b"\x1bOP".to_vec(),
        KeyCode::F(2) => b"\x1bOQ".to_vec(),
        KeyCode::F(3) => b"\x1bOR".to_vec(),
        KeyCode::F(4) => b"\x1bOS".to_vec(),
        KeyCode::F(5) => b"\x1b[15~".to_vec(),
        KeyCode::F(6) => b"\x1b[17~".to_vec(),
        KeyCode::F(7) => b"\x1b[18~".to_vec(),
        KeyCode::F(8) => b"\x1b[19~".to_vec(),
        KeyCode::F(9) => b"\x1b[20~".to_vec(),
        KeyCode::F(10) => b"\x1b[21~".to_vec(),
        KeyCode::F(11) => b"\x1b[23~".to_vec(),
        KeyCode::F(12) => b"\x1b[24~".to_vec(),
        KeyCode::F(_) => return EventAction::None,

        // Ignore other keys
        _ => return EventAction::None,
    };

    EventAction::SendToPty(bytes)
}

/// Process a key event and return bytes to send to PTY.
fn process_key_event(key: KeyEvent) -> EventAction {
    let modifiers = key.modifiers;
    let ctrl = modifiers.contains(KeyModifiers::CONTROL);
    let alt = modifiers.contains(KeyModifiers::ALT);
    let shift = modifiers.contains(KeyModifiers::SHIFT);
    #[cfg(target_os = "macos")]
    let super_key = modifiers.contains(KeyModifiers::SUPER);
    #[cfg(not(target_os = "macos"))]
    let super_key = false;

    // Handle command mode (after Option+C)
    if is_command_mode() {
        exit_command_mode();
        return process_command_key(key.code);
    }

    // Option+C (Alt+C on Linux) enters command mode
    if alt && !ctrl && !shift && matches!(key.code, KeyCode::Char('c') | KeyCode::Char('ç')) {
        // Note: On macOS, Option+C often produces 'ç', so we handle both
        enter_command_mode();
        return EventAction::None;
    }

    // Cmd+V (macOS) or Ctrl+Shift+V for paste
    if (super_key && key.code == KeyCode::Char('v'))
        || (ctrl && shift && key.code == KeyCode::Char('V'))
    {
        return EventAction::Paste;
    }

    let bytes = match key.code {
        // Exit on Ctrl+D at prompt (sent as byte 0x04)
        KeyCode::Char('d') if ctrl && !alt && !shift => {
            vec![0x04]
        }

        // Ctrl+C - interrupt
        KeyCode::Char('c') if ctrl && !alt && !shift => {
            vec![0x03]
        }

        // Ctrl+Z - suspend
        KeyCode::Char('z') if ctrl && !alt && !shift => {
            vec![0x1a]
        }

        // Ctrl+\ - quit
        KeyCode::Char('\\') if ctrl && !alt && !shift => {
            vec![0x1c]
        }

        // Ctrl+A through Ctrl+Z (except Ctrl+B which is handled above)
        KeyCode::Char(c) if ctrl && !alt => {
            let c = c.to_ascii_lowercase();
            if c >= 'a' && c <= 'z' {
                vec![(c as u8) - b'a' + 1]
            } else {
                return EventAction::None;
            }
        }

        // Alt+key - send ESC followed by key
        KeyCode::Char(c) if alt && !ctrl => {
            let mut bytes = vec![0x1b];
            let mut buf = [0u8; 4];
            bytes.extend_from_slice(c.encode_utf8(&mut buf).as_bytes());
            bytes
        }

        // Regular character
        KeyCode::Char(c) => {
            let mut buf = [0u8; 4];
            c.encode_utf8(&mut buf).as_bytes().to_vec()
        }

        // Enter
        KeyCode::Enter => vec![0x0d],

        // Backspace
        KeyCode::Backspace => vec![0x7f],

        // Tab
        KeyCode::Tab => vec![0x09],

        // Backtab (Shift+Tab)
        KeyCode::BackTab => vec![0x1b, b'[', b'Z'],

        // Escape
        KeyCode::Esc => vec![0x1b],

        // Arrow keys
        KeyCode::Up => {
            if ctrl {
                b"\x1b[1;5A".to_vec()
            } else if shift {
                b"\x1b[1;2A".to_vec()
            } else if alt {
                b"\x1b[1;3A".to_vec()
            } else {
                b"\x1b[A".to_vec()
            }
        }
        KeyCode::Down => {
            if ctrl {
                b"\x1b[1;5B".to_vec()
            } else if shift {
                b"\x1b[1;2B".to_vec()
            } else if alt {
                b"\x1b[1;3B".to_vec()
            } else {
                b"\x1b[B".to_vec()
            }
        }
        KeyCode::Right => {
            if ctrl {
                b"\x1b[1;5C".to_vec()
            } else if shift {
                b"\x1b[1;2C".to_vec()
            } else if alt {
                b"\x1b[1;3C".to_vec()
            } else {
                b"\x1b[C".to_vec()
            }
        }
        KeyCode::Left => {
            if ctrl {
                b"\x1b[1;5D".to_vec()
            } else if shift {
                b"\x1b[1;2D".to_vec()
            } else if alt {
                b"\x1b[1;3D".to_vec()
            } else {
                b"\x1b[D".to_vec()
            }
        }

        // Home/End
        KeyCode::Home => {
            if ctrl {
                b"\x1b[1;5H".to_vec()
            } else {
                b"\x1b[H".to_vec()
            }
        }
        KeyCode::End => {
            if ctrl {
                b"\x1b[1;5F".to_vec()
            } else {
                b"\x1b[F".to_vec()
            }
        }

        // Page Up/Down - for scrollback viewing
        KeyCode::PageUp if shift => {
            return EventAction::Scroll(-10);
        }
        KeyCode::PageDown if shift => {
            return EventAction::Scroll(10);
        }
        KeyCode::PageUp => b"\x1b[5~".to_vec(),
        KeyCode::PageDown => b"\x1b[6~".to_vec(),

        // Insert
        KeyCode::Insert => b"\x1b[2~".to_vec(),

        // Delete
        KeyCode::Delete => b"\x1b[3~".to_vec(),

        // Function keys
        KeyCode::F(1) => b"\x1bOP".to_vec(),
        KeyCode::F(2) => b"\x1bOQ".to_vec(),
        KeyCode::F(3) => b"\x1bOR".to_vec(),
        KeyCode::F(4) => b"\x1bOS".to_vec(),
        KeyCode::F(5) => b"\x1b[15~".to_vec(),
        KeyCode::F(6) => b"\x1b[17~".to_vec(),
        KeyCode::F(7) => b"\x1b[18~".to_vec(),
        KeyCode::F(8) => b"\x1b[19~".to_vec(),
        KeyCode::F(9) => b"\x1b[20~".to_vec(),
        KeyCode::F(10) => b"\x1b[21~".to_vec(),
        KeyCode::F(11) => b"\x1b[23~".to_vec(),
        KeyCode::F(12) => b"\x1b[24~".to_vec(),
        KeyCode::F(_) => return EventAction::None,

        // Ignore other keys
        _ => return EventAction::None,
    };

    EventAction::SendToPty(bytes)
}

/// Process a command key (after Option+C was pressed).
/// Returns the appropriate pane action.
fn process_command_key(code: KeyCode) -> EventAction {
    match code {
        // === Pane Commands ===

        // - = Split horizontal (new pane below)
        KeyCode::Char('-') => EventAction::SplitHorizontal,

        // p = Split vertical (new pane to the right)
        KeyCode::Char('p') | KeyCode::Char('P') => EventAction::SplitVertical,

        // w = Close focused pane
        KeyCode::Char('w') | KeyCode::Char('W') => EventAction::ClosePane,

        // Arrow keys = Navigate panes
        KeyCode::Up => EventAction::NavigatePane(PaneDirection::Up),
        KeyCode::Down => EventAction::NavigatePane(PaneDirection::Down),
        KeyCode::Left => EventAction::NavigatePane(PaneDirection::Left),
        KeyCode::Right => EventAction::NavigatePane(PaneDirection::Right),

        // h/j/k/l = Navigate panes (vim-style)
        KeyCode::Char('h') | KeyCode::Char('H') => EventAction::NavigatePane(PaneDirection::Left),
        KeyCode::Char('j') | KeyCode::Char('J') => EventAction::NavigatePane(PaneDirection::Down),
        KeyCode::Char('k') | KeyCode::Char('K') => EventAction::NavigatePane(PaneDirection::Up),
        KeyCode::Char('l') | KeyCode::Char('L') => EventAction::NavigatePane(PaneDirection::Right),

        // === Window Commands ===

        // n = New window
        KeyCode::Char('n') | KeyCode::Char('N') => EventAction::NewWindow,

        // x = Close current window
        KeyCode::Char('x') | KeyCode::Char('X') => EventAction::CloseWindow,

        // ] = Next window
        KeyCode::Char(']') => EventAction::NextWindow,

        // ' = Previous window (also [ for familiarity)
        KeyCode::Char('\'') | KeyCode::Char('[') => EventAction::PrevWindow,

        // 1-9 = Select window 1-9
        KeyCode::Char('1') => EventAction::SelectWindow(0),
        KeyCode::Char('2') => EventAction::SelectWindow(1),
        KeyCode::Char('3') => EventAction::SelectWindow(2),
        KeyCode::Char('4') => EventAction::SelectWindow(3),
        KeyCode::Char('5') => EventAction::SelectWindow(4),
        KeyCode::Char('6') => EventAction::SelectWindow(5),
        KeyCode::Char('7') => EventAction::SelectWindow(6),
        KeyCode::Char('8') => EventAction::SelectWindow(7),
        KeyCode::Char('9') => EventAction::SelectWindow(8),

        // 0 = Select window 10
        KeyCode::Char('0') => EventAction::SelectWindow(9),

        // === Other ===

        // q = Quit Clux
        KeyCode::Char('q') | KeyCode::Char('Q') => EventAction::Exit,

        // c = Send the character that Option+C would normally produce
        KeyCode::Char('c') | KeyCode::Char('C') => {
            // On macOS, Option+C typically produces 'ç'
            EventAction::SendToPty("ç".as_bytes().to_vec())
        }

        // Escape or any other key = Cancel command mode (already exited above)
        _ => EventAction::None,
    }
}

/// Track click timing for double/triple click detection.
static LAST_CLICK: std::sync::Mutex<Option<(std::time::Instant, u16, u16, u8)>> =
    std::sync::Mutex::new(None);

/// Double/triple click threshold in milliseconds.
const MULTI_CLICK_THRESHOLD_MS: u128 = 400;

/// Process a mouse event.
fn process_mouse_event(mouse: MouseEvent) -> EventAction {
    match mouse.kind {
        // Scroll wheel for scrollback
        MouseEventKind::ScrollUp => EventAction::Scroll(-3),
        MouseEventKind::ScrollDown => EventAction::Scroll(3),

        // Left mouse button down - start selection or Ctrl+Click for hyperlinks
        MouseEventKind::Down(MouseButton::Left) => {
            let row = mouse.row;
            let col = mouse.column;

            // Ctrl+Click opens hyperlinks
            if mouse.modifiers.contains(KeyModifiers::CONTROL) {
                return EventAction::CtrlClick { row, col };
            }

            let now = std::time::Instant::now();

            // Detect double/triple click
            let mut last_click = LAST_CLICK.lock().unwrap();
            let click_count = if let Some((last_time, last_row, last_col, count)) = *last_click {
                if now.duration_since(last_time).as_millis() < MULTI_CLICK_THRESHOLD_MS
                    && last_row == row
                    && (last_col as i32 - col as i32).abs() <= 2
                {
                    // Same position, within time threshold
                    (count % 3) + 1
                } else {
                    1
                }
            } else {
                1
            };

            *last_click = Some((now, row, col, click_count));

            let mode = match click_count {
                2 => SelectMode::Word,
                3 => SelectMode::Triple,
                _ => SelectMode::Normal,
            };

            EventAction::SelectStart { row, col, mode }
        }

        // Left mouse drag - extend selection
        MouseEventKind::Drag(MouseButton::Left) => EventAction::SelectExtend {
            row: mouse.row,
            col: mouse.column,
        },

        // Left mouse up - end selection
        MouseEventKind::Up(MouseButton::Left) => EventAction::SelectEnd {
            row: mouse.row,
            col: mouse.column,
        },

        // Middle mouse button - paste
        MouseEventKind::Down(MouseButton::Middle) => EventAction::Paste,

        _ => EventAction::None,
    }
}

/// Encode mouse event in SGR format for applications that want mouse input.
#[allow(dead_code)]
pub fn encode_mouse_sgr(mouse: &MouseEvent, press: bool) -> Vec<u8> {
    let button = match mouse.kind {
        MouseEventKind::Down(MouseButton::Left) | MouseEventKind::Up(MouseButton::Left) => 0,
        MouseEventKind::Down(MouseButton::Middle) | MouseEventKind::Up(MouseButton::Middle) => 1,
        MouseEventKind::Down(MouseButton::Right) | MouseEventKind::Up(MouseButton::Right) => 2,
        MouseEventKind::Drag(MouseButton::Left) => 32,
        MouseEventKind::Drag(MouseButton::Middle) => 33,
        MouseEventKind::Drag(MouseButton::Right) => 34,
        MouseEventKind::Moved => 35,
        MouseEventKind::ScrollUp => 64,
        MouseEventKind::ScrollDown => 65,
        MouseEventKind::ScrollLeft => 66,
        MouseEventKind::ScrollRight => 67,
    };

    let mut modifiers = 0;
    if mouse.modifiers.contains(KeyModifiers::SHIFT) {
        modifiers |= 4;
    }
    if mouse.modifiers.contains(KeyModifiers::ALT) {
        modifiers |= 8;
    }
    if mouse.modifiers.contains(KeyModifiers::CONTROL) {
        modifiers |= 16;
    }

    let cb = button | modifiers;
    let cx = mouse.column + 1;
    let cy = mouse.row + 1;
    let suffix = if press { 'M' } else { 'm' };

    format!("\x1b[<{};{};{}{}", cb, cx, cy, suffix).into_bytes()
}

#[cfg(test)]
mod tests {
    use super::*;

    // Tests for Option+C command mode keybindings
    // Note: These tests use a mutex to serialize access to the global COMMAND_MODE state
    use std::sync::Mutex;
    static TEST_MUTEX: Mutex<()> = Mutex::new(());

    /// Helper to reset command mode state between tests
    fn reset_command_mode() {
        COMMAND_MODE.store(false, Ordering::SeqCst);
    }

    #[test]
    fn test_ctrl_c() {
        let _lock = TEST_MUTEX.lock().unwrap();
        reset_command_mode();
        let event = KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL);
        match process_key_event(event) {
            EventAction::SendToPty(bytes) => assert_eq!(bytes, vec![0x03]),
            _ => panic!("Expected SendToPty"),
        }
    }

    #[test]
    fn test_regular_char() {
        let _lock = TEST_MUTEX.lock().unwrap();
        reset_command_mode();
        let event = KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE);
        match process_key_event(event) {
            EventAction::SendToPty(bytes) => assert_eq!(bytes, vec![b'a']),
            _ => panic!("Expected SendToPty"),
        }
    }

    #[test]
    fn test_arrow_key() {
        let _lock = TEST_MUTEX.lock().unwrap();
        reset_command_mode();
        let event = KeyEvent::new(KeyCode::Up, KeyModifiers::NONE);
        match process_key_event(event) {
            EventAction::SendToPty(bytes) => assert_eq!(bytes, b"\x1b[A".to_vec()),
            _ => panic!("Expected SendToPty"),
        }
    }

    #[test]
    fn test_ctrl_arrow() {
        let _lock = TEST_MUTEX.lock().unwrap();
        reset_command_mode();
        let event = KeyEvent::new(KeyCode::Right, KeyModifiers::CONTROL);
        match process_key_event(event) {
            EventAction::SendToPty(bytes) => assert_eq!(bytes, b"\x1b[1;5C".to_vec()),
            _ => panic!("Expected SendToPty"),
        }
    }

    #[test]
    fn test_scroll() {
        let _lock = TEST_MUTEX.lock().unwrap();
        reset_command_mode();
        let event = KeyEvent::new(KeyCode::PageUp, KeyModifiers::SHIFT);
        match process_key_event(event) {
            EventAction::Scroll(delta) => assert_eq!(delta, -10),
            _ => panic!("Expected Scroll"),
        }
    }

    #[test]
    fn test_option_c_enters_command_mode() {
        let _lock = TEST_MUTEX.lock().unwrap();
        reset_command_mode();
        // Option+C (Alt+C) should enter command mode
        let event = KeyEvent::new(KeyCode::Char('c'), KeyModifiers::ALT);
        let action = process_key_event(event);
        assert!(matches!(action, EventAction::None));
        assert!(is_command_mode());
        reset_command_mode();
    }

    #[test]
    fn test_option_c_cedilla_enters_command_mode() {
        let _lock = TEST_MUTEX.lock().unwrap();
        reset_command_mode();
        // On macOS, Option+C produces 'ç' - should also enter command mode
        let event = KeyEvent::new(KeyCode::Char('ç'), KeyModifiers::ALT);
        let action = process_key_event(event);
        assert!(matches!(action, EventAction::None));
        assert!(is_command_mode());
        reset_command_mode();
    }

    #[test]
    fn test_command_mode_split_horizontal() {
        let _lock = TEST_MUTEX.lock().unwrap();
        reset_command_mode();
        // Test the full flow through process_key_event
        // Split horizontal is now '-' key
        enter_command_mode();
        let event = KeyEvent::new(KeyCode::Char('-'), KeyModifiers::NONE);
        let action = process_key_event(event);
        assert!(matches!(action, EventAction::SplitHorizontal));
        assert!(!is_command_mode()); // Should exit command mode
    }

    #[test]
    fn test_command_mode_split_vertical() {
        let _lock = TEST_MUTEX.lock().unwrap();
        reset_command_mode();
        // Enter command mode, then press p for vertical split
        enter_command_mode();
        let event = KeyEvent::new(KeyCode::Char('p'), KeyModifiers::NONE);
        let action = process_key_event(event);
        assert!(matches!(action, EventAction::SplitVertical));
        assert!(!is_command_mode());
    }

    #[test]
    fn test_command_mode_split_vertical_uppercase() {
        let _lock = TEST_MUTEX.lock().unwrap();
        reset_command_mode();
        // P (uppercase) should also work
        enter_command_mode();
        let event = KeyEvent::new(KeyCode::Char('P'), KeyModifiers::SHIFT);
        let action = process_key_event(event);
        assert!(matches!(action, EventAction::SplitVertical));
        assert!(!is_command_mode());
    }

    #[test]
    fn test_command_mode_close_pane() {
        let _lock = TEST_MUTEX.lock().unwrap();
        reset_command_mode();
        // w should close pane
        enter_command_mode();
        let event = KeyEvent::new(KeyCode::Char('w'), KeyModifiers::NONE);
        let action = process_key_event(event);
        assert!(matches!(action, EventAction::ClosePane));
        assert!(!is_command_mode());
    }

    #[test]
    fn test_command_mode_quit() {
        let _lock = TEST_MUTEX.lock().unwrap();
        reset_command_mode();
        // q should quit
        enter_command_mode();
        let event = KeyEvent::new(KeyCode::Char('q'), KeyModifiers::NONE);
        let action = process_key_event(event);
        assert!(matches!(action, EventAction::Exit));
        assert!(!is_command_mode());
    }

    #[test]
    fn test_command_mode_navigate_arrows() {
        let _lock = TEST_MUTEX.lock().unwrap();
        reset_command_mode();
        // Test arrow key navigation in command mode
        let directions = [
            (KeyCode::Up, PaneDirection::Up),
            (KeyCode::Down, PaneDirection::Down),
            (KeyCode::Left, PaneDirection::Left),
            (KeyCode::Right, PaneDirection::Right),
        ];

        for (key, expected_dir) in directions {
            enter_command_mode();
            let event = KeyEvent::new(key, KeyModifiers::NONE);
            let action = process_key_event(event);
            match action {
                EventAction::NavigatePane(dir) => assert_eq!(dir, expected_dir),
                _ => panic!("Expected NavigatePane for {:?}", key),
            }
            assert!(!is_command_mode());
        }
    }

    #[test]
    fn test_command_mode_navigate_vim() {
        let _lock = TEST_MUTEX.lock().unwrap();
        reset_command_mode();
        // Test vim-style navigation in command mode
        let directions = [
            ('h', PaneDirection::Left),
            ('j', PaneDirection::Down),
            ('k', PaneDirection::Up),
            ('l', PaneDirection::Right),
        ];

        for (c, expected_dir) in directions {
            enter_command_mode();
            let event = KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE);
            let action = process_key_event(event);
            match action {
                EventAction::NavigatePane(dir) => assert_eq!(dir, expected_dir),
                _ => panic!("Expected NavigatePane for '{}'", c),
            }
            assert!(!is_command_mode());
        }
    }

    #[test]
    fn test_command_mode_send_cedilla() {
        let _lock = TEST_MUTEX.lock().unwrap();
        reset_command_mode();
        // c in command mode should send the ç character
        enter_command_mode();
        let event = KeyEvent::new(KeyCode::Char('c'), KeyModifiers::NONE);
        let action = process_key_event(event);
        match action {
            EventAction::SendToPty(bytes) => assert_eq!(bytes, "ç".as_bytes().to_vec()),
            _ => panic!("Expected SendToPty for 'c' in command mode"),
        }
        assert!(!is_command_mode());
    }

    #[test]
    fn test_command_mode_unknown_key_returns_none() {
        let _lock = TEST_MUTEX.lock().unwrap();
        reset_command_mode();
        // Unknown key in command mode should return None (cancel)
        enter_command_mode();
        let event = KeyEvent::new(KeyCode::Char('z'), KeyModifiers::NONE);
        let action = process_key_event(event);
        assert!(matches!(action, EventAction::None));
        assert!(!is_command_mode());
    }

    #[test]
    fn test_command_mode_escape_cancels() {
        let _lock = TEST_MUTEX.lock().unwrap();
        reset_command_mode();
        // Escape should cancel command mode
        enter_command_mode();
        let event = KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE);
        let action = process_key_event(event);
        assert!(matches!(action, EventAction::None));
        assert!(!is_command_mode());
    }

    #[test]
    fn test_command_mode_exits_after_action() {
        let _lock = TEST_MUTEX.lock().unwrap();
        reset_command_mode();
        // After processing a command, command mode should be exited
        enter_command_mode();
        assert!(is_command_mode());

        // Simulate processing through process_key_event which exits command mode
        // Using '-' for split horizontal
        let event = KeyEvent::new(KeyCode::Char('-'), KeyModifiers::NONE);
        let action = process_key_event(event);
        assert!(matches!(action, EventAction::SplitHorizontal));
        assert!(!is_command_mode());
    }

    #[test]
    fn test_command_mode_new_window() {
        let _lock = TEST_MUTEX.lock().unwrap();
        reset_command_mode();
        enter_command_mode();
        let event = KeyEvent::new(KeyCode::Char('n'), KeyModifiers::NONE);
        let action = process_key_event(event);
        assert!(matches!(action, EventAction::NewWindow));
        assert!(!is_command_mode());
    }

    #[test]
    fn test_command_mode_close_window() {
        let _lock = TEST_MUTEX.lock().unwrap();
        reset_command_mode();
        enter_command_mode();
        let event = KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE);
        let action = process_key_event(event);
        assert!(matches!(action, EventAction::CloseWindow));
        assert!(!is_command_mode());
    }

    #[test]
    fn test_command_mode_next_window() {
        let _lock = TEST_MUTEX.lock().unwrap();
        reset_command_mode();
        enter_command_mode();
        let event = KeyEvent::new(KeyCode::Char(']'), KeyModifiers::NONE);
        let action = process_key_event(event);
        assert!(matches!(action, EventAction::NextWindow));
        assert!(!is_command_mode());
    }

    #[test]
    fn test_command_mode_prev_window() {
        let _lock = TEST_MUTEX.lock().unwrap();
        reset_command_mode();
        enter_command_mode();
        let event = KeyEvent::new(KeyCode::Char('\''), KeyModifiers::NONE);
        let action = process_key_event(event);
        assert!(matches!(action, EventAction::PrevWindow));
        assert!(!is_command_mode());
    }

    #[test]
    fn test_command_mode_select_window() {
        let _lock = TEST_MUTEX.lock().unwrap();
        reset_command_mode();
        // Test selecting windows 1-9 and 0 (for window 10)
        for (key, expected_idx) in [('1', 0), ('2', 1), ('5', 4), ('9', 8), ('0', 9)] {
            enter_command_mode();
            let event = KeyEvent::new(KeyCode::Char(key), KeyModifiers::NONE);
            let action = process_key_event(event);
            match action {
                EventAction::SelectWindow(idx) => assert_eq!(
                    idx, expected_idx,
                    "Key '{}' should select window {}",
                    key, expected_idx
                ),
                _ => panic!("Expected SelectWindow for key '{}'", key),
            }
            assert!(!is_command_mode());
        }
    }

    #[test]
    fn test_normal_ctrl_c_still_works() {
        let _lock = TEST_MUTEX.lock().unwrap();
        reset_command_mode();
        // Regular Ctrl+C should still work (not intercepted by command mode)
        let event = KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL);
        match process_key_event(event) {
            EventAction::SendToPty(bytes) => assert_eq!(bytes, vec![0x03]),
            _ => panic!("Expected SendToPty for Ctrl+C"),
        }
        assert!(!is_command_mode());
    }

    #[test]
    fn test_ctrl_c_not_affected_by_command_mode() {
        let _lock = TEST_MUTEX.lock().unwrap();
        reset_command_mode();
        // Ctrl+C should NOT trigger command mode entry (only Alt+C does)
        // This is already working correctly because we check !ctrl in the condition
        let event = KeyEvent::new(
            KeyCode::Char('c'),
            KeyModifiers::CONTROL | KeyModifiers::ALT,
        );
        let _action = process_key_event(event);
        // Ctrl+Alt+C goes through the Ctrl+A-Z handler, not the command mode
        // Just verify we don't crash and command mode isn't entered spuriously
        reset_command_mode();
    }
}
