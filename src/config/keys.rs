//! Parsing key specifications like "ctrl+shift+a".

use super::ConfigError;
use crossterm::event::{KeyCode, KeyModifiers};

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
