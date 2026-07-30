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
fn test_session_manager_name_collision() {
    let mut manager = test_manager();

    manager
        .create_session(Some("work".to_string()), 80, 24)
        .unwrap();

    // Try to create another with same name
    let result = manager.create_session(Some("work".to_string()), 80, 24);

    assert!(matches!(result, Err(SessionError::NameExists(_))));
}

#[test]
fn test_session_manager_rejects_empty_or_control_names() {
    let mut manager = test_manager();

    let empty = manager.create_session(Some("   ".to_string()), 80, 24);
    assert!(matches!(empty, Err(SessionError::InvalidName(_))));

    let control = manager.create_session(Some("bad\nname".to_string()), 80, 24);
    assert!(matches!(control, Err(SessionError::InvalidName(_))));
}

#[test]
fn test_session_manager_trims_name_on_create() {
    let mut manager = test_manager();

    let id = manager
        .create_session(Some("  work  ".to_string()), 80, 24)
        .unwrap();
    assert_eq!(manager.get(id).unwrap().name, "work");
    assert!(manager.get_by_name("work").is_some());
}

#[test]
fn test_session_manager_close() {
    let mut manager = test_manager();

    let id = manager
        .create_session(Some("temp".to_string()), 80, 24)
        .unwrap();
    assert_eq!(manager.count(), 1);

    assert!(manager.close_session(id));
    assert_eq!(manager.count(), 0);
    assert!(manager.get(id).is_none());
    assert!(manager.get_by_name("temp").is_none());

    // Closing again should return false
    assert!(!manager.close_session(id));
}

#[test]
fn test_session_manager_close_by_name() {
    let mut manager = test_manager();

    manager
        .create_session(Some("temp".to_string()), 80, 24)
        .unwrap();

    assert!(manager.close_session_by_name("temp"));
    assert_eq!(manager.count(), 0);

    // Closing again should return false
    assert!(!manager.close_session_by_name("temp"));
}

#[test]
fn test_session_manager_rename() {
    let mut manager = test_manager();

    let id = manager
        .create_session(Some("old-name".to_string()), 80, 24)
        .unwrap();

    manager.rename_session(id, "new-name".to_string()).unwrap();

    assert!(manager.get_by_name("old-name").is_none());
    assert!(manager.get_by_name("new-name").is_some());
    assert_eq!(manager.get(id).unwrap().name, "new-name");
}

#[test]
fn test_session_manager_rename_same_name_is_noop() {
    let mut manager = test_manager();

    let id = manager
        .create_session(Some("same".to_string()), 80, 24)
        .unwrap();

    assert!(manager.rename_session(id, "same".to_string()).is_ok());
    assert_eq!(manager.get(id).unwrap().name, "same");
}

#[test]
fn test_session_manager_rename_trims_and_validates() {
    let mut manager = test_manager();

    let id = manager
        .create_session(Some("old".to_string()), 80, 24)
        .unwrap();

    manager
        .rename_session(id, "  new-name  ".to_string())
        .unwrap();
    assert!(manager.get_by_name("old").is_none());
    assert!(manager.get_by_name("new-name").is_some());

    let invalid = manager.rename_session(id, " \n ".to_string());
    assert!(matches!(invalid, Err(SessionError::InvalidName(_))));
}

#[test]
fn test_session_manager_rename_collision() {
    let mut manager = test_manager();

    let id1 = manager
        .create_session(Some("session1".to_string()), 80, 24)
        .unwrap();
    let _id2 = manager
        .create_session(Some("session2".to_string()), 80, 24)
        .unwrap();

    let result = manager.rename_session(id1, "session2".to_string());

    assert!(matches!(result, Err(SessionError::NameExists(_))));
}
