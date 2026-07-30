//! Session and session-manager tests.

use super::*;

// Helper to create a session manager for testing
fn test_manager() -> SessionManager {
    SessionManager::new("/bin/sh".to_string())
}

// ------------------------------------------------------------------------
// Session Tests
// ------------------------------------------------------------------------

#[test]
fn test_session_manager_create() {
    let mut manager = test_manager();

    let id = manager
        .create_session(Some("work".to_string()), 80, 24)
        .unwrap();

    assert_eq!(manager.count(), 1);
    assert!(manager.get(id).is_some());
    assert!(manager.get_by_name("work").is_some());
}

#[test]
fn test_session_manager_auto_name() {
    let mut manager = test_manager();

    // First session gets "default"
    let id1 = manager.create_session(None, 80, 24).unwrap();
    assert_eq!(manager.get(id1).unwrap().name, "default");

    // Second session gets "session-1"
    let id2 = manager.create_session(None, 80, 24).unwrap();
    assert_eq!(manager.get(id2).unwrap().name, "session-1");

    // Third session gets "session-2"
    let id3 = manager.create_session(None, 80, 24).unwrap();
    assert_eq!(manager.get(id3).unwrap().name, "session-2");
}

#[test]
fn test_session_manager_get_or_create_default() {
    let mut manager = test_manager();

    // First call with no sessions creates the default session
    let id1 = manager.get_or_create_default(80, 24).unwrap();
    assert_eq!(manager.get(id1).unwrap().name, "default");
    assert_eq!(manager.count(), 1);

    // Second call returns the same session
    let id2 = manager.get_or_create_default(100, 50).unwrap();
    assert_eq!(id1, id2);
    assert_eq!(manager.count(), 1);
}

#[test]
fn test_session_manager_get_or_create_default_attaches_to_existing() {
    let mut manager = test_manager();

    // Create a named session first
    let work_id = manager
        .create_session(Some("work".to_string()), 80, 24)
        .unwrap();
    assert_eq!(manager.count(), 1);

    // get_or_create_default should attach to existing "work" session, not create "default"
    let id = manager.get_or_create_default(80, 24).unwrap();
    assert_eq!(id, work_id);
    assert_eq!(manager.count(), 1); // No new session created
}

#[test]
fn test_session_manager_get_or_create_default_picks_lowest_id() {
    let mut manager = test_manager();

    let first = manager
        .create_session(Some("first".to_string()), 80, 24)
        .unwrap();
    let second = manager
        .create_session(Some("second".to_string()), 80, 24)
        .unwrap();
    assert_ne!(first, second);

    // Remove the first to ensure IDs are sparse, then create another.
    assert!(manager.close_session(first));
    let third = manager
        .create_session(Some("third".to_string()), 80, 24)
        .unwrap();
    assert_ne!(second, third);

    // Should pick the lowest available ID among existing sessions.
    let id = manager.get_or_create_default(80, 24).unwrap();
    let expected = if second.0 < third.0 { second } else { third };
    assert_eq!(id, expected);
}

#[test]
fn test_session_manager_list_info() {
    let mut manager = test_manager();

    manager
        .create_session(Some("work".to_string()), 80, 24)
        .unwrap();
    manager
        .create_session(Some("personal".to_string()), 100, 50)
        .unwrap();

    let info = manager.list_info();

    assert_eq!(info.len(), 2);

    let names: Vec<_> = info.iter().map(|i| i.name.as_str()).collect();
    assert!(names.contains(&"work"));
    assert!(names.contains(&"personal"));
}

#[test]
fn test_session_manager_id_for_name() {
    let mut manager = test_manager();

    let id = manager
        .create_session(Some("test".to_string()), 80, 24)
        .unwrap();

    assert_eq!(manager.id_for_name("test"), Some(id));
    assert_eq!(manager.id_for_name("nonexistent"), None);
}
