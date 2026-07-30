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
