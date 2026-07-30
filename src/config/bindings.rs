//! Turning the keybinding config into an action lookup.

use std::collections::HashMap;

use super::{ConfigError, ParsedKey};
impl super::Config {
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
    pub(super) fn add_binding(
        bindings: &mut HashMap<ParsedKey, String>,
        key_str: &str,
        action: &str,
    ) {
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
