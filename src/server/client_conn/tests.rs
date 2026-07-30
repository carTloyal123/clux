//! Tests for client conn.

use super::*;
use crate::protocol::{read_message, write_message, PROTOCOL_VERSION};
use std::io::Write;
use std::os::unix::net::UnixStream;

fn create_test_pair() -> (ClientConnection, UnixStream) {
    let (server_stream, client_stream) = UnixStream::pair().unwrap();
    let conn = ClientConnection::new(ClientId(0), server_stream);
    (conn, client_stream)
}

#[test]
fn test_client_connection_creation() {
    let (conn, _client) = create_test_pair();

    assert_eq!(conn.id, ClientId(0));
    assert_eq!(conn.state, ClientState::Connected);
    assert!(conn.is_alive());
}

#[test]
fn test_client_send_message() {
    let (mut conn, mut client) = create_test_pair();
    client.set_nonblocking(false).unwrap();

    // Send a message
    let msg = ServerMessage::Pong;
    conn.send_message(&msg).unwrap();

    // Read it on the client side
    let received: ServerMessage = read_message(&mut client).unwrap();
    assert_eq!(received, ServerMessage::Pong);
}

#[test]
fn test_client_receive_message() {
    let (mut conn, mut client) = create_test_pair();
    client.set_nonblocking(false).unwrap();

    // Send a message from client
    let msg = ClientMessage::Ping;
    write_message(&mut client, &msg).unwrap();

    // Receive it on the server side
    // Need a small delay for the data to arrive
    std::thread::sleep(std::time::Duration::from_millis(10));

    let received = conn.try_read_message().unwrap();
    assert_eq!(received, Some(ClientMessage::Ping));
}

#[test]
fn test_client_partial_message() {
    let (mut conn, mut client) = create_test_pair();
    client.set_nonblocking(false).unwrap();

    // Serialize a message
    let msg = ClientMessage::Hello {
        version: PROTOCOL_VERSION,
        term_cols: 80,
        term_rows: 24,
        term_type: "xterm".to_string(),
    };
    let mut buf = Vec::new();
    write_message(&mut buf, &msg).unwrap();

    // Send first half
    client.write_all(&buf[..buf.len() / 2]).unwrap();
    std::thread::sleep(std::time::Duration::from_millis(10));

    // Should return None (incomplete)
    let result = conn.try_read_message().unwrap();
    assert!(result.is_none());

    // Send second half
    client.write_all(&buf[buf.len() / 2..]).unwrap();
    std::thread::sleep(std::time::Duration::from_millis(10));

    // Should return the complete message
    let result = conn.try_read_message().unwrap();
    assert_eq!(result, Some(msg));
}

#[test]
fn test_client_connection_closed() {
    let (mut conn, client) = create_test_pair();

    // Close the client side
    drop(client);

    // Try to read - should get ConnectionClosed
    std::thread::sleep(std::time::Duration::from_millis(10));
    let result = conn.try_read_message();
    assert!(matches!(result, Err(ProtocolError::ConnectionClosed)));
    assert!(!conn.is_alive());
}

#[test]
fn test_client_state_transitions() {
    let (mut conn, _client) = create_test_pair();

    assert_eq!(conn.state, ClientState::Connected);

    conn.state = ClientState::Ready;
    assert_eq!(conn.state, ClientState::Ready);

    let session_id = crate::session::SessionId(1);
    conn.state = ClientState::Attached(session_id);
    assert_eq!(conn.state, ClientState::Attached(session_id));
}
