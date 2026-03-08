//! Configuration management for Clux.
//!
//! Loads keybindings and other settings from TOML config files.
//! Config is loaded from `~/.config/clux/config.toml` or `~/.cluxrc`.

use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

use crossterm::event::{KeyCode, KeyModifiers};
use serde::Deserialize;
use thiserror::Error;

/// Configuration error types.
#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("Invalid key specification: {0}")]
    InvalidKey(String),

    #[error("Failed to parse config file: {0}")]
    ParseError(#[from] toml::de::Error),

    #[error("Failed to read config file: {0}")]
    IoError(#[from] std::io::Error),
}

/// A parsed key with modifiers.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ParsedKey {
    pub code: KeyCode,
    pub modifiers: KeyModifiers,
}

impl ParsedKey {
    /// Parse a key string like "ctrl+shift+a" or "alt+c" into components.
    pub fn parse(s: &str) -> Result<Self, ConfigError> {
        let s = s.trim().to_lowercase();
        let parts: Vec<&str> = s.split('+').collect();
        let mut modifiers = KeyModifiers::NONE;
        let mut key_part: Option<&str> = None;

        for part in &parts {
            match *part {
                "ctrl" | "control" => modifiers |= KeyModifiers::CONTROL,
                "alt" | "option" => modifiers |= KeyModifiers::ALT,
                "shift" => modifiers |= KeyModifiers::SHIFT,
                "super" | "cmd" | "win" | "meta" => modifiers |= KeyModifiers::SUPER,
                other if other.is_empty() => return Err(ConfigError::InvalidKey(s)),
                other => {
                    if key_part.replace(other).is_some() {
                        return Err(ConfigError::InvalidKey(s));
                    }
                }
            }
        }

        let key_part = match key_part {
            Some(k) => k,
            None => return Err(ConfigError::InvalidKey(s)),
        };

        if key_part.is_empty() {
            return Err(ConfigError::InvalidKey(s));
        }

        let code = Self::parse_key_code(key_part)?;
        Ok(Self { code, modifiers })
    }

    /// Parse a key code string into a KeyCode.
    fn parse_key_code(s: &str) -> Result<KeyCode, ConfigError> {
        match s {
            // Special keys
            "enter" | "return" => Ok(KeyCode::Enter),
            "escape" | "esc" => Ok(KeyCode::Esc),
            "tab" => Ok(KeyCode::Tab),
            "backtab" => Ok(KeyCode::BackTab),
            "space" => Ok(KeyCode::Char(' ')),
            "backspace" | "bs" => Ok(KeyCode::Backspace),
            "delete" | "del" => Ok(KeyCode::Delete),
            "insert" | "ins" => Ok(KeyCode::Insert),

            // Navigation keys
            "up" => Ok(KeyCode::Up),
            "down" => Ok(KeyCode::Down),
            "left" => Ok(KeyCode::Left),
            "right" => Ok(KeyCode::Right),
            "home" => Ok(KeyCode::Home),
            "end" => Ok(KeyCode::End),
            "pageup" | "pgup" => Ok(KeyCode::PageUp),
            "pagedown" | "pgdn" => Ok(KeyCode::PageDown),

            // Function keys
            s if s.starts_with('f') && s.len() > 1 => {
                let num: u8 = s[1..]
                    .parse()
                    .map_err(|_| ConfigError::InvalidKey(s.to_string()))?;
                if num >= 1 && num <= 24 {
                    Ok(KeyCode::F(num))
                } else {
                    Err(ConfigError::InvalidKey(s.to_string()))
                }
            }

            // Single character
            s if s.len() == 1 => {
                let c = s.chars().next().unwrap();
                Ok(KeyCode::Char(c))
            }

            // Special single-char symbols that might be spelled out
            "minus" => Ok(KeyCode::Char('-')),
            "plus" => Ok(KeyCode::Char('+')),
            "equals" => Ok(KeyCode::Char('=')),
            "bracket_left" | "lbracket" => Ok(KeyCode::Char('[')),
            "bracket_right" | "rbracket" => Ok(KeyCode::Char(']')),
            "semicolon" => Ok(KeyCode::Char(';')),
            "quote" | "apostrophe" => Ok(KeyCode::Char('\'')),
            "comma" => Ok(KeyCode::Char(',')),
            "period" | "dot" => Ok(KeyCode::Char('.')),
            "slash" => Ok(KeyCode::Char('/')),
            "backslash" => Ok(KeyCode::Char('\\')),
            "grave" | "backtick" => Ok(KeyCode::Char('`')),

            _ => Err(ConfigError::InvalidKey(s.to_string())),
        }
    }

