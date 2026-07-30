//! Config parsing tests.

use super::*;
use crossterm::event::{KeyCode, KeyModifiers};

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
