//! The configuration sections.

use std::path::PathBuf;

use serde::Deserialize;

/// Mouse selection configuration.
#[derive(Debug, Deserialize, Clone)]
#[serde(default)]
pub struct SelectionConfig {
    /// Copy to the clipboard as soon as the mouse button is released.
    ///
    /// Clux enables mouse reporting, so the host terminal's own selection is not
    /// available inside a session - selecting has to do something useful on its
    /// own. Set to false if you would rather copy explicitly.
    pub copy_on_select: bool,
}
/// Hyperlink configuration.
#[derive(Debug, Deserialize, Clone)]
#[serde(default)]
pub struct LinksConfig {
    /// Turn URL-shaped text into real OSC 8 hyperlinks for the host terminal.
    ///
    /// Host terminals match URLs against their own grid, where every row clux
    /// paints looks like a hard-wrapped line, so they cannot follow a URL that
    /// wraps inside a pane and will happily run a match across a pane divider.
    /// Clux is the only process that knows where its logical lines end, so it
    /// resolves those links itself. Set to false to leave detection to the host
    /// terminal.
    pub auto_detect: bool,
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
/// Prefix key configuration.
#[derive(Debug, Deserialize, Clone)]
pub struct PrefixConfig {
    #[serde(default = "default_prefix_key")]
    pub key: String,
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
/// Application-level keybindings (used after prefix).
#[derive(Debug, Deserialize, Clone)]
#[serde(default)]
pub struct AppBindings {
    pub quit: String,
    pub detach: String,
    pub send_prefix: String,
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

impl Default for SelectionConfig {
    fn default() -> Self {
        Self {
            copy_on_select: true,
        }
    }
}

impl Default for LinksConfig {
    fn default() -> Self {
        Self { auto_detect: true }
    }
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

impl Default for PrefixConfig {
    fn default() -> Self {
        Self {
            key: "alt+c".to_string(),
        }
    }
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

impl Default for AppBindings {
    fn default() -> Self {
        Self {
            quit: "q".to_string(),
            detach: "d".to_string(),
            send_prefix: "c".to_string(),
        }
    }
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

fn default_prefix_key() -> String {
    "alt+c".to_string()
}