    /// Check if this key matches a crossterm KeyEvent (ignoring case for chars).
    pub fn matches(&self, code: KeyCode, modifiers: KeyModifiers) -> bool {
        // For character keys, compare case-insensitively
        let code_matches = match (&self.code, &code) {
            (KeyCode::Char(a), KeyCode::Char(b)) => a.eq_ignore_ascii_case(b),
            (a, b) => a == b,
        };
        code_matches && self.modifiers == modifiers
    }
}

/// Root configuration structure.
#[derive(Debug, Deserialize, Clone)]
pub struct Config {
    #[serde(default)]
    pub prefix: PrefixConfig,
    #[serde(default)]
    pub keybindings: KeybindingsConfig,
    #[serde(default)]
    pub server: ServerLoggingConfig,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            prefix: PrefixConfig::default(),
            keybindings: KeybindingsConfig::default(),
            server: ServerLoggingConfig::default(),
        }
    }
}

/// Server logging configuration.
#[derive(Debug, Deserialize, Clone)]
#[serde(default)]
pub struct ServerLoggingConfig {
    /// Log level: "error", "warn", "info", "debug", "trace"
    pub log_level: String,
    /// Directory for log files. Defaults to ~/.local/state/clux/
    /// Set to empty string "" to disable file logging (stderr only).
    pub log_dir: Option<String>,
}

impl Default for ServerLoggingConfig {
    fn default() -> Self {
        Self {
            log_level: "info".to_string(),
            log_dir: None, // Will use default_log_dir()
        }
    }
}

impl ServerLoggingConfig {
    /// Get the effective log directory, using the default if not configured.
    pub fn effective_log_dir(&self) -> Option<PathBuf> {
        match &self.log_dir {
            Some(dir) if dir.is_empty() => None, // Explicitly disabled
            Some(dir) => Some(PathBuf::from(shellexpand::tilde(dir).into_owned())),
            None => Self::default_log_dir(),
        }
    }

    /// Get the default log directory (~/.local/state/clux/).
    pub fn default_log_dir() -> Option<PathBuf> {
        // Use XDG state directory if available, otherwise ~/.local/state/clux/
        if let Some(state_dir) = dirs::state_dir() {
            Some(state_dir.join("clux"))
        } else if let Some(home) = dirs::home_dir() {
            Some(home.join(".local").join("state").join("clux"))
        } else {
            None
        }
    }
}

/// Prefix key configuration.
#[derive(Debug, Deserialize, Clone)]
pub struct PrefixConfig {
    #[serde(default = "default_prefix_key")]
    pub key: String,
}

fn default_prefix_key() -> String {
    "alt+c".to_string()
}

impl Default for PrefixConfig {
    fn default() -> Self {
        Self {
            key: "alt+c".to_string(),
        }
    }
}

/// All keybinding categories.
#[derive(Debug, Deserialize, Clone, Default)]
pub struct KeybindingsConfig {
    #[serde(default)]
    pub pane: PaneBindings,
    #[serde(default)]
    pub window: WindowBindings,
    #[serde(default)]
    pub app: AppBindings,
    #[serde(default)]
    pub direct: DirectBindings,
}

/// Pane management keybindings (used after prefix).
#[derive(Debug, Deserialize, Clone)]
#[serde(default)]
pub struct PaneBindings {
    pub split_horizontal: String,
    pub split_vertical: String,
    pub close: String,
    pub navigate_up: String,
    pub navigate_down: String,
    pub navigate_left: String,
    pub navigate_right: String,
    pub navigate_up_arrow: String,
    pub navigate_down_arrow: String,
    pub navigate_left_arrow: String,
    pub navigate_right_arrow: String,
}

impl Default for PaneBindings {
    fn default() -> Self {
        Self {
            split_horizontal: "-".to_string(),
            split_vertical: "p".to_string(),
            close: "w".to_string(),
            navigate_up: "k".to_string(),
            navigate_down: "j".to_string(),
            navigate_left: "h".to_string(),
            navigate_right: "l".to_string(),
            navigate_up_arrow: "up".to_string(),
            navigate_down_arrow: "down".to_string(),
            navigate_left_arrow: "left".to_string(),
            navigate_right_arrow: "right".to_string(),
        }
    }
}

