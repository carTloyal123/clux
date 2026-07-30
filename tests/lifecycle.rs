//! Server lifecycle: spawn, detach, reattach, shutdown.

mod common;

use clux::client::Client;
use clux::protocol::{CommandAction, ServerMessage};
use common::harness::*;
use std::path::PathBuf;
use std::thread;
use std::time::Duration;

// Server Lifecycle Tests
// ============================================================================
//
// The contract: a client starts the server on first use, the server outlives a
// detach so sessions can be reattached, and it shuts itself down once the last
// session is gone.

#[test]
fn test_client_starts_the_server_on_first_use() {
    let socket_path = unique_socket_path();
    assert!(!socket_path.exists(), "socket should not exist yet");

    // The client looks for clux-server beside its own executable and otherwise on
    // PATH. Under `cargo test` the running executable is the test binary in
    // deps/, so PATH is what resolves it.
    let _env_guard = SSH_ENV_LOCK.lock().unwrap();
    let bin_dir = PathBuf::from(env!("CARGO_BIN_EXE_clux-server"))
        .parent()
        .expect("bin dir")
        .to_path_buf();
    let original_path = std::env::var("PATH").unwrap_or_default();
    std::env::set_var("PATH", format!("{}:{}", bin_dir.display(), original_path));

    let client = attach_client(&socket_path, "spawned", true, true);
    std::env::set_var("PATH", original_path);

    let mut client = client.expect("client should have started a server and attached");
    assert!(
        socket_path.exists(),
        "server socket should exist after the client started it"
    );
    assert!(client.is_attached());

    // The server we started is nobody else's to clean up.
    let _ = client.shutdown_server();
    thread::sleep(Duration::from_millis(300));
    let _ = std::fs::remove_file(&socket_path);
}

#[test]
fn test_server_survives_detach_and_session_reattaches() {
    let socket_path = unique_socket_path();
    // Auto-shutdown left on: a detached session must keep the server alive.
    let mut server = start_server_with_auto_exit(&socket_path, true).expect("start server");
    wait_for_socket(&socket_path, Duration::from_secs(5)).expect("socket");

    {
        let mut first = attach_client(&socket_path, "persistent", true, false).expect("attach");
        drain_for(&mut first, Duration::from_millis(500));
        first
            .send_input(b"echo SURVIVED_DETACH\n".to_vec())
            .expect("send input");
        wait_for_text_on(&mut first, "SURVIVED_DETACH", Duration::from_secs(10))
            .expect("should see output before detaching");
        first.detach().expect("detach");
    }

    // Well past the auto-shutdown grace period: the session still exists, so the
    // server must not have exited.
    thread::sleep(Duration::from_secs(2));
    assert!(
        matches!(server.try_wait(), Ok(None)),
        "server exited while a detached session still existed"
    );

    let mut second =
        attach_client(&socket_path, "persistent", false, false).expect("reattach to session");
    wait_for_text_on(&mut second, "SURVIVED_DETACH", Duration::from_secs(10))
        .expect("reattached session should still hold its output");

    let _ = second.shutdown_server();
    let _ = server.kill();
    let _ = server.wait();
    let _ = std::fs::remove_file(&socket_path);
}

#[test]
fn test_server_exits_after_last_session_closes() {
    let socket_path = unique_socket_path();
    let mut server = start_server_with_auto_exit(&socket_path, true).expect("start server");
    wait_for_socket(&socket_path, Duration::from_secs(5)).expect("socket");

    {
        let mut client = attach_client(&socket_path, "transient", true, false).expect("attach");
        drain_for(&mut client, Duration::from_millis(500));

        // Quit closes the session, unlike detach.
        client.send_command(CommandAction::Quit).expect("quit");
        drain_for(&mut client, Duration::from_millis(300));
    }

    assert!(
        wait_for_exit(&mut server, Duration::from_secs(10)),
        "server should shut down once its last session closed"
    );

    let _ = std::fs::remove_file(&socket_path);
}

#[test]
fn test_server_exits_when_the_last_shell_exits() {
    // The everyday path: no quit command, the user just types `exit`.
    let socket_path = unique_socket_path();
    let mut server = start_server_with_auto_exit(&socket_path, true).expect("start server");
    wait_for_socket(&socket_path, Duration::from_secs(5)).expect("socket");

    {
        let mut client = attach_client(&socket_path, "shell-exit", true, false).expect("attach");
        drain_for(&mut client, Duration::from_millis(500));
        client.send_input(b"exit\n".to_vec()).expect("send exit");
        drain_for(&mut client, Duration::from_millis(500));
    }

    assert!(
        wait_for_exit(&mut server, Duration::from_secs(10)),
        "server should shut down after the last shell exited"
    );

    let _ = std::fs::remove_file(&socket_path);
}

// ============================================================================
