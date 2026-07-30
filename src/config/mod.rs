//! Configuration management for Clux.
//!
//! Loads keybindings and other settings from TOML config files.
//! Config is loaded from `~/.config/clux/config.toml` or `~/.cluxrc`.

use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

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

/// Root configuration structure.
#[derive(Debug, Deserialize, Clone)]
pub struct Config {
    #[serde(default)]
    pub prefix: PrefixConfig,
    #[serde(default)]
    pub keybindings: KeybindingsConfig,
    #[serde(default)]
    pub server: ServerLoggingConfig,
    #[serde(default)]
    pub links: LinksConfig,
    #[serde(default)]
    pub selection: SelectionConfig,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            prefix: PrefixConfig::default(),
            keybindings: KeybindingsConfig::default(),
            server: ServerLoggingConfig::default(),
            links: LinksConfig::default(),
            selection: SelectionConfig::default(),
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

mod keys;
mod sections;
#[cfg(test)]
mod tests;

pub use keys::*;
pub use sections::*;