/// Window management keybindings (used after prefix).
#[derive(Debug, Deserialize, Clone)]
#[serde(default)]
pub struct WindowBindings {
    pub new: String,
    pub close: String,
    pub next: String,
    pub previous: String,
    pub previous_alt: String,
    pub select_1: String,
    pub select_2: String,
    pub select_3: String,
    pub select_4: String,
    pub select_5: String,
    pub select_6: String,
    pub select_7: String,
    pub select_8: String,
    pub select_9: String,
    pub select_10: String,
}

impl Default for WindowBindings {
    fn default() -> Self {
        Self {
            new: "n".to_string(),
            close: "x".to_string(),
            next: "]".to_string(),
            previous: "'".to_string(),
            previous_alt: "[".to_string(),
            select_1: "1".to_string(),
            select_2: "2".to_string(),
            select_3: "3".to_string(),
            select_4: "4".to_string(),
            select_5: "5".to_string(),
            select_6: "6".to_string(),
            select_7: "7".to_string(),
            select_8: "8".to_string(),
            select_9: "9".to_string(),
            select_10: "0".to_string(),
        }
    }
}

/// Application-level keybindings (used after prefix).
#[derive(Debug, Deserialize, Clone)]
#[serde(default)]
pub struct AppBindings {
    pub quit: String,
    pub detach: String,
    pub send_prefix: String,
}

impl Default for AppBindings {
    fn default() -> Self {
        Self {
            quit: "q".to_string(),
            detach: "d".to_string(),
            send_prefix: "c".to_string(),
        }
    }
}

/// Direct keybindings (no prefix needed).
#[derive(Debug, Deserialize, Clone)]
#[serde(default)]
pub struct DirectBindings {
    pub scroll_up: String,
    pub scroll_down: String,
    pub paste: String,
    pub paste_alt: String,
}

impl Default for DirectBindings {
    fn default() -> Self {
        Self {
            scroll_up: "shift+pageup".to_string(),
            scroll_down: "shift+pagedown".to_string(),
            paste: "super+v".to_string(),
            paste_alt: "ctrl+shift+v".to_string(),
        }
    }
}

/// Source of configuration (for debugging).
#[derive(Debug, Clone)]
pub enum ConfigSource {
    /// Loaded from a file at the given path.
    File(PathBuf),
    /// Using built-in defaults.
    Default,
}

impl std::fmt::Display for ConfigSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ConfigSource::File(path) => write!(f, "{}", path.display()),
            ConfigSource::Default => write!(f, "(built-in defaults)"),
        }
    }
}

impl Config {
    /// Load configuration from file or return defaults.
    /// Returns both the config and its source.
    ///
    /// Search order:
    /// 1. ~/.config/clux/config.toml (XDG standard, works on all platforms)
    /// 2. Platform config dir (~/Library/Application Support/clux/config.toml on macOS)
    /// 3. ~/.cluxrc (classic dotfile fallback)
    pub fn load() -> (Self, ConfigSource) {
        // Try ~/.config/clux/config.toml first (XDG standard, cross-platform)
        if let Some(home_dir) = dirs::home_dir() {
            let config_path = home_dir.join(".config").join("clux").join("config.toml");
            if config_path.exists() {
                match Self::load_from_path(&config_path) {
                    Ok(config) => {
                        return (config, ConfigSource::File(config_path));
                    }
                    Err(e) => {
                        eprintln!("Warning: Failed to load {:?}: {}", config_path, e);
                    }
                }
            }
        }

        // Try platform-specific config directory (e.g., ~/Library/Application Support on macOS)
        if let Some(config_dir) = dirs::config_dir() {
            let config_path = config_dir.join("clux").join("config.toml");
            if config_path.exists() {
                match Self::load_from_path(&config_path) {
                    Ok(config) => {
                        return (config, ConfigSource::File(config_path));
                    }
                    Err(e) => {
                        eprintln!("Warning: Failed to load {:?}: {}", config_path, e);
                    }
                }
            }
        }

        // Try ~/.cluxrc fallback
        if let Some(home_dir) = dirs::home_dir() {
            let config_path = home_dir.join(".cluxrc");
            if config_path.exists() {
                match Self::load_from_path(&config_path) {
                    Ok(config) => {
                        return (config, ConfigSource::File(config_path));
                    }
                    Err(e) => {
                        eprintln!("Warning: Failed to load {:?}: {}", config_path, e);
                    }
                }
            }
        }

        (Self::default(), ConfigSource::Default)
    }

