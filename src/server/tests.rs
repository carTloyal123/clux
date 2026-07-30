//! Server tests: handshake, sessions, and connection handling.

use super::*;
use crate::protocol::{read_message, write_message};
use crate::protocol::{ClientMessage, PROTOCOL_VERSION};
use std::os::unix::net::UnixStream;
use std::thread;
use std::time::Duration;

fn temp_socket_path() -> PathBuf {
    let uid = unsafe { libc::getuid() };
    let id = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    PathBuf::from(format!("/tmp/clux-test-{}-{}.sock", uid, id))
}

#[test]
fn test_server_config_default() {
    let config = ServerConfig::default();
    assert!(!config.shell.is_empty());
    assert!(config.socket_path.to_string_lossy().contains("clux"));
}

#[test]
fn test_default_socket_path() {
    let path = default_socket_path();
    assert!(path.to_string_lossy().contains("clux"));
    assert!(path.to_string_lossy().contains(".sock"));
}

#[test]
fn test_server_creation() {
    let socket_path = temp_socket_path();
    let config = ServerConfig {
        socket_path: socket_path.clone(),
        ..Default::default()
    };

    let server = Server::new(config);
    assert!(server.is_ok());

    let server = server.unwrap();
    assert_eq!(server.client_count(), 0);
    assert_eq!(server.session_count(), 0);

    // Clean up
    let _ = std::fs::remove_file(&socket_path);
}

#[test]
fn test_server_accepts_connection() {
    let socket_path = temp_socket_path();
    let config = ServerConfig {
        socket_path: socket_path.clone(),
        ..Default::default()
    };

    let mut server = Server::new(config).unwrap();

    // Connect a client
    let _client = UnixStream::connect(&socket_path).unwrap();

    // Process the accept
    server.accept_client().unwrap();

    assert_eq!(server.client_count(), 1);

    // Clean up
    let _ = std::fs::remove_file(&socket_path);
}

#[test]
fn test_client_hello_handshake() {
    let socket_path = temp_socket_path();
    let config = ServerConfig {
        socket_path: socket_path.clone(),
        ..Default::default()
    };

    let mut server = Server::new(config).unwrap();

    // Connect a client
    let mut client_stream = UnixStream::connect(&socket_path).unwrap();
    client_stream.set_nonblocking(false).unwrap();

    // Accept the connection
    server.accept_client().unwrap();
    assert_eq!(server.client_count(), 1);

    // Send Hello
    let hello = ClientMessage::Hello {
        version: PROTOCOL_VERSION,
        term_cols: 80,
        term_rows: 24,
        term_type: "xterm-256color".to_string(),
    };
    write_message(&mut client_stream, &hello).unwrap();

    // Process the message
    let client_id = ClientId(0);

    // Need to wait a bit for data to arrive
    thread::sleep(Duration::from_millis(10));

    server.handle_client_event(client_id).unwrap();

    // Read the response
    let response: ServerMessage = read_message(&mut client_stream).unwrap();

    match response {
        ServerMessage::HelloAck {
            version,
            server_pid,
        } => {
            assert_eq!(version, PROTOCOL_VERSION);
            assert!(server_pid > 0);
        }
        _ => panic!("Expected HelloAck, got {:?}", response),
    }

    // Clean up
    let _ = std::fs::remove_file(&socket_path);
}

#[test]
fn test_client_list_sessions() {
    let socket_path = temp_socket_path();
    let config = ServerConfig {
        socket_path: socket_path.clone(),
        ..Default::default()
    };

    let mut server = Server::new(config).unwrap();

    // Create a session
    server
        .sessions
        .create_session(Some("test-session".to_string()), 80, 24)
        .unwrap();

    // Connect a client
    let mut client_stream = UnixStream::connect(&socket_path).unwrap();
    client_stream.set_nonblocking(false).unwrap();

    // Accept and do handshake
    server.accept_client().unwrap();
    let hello = ClientMessage::Hello {
        version: PROTOCOL_VERSION,
        term_cols: 80,
        term_rows: 24,
        term_type: "xterm".to_string(),
    };
    write_message(&mut client_stream, &hello).unwrap();
    thread::sleep(Duration::from_millis(10));
    server.handle_client_event(ClientId(0)).unwrap();
    let _: ServerMessage = read_message(&mut client_stream).unwrap();

    // Request session list
    write_message(&mut client_stream, &ClientMessage::ListSessions).unwrap();
    thread::sleep(Duration::from_millis(10));
    server.handle_client_event(ClientId(0)).unwrap();

    let response: ServerMessage = read_message(&mut client_stream).unwrap();
    match response {
        ServerMessage::SessionList(sessions) => {
            assert_eq!(sessions.len(), 1);
            assert_eq!(sessions[0].name, "test-session");
        }
        _ => panic!("Expected SessionList, got {:?}", response),
    }

    // Clean up
    let _ = std::fs::remove_file(&socket_path);
}
