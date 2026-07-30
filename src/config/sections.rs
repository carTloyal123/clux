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

fn default_prefix_key() -> String {
    "alt+c".to_string()
}
