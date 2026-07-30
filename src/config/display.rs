//! Loading config from disk and printing it.

use std::path::PathBuf;

use super::{Config, ConfigError, ConfigSource};
use std::fs;
impl super::Config {
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
}