    /// Load configuration from a specific path.
    pub fn load_from_path(path: &PathBuf) -> Result<Self, ConfigError> {
        let contents = fs::read_to_string(path)?;
        let config: Config = toml::from_str(&contents)?;
        Ok(config)
    }

    /// Get the default configuration as a TOML string.
    pub fn default_toml() -> &'static str {
        DEFAULT_CONFIG
    }

    /// Display the configuration for debugging.
    pub fn display(&self, source: &ConfigSource) {
        println!("Clux Configuration");
        println!("==================");
        println!();
        println!("Source: {}", source);
        println!();
        println!("[prefix]");
        println!("  key = {:?}", self.prefix.key);
        println!();
        println!("[keybindings.pane]");
        println!(
            "  split_horizontal = {:?}",
            self.keybindings.pane.split_horizontal
        );
        println!(
            "  split_vertical = {:?}",
            self.keybindings.pane.split_vertical
        );
        println!("  close = {:?}", self.keybindings.pane.close);
        println!("  navigate_up = {:?}", self.keybindings.pane.navigate_up);
        println!(
            "  navigate_down = {:?}",
            self.keybindings.pane.navigate_down
        );
        println!(
            "  navigate_left = {:?}",
            self.keybindings.pane.navigate_left
        );
        println!(
            "  navigate_right = {:?}",
            self.keybindings.pane.navigate_right
        );
        println!();
        println!("[keybindings.window]");
        println!("  new = {:?}", self.keybindings.window.new);
        println!("  close = {:?}", self.keybindings.window.close);
        println!("  next = {:?}", self.keybindings.window.next);
        println!("  previous = {:?}", self.keybindings.window.previous);
        println!();
        println!("[keybindings.app]");
        println!("  quit = {:?}", self.keybindings.app.quit);
        println!("  send_prefix = {:?}", self.keybindings.app.send_prefix);
        println!();
        println!("[keybindings.direct]");
        println!("  scroll_up = {:?}", self.keybindings.direct.scroll_up);
        println!("  scroll_down = {:?}", self.keybindings.direct.scroll_down);
        println!("  paste = {:?}", self.keybindings.direct.paste);
        println!("  paste_alt = {:?}", self.keybindings.direct.paste_alt);
    }

    /// Build a lookup table mapping keys to action names for command mode.
    pub fn build_command_bindings(&self) -> HashMap<ParsedKey, String> {
        let mut bindings = HashMap::new();

        // Pane bindings
        Self::add_binding(
            &mut bindings,
            &self.keybindings.pane.split_horizontal,
            "split_horizontal",
        );
        Self::add_binding(
            &mut bindings,
            &self.keybindings.pane.split_vertical,
            "split_vertical",
        );
        Self::add_binding(&mut bindings, &self.keybindings.pane.close, "close_pane");
        Self::add_binding(
            &mut bindings,
            &self.keybindings.pane.navigate_up,
            "navigate_up",
        );
        Self::add_binding(
            &mut bindings,
            &self.keybindings.pane.navigate_down,
            "navigate_down",
        );
        Self::add_binding(
            &mut bindings,
            &self.keybindings.pane.navigate_left,
            "navigate_left",
        );
        Self::add_binding(
            &mut bindings,
            &self.keybindings.pane.navigate_right,
            "navigate_right",
        );
        Self::add_binding(
            &mut bindings,
            &self.keybindings.pane.navigate_up_arrow,
            "navigate_up",
        );
        Self::add_binding(
            &mut bindings,
            &self.keybindings.pane.navigate_down_arrow,
            "navigate_down",
        );
        Self::add_binding(
            &mut bindings,
            &self.keybindings.pane.navigate_left_arrow,
            "navigate_left",
        );
        Self::add_binding(
            &mut bindings,
            &self.keybindings.pane.navigate_right_arrow,
            "navigate_right",
        );

        // Window bindings
        Self::add_binding(&mut bindings, &self.keybindings.window.new, "new_window");
        Self::add_binding(
            &mut bindings,
            &self.keybindings.window.close,
            "close_window",
        );
        Self::add_binding(&mut bindings, &self.keybindings.window.next, "next_window");
        Self::add_binding(
            &mut bindings,
            &self.keybindings.window.previous,
            "prev_window",
        );
        Self::add_binding(
            &mut bindings,
            &self.keybindings.window.previous_alt,
            "prev_window",
        );
        Self::add_binding(
            &mut bindings,
            &self.keybindings.window.select_1,
            "select_window_1",
        );
        Self::add_binding(
            &mut bindings,
            &self.keybindings.window.select_2,
            "select_window_2",
        );
        Self::add_binding(
            &mut bindings,
            &self.keybindings.window.select_3,
            "select_window_3",
        );
        Self::add_binding(
            &mut bindings,
            &self.keybindings.window.select_4,
            "select_window_4",
        );
        Self::add_binding(
            &mut bindings,
            &self.keybindings.window.select_5,
            "select_window_5",
        );
        Self::add_binding(
            &mut bindings,
            &self.keybindings.window.select_6,
            "select_window_6",
        );
        Self::add_binding(
            &mut bindings,
            &self.keybindings.window.select_7,
            "select_window_7",
        );
        Self::add_binding(
            &mut bindings,
            &self.keybindings.window.select_8,
            "select_window_8",
        );
        Self::add_binding(
            &mut bindings,
            &self.keybindings.window.select_9,
            "select_window_9",
        );
        Self::add_binding(
            &mut bindings,
            &self.keybindings.window.select_10,
            "select_window_10",
        );

        // App bindings
        Self::add_binding(&mut bindings, &self.keybindings.app.quit, "quit");
        Self::add_binding(
            &mut bindings,
            &self.keybindings.app.send_prefix,
            "send_prefix",
        );

        bindings
    }

    /// Build a lookup table for direct bindings (no prefix needed).
    pub fn build_direct_bindings(&self) -> HashMap<ParsedKey, String> {
        let mut bindings = HashMap::new();

        Self::add_binding(
            &mut bindings,
            &self.keybindings.direct.scroll_up,
            "scroll_up",
        );
        Self::add_binding(
            &mut bindings,
            &self.keybindings.direct.scroll_down,
            "scroll_down",
        );
        Self::add_binding(&mut bindings, &self.keybindings.direct.paste, "paste");
        Self::add_binding(&mut bindings, &self.keybindings.direct.paste_alt, "paste");

        bindings
    }

    /// Parse the prefix key.
    pub fn parse_prefix(&self) -> Result<ParsedKey, ConfigError> {
        ParsedKey::parse(&self.prefix.key)
    }

    fn add_binding(bindings: &mut HashMap<ParsedKey, String>, key_str: &str, action: &str) {
        match ParsedKey::parse(key_str) {
            Ok(key) => {
                bindings.insert(key, action.to_string());
            }
            Err(e) => {
                log::warn!("Invalid keybinding '{}': {}", key_str, e);
            }
        }
    }
}

