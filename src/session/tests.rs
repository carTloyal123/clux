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
fn test_session_attach_detach() {
    let mut manager = test_manager();
    let session_id = manager
        .create_session(Some("test".to_string()), 80, 24)
        .unwrap();
    let session = manager.get_mut(session_id).unwrap();

    let client1 = ClientId(1);
    let client2 = ClientId(2);

    // Initially no clients
    assert!(!session.has_clients());
    assert_eq!(session.client_count(), 0);

    // Attach first client
    assert!(session.attach_client(client1));
    assert!(session.has_clients());
    assert_eq!(session.client_count(), 1);

    // Attach same client again (should return false)
    assert!(!session.attach_client(client1));
    assert_eq!(session.client_count(), 1);

    // Attach second client
    assert!(session.attach_client(client2));
    assert_eq!(session.client_count(), 2);

    // Detach first client
    assert!(session.detach_client(client1));
    assert_eq!(session.client_count(), 1);
    assert!(session.has_clients());

    // Detach same client again (should return false)
    assert!(!session.detach_client(client1));

    // Detach second client
    assert!(session.detach_client(client2));
    assert!(!session.has_clients());
}

#[test]
fn test_session_info() {
    let mut manager = test_manager();
    let session_id = manager
        .create_session(Some("my-session".to_string()), 80, 24)
        .unwrap();
    let session = manager.get(session_id).unwrap();

    let info = session.info();

    assert_eq!(info.id, session_id.0);
    assert_eq!(info.name, "my-session");
    assert_eq!(info.windows, 1); // Initial window
    assert_eq!(info.attached_clients, 0);
    assert!(info.created_at > 0);
}

#[test]
fn test_session_effective_size() {
    let mut manager = test_manager();
    let session_id = manager
        .create_session(Some("test".to_string()), 100, 50)
        .unwrap();
    let session = manager.get_mut(session_id).unwrap();

    let client1 = ClientId(1);
    let client2 = ClientId(2);
    session.attach_client(client1);
    session.attach_client(client2);

    let mut client_sizes = HashMap::new();
    client_sizes.insert(client1, (120, 40));
    client_sizes.insert(client2, (80, 30));

    // Should use smallest of each dimension
    let (cols, rows) = session.effective_size(&client_sizes);
    assert_eq!(cols, 80);
    assert_eq!(rows, 30);
}

#[test]
fn test_session_effective_size_no_clients() {
    let mut manager = test_manager();
    let session_id = manager
        .create_session(Some("test".to_string()), 100, 50)
        .unwrap();
    let session = manager.get(session_id).unwrap();

    let client_sizes = HashMap::new();

    // Should use current window manager size
    let (cols, rows) = session.effective_size(&client_sizes);
    assert_eq!(cols, 100);
    assert_eq!(rows, 50);
}

// ------------------------------------------------------------------------
// SessionManager Tests
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
