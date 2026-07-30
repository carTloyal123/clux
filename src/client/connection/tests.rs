//! Connection tests.

use super::*;
use crate::protocol::{read_message, write_message, PROTOCOL_VERSION};
use std::os::unix::net::UnixListener;
use std::thread;
use std::time::Duration;

fn temp_socket_path() -> std::path::PathBuf {
    let uid = unsafe { nix::libc::getuid() };
    let id = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::path::PathBuf::from(format!("/tmp/clux-test-{}-{}.sock", uid, id))
}

#[test]
fn test_connection_send_recv() {
    let socket_path = temp_socket_path();

    // Start a mock server
    let path_clone = socket_path.clone();
    let server_thread = thread::spawn(move || {
        let listener = UnixListener::bind(&path_clone).unwrap();
        let (mut stream, _) = listener.accept().unwrap();

        // Read Hello
        let msg: ClientMessage = read_message(&mut stream).unwrap();
        assert!(matches!(msg, ClientMessage::Hello { .. }));

        // Send HelloAck
        let response = ServerMessage::HelloAck {
            version: PROTOCOL_VERSION,
            server_pid: 12345,
        };
        write_message(&mut stream, &response).unwrap();
    });

    // Give the server time to start
    thread::sleep(Duration::from_millis(50));

    // Connect
    let mut conn = ServerConnection::connect(&socket_path).unwrap();

    // Send Hello
    let hello = ClientMessage::Hello {
        version: PROTOCOL_VERSION,
        term_cols: 80,
        term_rows: 24,
        term_type: "xterm".to_string(),
    };
    conn.send(&hello).unwrap();

    // Receive response
    let response = conn.recv().unwrap();
    assert!(matches!(
        response,
        ServerMessage::HelloAck {
            version: PROTOCOL_VERSION,
            ..
        }
    ));

    server_thread.join().unwrap();

    // Clean up
    let _ = std::fs::remove_file(&socket_path);
}

#[test]
fn test_connection_to_nonexistent() {
    let socket_path = temp_socket_path();

    let result = ServerConnection::connect(&socket_path);
    assert!(result.is_err());
}

#[test]
fn test_connection_multiple_messages() {
    let socket_path = temp_socket_path();

    // Start a mock server
    let path_clone = socket_path.clone();
    let server_thread = thread::spawn(move || {
        let listener = UnixListener::bind(&path_clone).unwrap();
        let (mut stream, _) = listener.accept().unwrap();

        // Echo back whatever we receive as Pong
        for _ in 0..3 {
            let _: ClientMessage = read_message(&mut stream).unwrap();
            write_message(&mut stream, &ServerMessage::Pong).unwrap();
        }
    });

    thread::sleep(Duration::from_millis(50));

    let mut conn = ServerConnection::connect(&socket_path).unwrap();

    // Send and receive multiple messages
    for _ in 0..3 {
        conn.send(&ClientMessage::Ping).unwrap();
        let response = conn.recv().unwrap();
        assert_eq!(response, ServerMessage::Pong);
    }

    server_thread.join().unwrap();
    let _ = std::fs::remove_file(&socket_path);
}