/// The default configuration file with all options documented.
pub const DEFAULT_CONFIG: &str = r#"# Clux Terminal Multiplexer Configuration
# ========================================
#
# This file documents ALL available configuration options.
# Edit any value to customize your setup.

# ==============================================================================
#                              SERVER SETTINGS
# ==============================================================================
# These settings control the clux server process.

[server]
# Log level: "error", "warn", "info", "debug", "trace"
log_level = "info"

# Directory for log files. The server writes to {log_dir}/clux-server.log
# Defaults to ~/.local/state/clux/ if not specified.
# Set to "" (empty string) to disable file logging and only log to stderr.
# log_dir = "~/.local/state/clux"

# ==============================================================================
#                              KEYBINDINGS
# ==============================================================================
#
# Key Syntax:
#   - Modifiers: ctrl, alt, shift, super (cmd on macOS)
#   - Separator: + (e.g., "ctrl+shift+c")
#   - Special keys: enter, escape, tab, space, backspace, delete
#   - Function keys: f1, f2, ... f12
#   - Navigation: up, down, left, right, home, end, pageup, pagedown
#   - Characters: a-z, 0-9, and symbols like -, [, ], ', etc.
#
# Examples:
#   "a"           - The 'a' key
#   "ctrl+c"      - Ctrl+C
#   "alt+enter"   - Alt+Enter
#   "super+v"     - Cmd+V (macOS) / Super+V (Linux)

