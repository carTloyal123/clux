//! Configuration management for Clux.
//!
//! Loads keybindings and other settings from TOML config files.
//! Config is loaded from `~/.config/clux/config.toml` or `~/.cluxrc`.

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

impl Config {}

mod bindings;
mod default_config;
mod display;
mod keybindings;
mod keys;
mod sections;
#[cfg(test)]
mod tests;

pub use default_config::DEFAULT_CONFIG;
pub use keybindings::*;
pub use keys::*;
pub use sections::*;
