//! Tests for listener.

use super::*;
use std::time::{SystemTime, UNIX_EPOCH};

fn temp_socket_path() -> PathBuf {
    let uid = unsafe { libc::getuid() };
    let id = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    PathBuf::from(format!("/tmp/clux-test-{}-{}.sock", uid, id))
}

#[test]
fn test_listener_bind() {
    let path = temp_socket_path();

    let listener = SocketListener::bind(&path);
    assert!(listener.is_ok());

    let listener = listener.unwrap();
    assert!(path.exists());
    assert!(path.with_extension("lock").exists());

    drop(listener);

    // Files should be cleaned up
    assert!(!path.exists());
    assert!(!path.with_extension("lock").exists());
}

#[test]
fn test_listener_prevents_duplicate() {
    let path = temp_socket_path();

    let listener1 = SocketListener::bind(&path).unwrap();

    // Second bind should fail
    let listener2 = SocketListener::bind(&path);
    assert!(listener2.is_err());

    drop(listener1);

    // Now it should work
    let listener3 = SocketListener::bind(&path);
    assert!(listener3.is_ok());
}

#[test]
fn test_listener_accept() {
    let path = temp_socket_path();
    let listener = SocketListener::bind(&path).unwrap();

    // Connect a client
    let _client = UnixStream::connect(&path).unwrap();

    // Accept should succeed
    let result = listener.accept();
    assert!(result.is_ok());
}

#[test]
fn test_listener_accept_nonblocking() {
    let path = temp_socket_path();
    let listener = SocketListener::bind(&path).unwrap();

    // Accept with no pending connection should return WouldBlock
    let result = listener.accept();
    assert!(result.is_err());
    assert_eq!(result.unwrap_err().kind(), ErrorKind::WouldBlock);
}

#[test]
fn test_stale_lock_detection() {
    let path = temp_socket_path();
    let lock_path = path.with_extension("lock");

    // Create parent dir
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).unwrap();
    }

    // Create a lock file with a non-existent PID
    fs::write(&lock_path, "999999999").unwrap();

    // Should detect as stale
    assert!(SocketListener::is_lock_stale(&lock_path, &path));

    // Clean up
    let _ = fs::remove_file(&lock_path);
}

#[test]
fn test_lock_with_current_pid() {
    let path = temp_socket_path();
    let lock_path = path.with_extension("lock");

    // Create parent dir
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).unwrap();
    }

    // Create a lock file with current PID (but no socket)
    fs::write(&lock_path, format!("{}", std::process::id())).unwrap();

    // Should detect as stale since socket doesn't exist
    assert!(SocketListener::is_lock_stale(&lock_path, &path));

    // Clean up
    let _ = fs::remove_file(&lock_path);
}