# ==============================================================================
#                              COMMAND PREFIX
# ==============================================================================
# The prefix key enters "command mode" where the next key triggers an action.
# This is similar to tmux's prefix (Ctrl+B) or screen's (Ctrl+A).
#
# Default: Option+C (Alt+C on Linux)
# After pressing the prefix, press another key to execute a command.

[prefix]
key = "alt+c"

# ==============================================================================
#                             PANE MANAGEMENT
# ==============================================================================
# These keys work AFTER pressing the prefix key.
# Panes let you split your terminal into multiple views.

[keybindings.pane]
# Split the current pane into two
split_horizontal = "-"          # New pane below current
split_vertical = "p"            # New pane to the right

# Close the focused pane
close = "w"

# Navigate between panes (vim-style)
navigate_up = "k"
navigate_down = "j"
navigate_left = "h"
navigate_right = "l"

# Navigate between panes (arrow keys)
navigate_up_arrow = "up"
navigate_down_arrow = "down"
navigate_left_arrow = "left"
navigate_right_arrow = "right"

# ==============================================================================
#                            WINDOW MANAGEMENT
# ==============================================================================
# These keys work AFTER pressing the prefix key.
# Windows are like browser tabs - each has its own pane layout.

[keybindings.window]
# Create and close windows
new = "n"                       # Create a new window
close = "x"                     # Close the current window

# Navigate between windows
next = "]"                      # Switch to next window
previous = "'"                  # Switch to previous window
previous_alt = "["              # Alternative key for previous

# Jump directly to a window by number
select_1 = "1"                  # Switch to window 1
select_2 = "2"                  # Switch to window 2
select_3 = "3"                  # Switch to window 3
select_4 = "4"                  # Switch to window 4
select_5 = "5"                  # Switch to window 5
select_6 = "6"                  # Switch to window 6
select_7 = "7"                  # Switch to window 7
select_8 = "8"                  # Switch to window 8
select_9 = "9"                  # Switch to window 9
select_10 = "0"                 # Switch to window 10 (0 = 10)

# ==============================================================================
#                               APPLICATION
# ==============================================================================
# These keys work AFTER pressing the prefix key.

[keybindings.app]
quit = "q"                      # Exit Clux entirely
send_prefix = "c"               # Send the prefix key to the terminal
                                # (useful if an app needs Alt+C)

# ==============================================================================
#                            DIRECT KEYBINDINGS
# ==============================================================================
# These keys work WITHOUT pressing the prefix first.
# Use with caution to avoid conflicts with terminal applications.

[keybindings.direct]
# Scrollback navigation
scroll_up = "shift+pageup"      # Scroll up through history
scroll_down = "shift+pagedown"  # Scroll down through history

# Clipboard operations
paste = "super+v"               # Paste from clipboard (Cmd+V on macOS)
paste_alt = "ctrl+shift+v"      # Alternative paste binding
"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_simple_key() {
        let key = ParsedKey::parse("a").unwrap();
        assert_eq!(key.code, KeyCode::Char('a'));
        assert_eq!(key.modifiers, KeyModifiers::NONE);
    }

    #[test]
    fn test_parse_key_with_ctrl() {
        let key = ParsedKey::parse("ctrl+c").unwrap();
        assert_eq!(key.code, KeyCode::Char('c'));
        assert_eq!(key.modifiers, KeyModifiers::CONTROL);
    }

    #[test]
    fn test_parse_key_with_multiple_modifiers() {
        let key = ParsedKey::parse("ctrl+shift+a").unwrap();
        assert_eq!(key.code, KeyCode::Char('a'));
        assert_eq!(key.modifiers, KeyModifiers::CONTROL | KeyModifiers::SHIFT);
    }

    #[test]
    fn test_parse_key_alt_names() {
        // Test various modifier names
        let key = ParsedKey::parse("option+c").unwrap();
        assert_eq!(key.modifiers, KeyModifiers::ALT);

        let key = ParsedKey::parse("cmd+v").unwrap();
        assert_eq!(key.modifiers, KeyModifiers::SUPER);

        let key = ParsedKey::parse("control+x").unwrap();
        assert_eq!(key.modifiers, KeyModifiers::CONTROL);
    }

    #[test]
    fn test_parse_special_keys() {
        assert_eq!(ParsedKey::parse("enter").unwrap().code, KeyCode::Enter);
        assert_eq!(ParsedKey::parse("escape").unwrap().code, KeyCode::Esc);
        assert_eq!(ParsedKey::parse("tab").unwrap().code, KeyCode::Tab);
        assert_eq!(ParsedKey::parse("space").unwrap().code, KeyCode::Char(' '));
        assert_eq!(ParsedKey::parse("up").unwrap().code, KeyCode::Up);
        assert_eq!(ParsedKey::parse("pageup").unwrap().code, KeyCode::PageUp);
    }

    #[test]
    fn test_parse_function_keys() {
        assert_eq!(ParsedKey::parse("f1").unwrap().code, KeyCode::F(1));
        assert_eq!(ParsedKey::parse("f12").unwrap().code, KeyCode::F(12));
    }

    #[test]
    fn test_parse_symbols() {
        assert_eq!(ParsedKey::parse("-").unwrap().code, KeyCode::Char('-'));
        assert_eq!(ParsedKey::parse("[").unwrap().code, KeyCode::Char('['));
        assert_eq!(ParsedKey::parse("]").unwrap().code, KeyCode::Char(']'));
        assert_eq!(ParsedKey::parse("'").unwrap().code, KeyCode::Char('\''));
    }

    #[test]
    fn test_parse_case_insensitive() {
        let key1 = ParsedKey::parse("CTRL+A").unwrap();
        let key2 = ParsedKey::parse("ctrl+a").unwrap();
        assert_eq!(key1.code, key2.code);
        assert_eq!(key1.modifiers, key2.modifiers);
    }

    #[test]
    fn test_key_matches() {
        let key = ParsedKey::parse("ctrl+c").unwrap();
        assert!(key.matches(KeyCode::Char('c'), KeyModifiers::CONTROL));
        assert!(key.matches(KeyCode::Char('C'), KeyModifiers::CONTROL)); // Case insensitive
        assert!(!key.matches(KeyCode::Char('c'), KeyModifiers::NONE));
        assert!(!key.matches(KeyCode::Char('x'), KeyModifiers::CONTROL));
    }

    #[test]
    fn test_default_config_parses() {
        let config: Config = toml::from_str(DEFAULT_CONFIG).expect("Default config should parse");
        assert_eq!(config.prefix.key, "alt+c");
        assert_eq!(config.keybindings.pane.split_horizontal, "-");
    }

    #[test]
    fn test_config_default() {
        let config = Config::default();
        assert_eq!(config.prefix.key, "alt+c");
        assert_eq!(config.keybindings.window.new, "n");
    }

    #[test]
    fn test_build_command_bindings() {
        let config = Config::default();
        let bindings = config.build_command_bindings();

        // Check some expected bindings
        let split_h_key = ParsedKey::parse("-").unwrap();
        assert_eq!(
            bindings.get(&split_h_key),
            Some(&"split_horizontal".to_string())
        );

        let quit_key = ParsedKey::parse("q").unwrap();
        assert_eq!(bindings.get(&quit_key), Some(&"quit".to_string()));
    }

    #[test]
    fn test_build_direct_bindings() {
        let config = Config::default();
        let bindings = config.build_direct_bindings();

        let scroll_up_key = ParsedKey::parse("shift+pageup").unwrap();
        assert_eq!(bindings.get(&scroll_up_key), Some(&"scroll_up".to_string()));
    }

    #[test]
    fn test_parse_prefix() {
        let config = Config::default();
        let prefix = config.parse_prefix().unwrap();
        assert_eq!(prefix.code, KeyCode::Char('c'));
        assert_eq!(prefix.modifiers, KeyModifiers::ALT);
    }

    #[test]
    fn test_invalid_key() {
        assert!(ParsedKey::parse("").is_err());
        assert!(ParsedKey::parse("ctrl+").is_err());
        assert!(ParsedKey::parse("ctrl+a+b").is_err());
        assert!(ParsedKey::parse("invalidkey").is_err());
    }

    #[test]
    fn test_parse_trims_whitespace() {
        let key = ParsedKey::parse("  CTRL+X  ").unwrap();
        assert_eq!(key.code, KeyCode::Char('x'));
        assert_eq!(key.modifiers, KeyModifiers::CONTROL);
    }
}
