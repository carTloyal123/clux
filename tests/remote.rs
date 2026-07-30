//! Remote SSH bootstrap and forwarding.

mod common;

use clux::client::{Client, ClientConfig, ClientTarget};
use common::harness::*;
use common::ssh::*;
use std::path::PathBuf;
use std::thread;
use std::time::Duration;

// Remote SSH Tests
// ============================================================================

#[test]
fn test_remote_client_can_create_and_list_session_via_ssh() {
    let env = FakeSshEnv::new().expect("fake ssh env");

    let mut client = Client::connect(env.remote_config(80, 24), true).expect("remote connect");
    client
        .attach(Some("remote".to_string()), true)
        .expect("remote attach");
    client.detach().expect("remote detach");

    let mut list_client =
        Client::connect(env.remote_config(80, 24), false).expect("remote list connect");
    let sessions = list_client.list_sessions().expect("remote list sessions");

    assert!(sessions.iter().any(|session| session.name == "remote"));
    assert!(env.managed_binary_path().exists());
    assert_eq!(env.download_count(), 1);
}

#[test]
fn test_remote_client_can_kill_session_via_ssh() {
    let env = FakeSshEnv::new().expect("fake ssh env");

    let mut client = Client::connect(env.remote_config(80, 24), true).expect("remote connect");
    client
        .attach(Some("killme".to_string()), true)
        .expect("remote attach");
    client.detach().expect("remote detach");

    let mut admin = Client::connect(env.remote_config(80, 24), false).expect("remote admin");
    admin.kill_session("killme").expect("kill session");

    let mut check = Client::connect(env.remote_config(80, 24), false).expect("remote check");
    let sessions = check.list_sessions().expect("session list");
    assert!(!sessions.iter().any(|session| session.name == "killme"));
}

#[test]
fn test_remote_bootstrap_reuses_managed_install() {
    let env = FakeSshEnv::new().expect("fake ssh env");

    let mut first = Client::connect(env.remote_config(80, 24), true).expect("first connect");
    first
        .attach(Some("reuse".to_string()), true)
        .expect("first attach");
    first.detach().expect("first detach");

    assert!(env.managed_binary_path().exists());
    assert_eq!(env.download_count(), 1);

    let mut second = Client::connect(env.remote_config(80, 24), true).expect("second connect");
    second
        .attach(Some("reuse".to_string()), false)
        .expect("second attach");
    second.detach().expect("second detach");

    assert_eq!(
        env.download_count(),
        1,
        "bootstrap should reuse installed binary"
    );
}

#[test]
fn test_remote_bootstrap_fails_when_artifact_missing() {
    let env = FakeSshEnv::with_options(FakeSshOptions {
        artifact_present: false,
        ..FakeSshOptions::default()
    })
    .expect("fake ssh env");

    let err = match Client::connect(env.remote_config(80, 24), true) {
        Ok(_) => panic!("expected remote bootstrap to fail"),
        Err(err) => err,
    };
    assert!(matches!(
        err,
        clux::client::ClientError::RemoteArtifactUnavailable { .. }
    ));
}

#[test]
fn test_remote_bootstrap_fails_when_platform_unsupported() {
    let env = FakeSshEnv::with_options(FakeSshOptions {
        arch: "riscv64".to_string(),
        ..FakeSshOptions::default()
    })
    .expect("fake ssh env");

    let err = match Client::connect(env.remote_config(80, 24), true) {
        Ok(_) => panic!("expected unsupported platform failure"),
        Err(err) => err,
    };
    assert!(matches!(
        err,
        clux::client::ClientError::RemotePlatformUnsupported { .. }
    ));
}

#[test]
fn test_remote_bootstrap_fails_without_downloader() {
    let env = FakeSshEnv::with_options(FakeSshOptions {
        downloader: FakeDownloader::None,
        ..FakeSshOptions::default()
    })
    .expect("fake ssh env");

    let err = match Client::connect(env.remote_config(80, 24), true) {
        Ok(_) => panic!("expected missing downloader failure"),
        Err(err) => err,
    };
    assert!(matches!(
        err,
        clux::client::ClientError::RemoteMissingDownloadTool
    ));
}

#[test]
fn test_remote_connect_without_autostart_does_not_bootstrap() {
    let env = FakeSshEnv::new().expect("fake ssh env");

    let err = match Client::connect(env.remote_config(80, 24), false) {
        Ok(_) => panic!("expected connection failure without bootstrap"),
        Err(err) => err,
    };
    assert!(matches!(
        err,
        clux::client::ClientError::ConnectionFailed(_)
    ));
    assert!(!env.install_root().exists());
    assert_eq!(env.download_count(), 0);
}

#[test]
fn test_remote_bootstrap_works_with_wget_fallback() {
    let env = FakeSshEnv::with_options(FakeSshOptions {
        downloader: FakeDownloader::Wget,
        ..FakeSshOptions::default()
    })
    .expect("fake ssh env");

    let mut client = Client::connect(env.remote_config(80, 24), true).expect("remote connect");
    client
        .attach(Some("wget".to_string()), true)
        .expect("remote attach");
    client.detach().expect("remote detach");

    assert!(env.managed_binary_path().exists());
    assert_eq!(env.download_count(), 1);
}

#[test]
fn test_remote_cli_info_and_kill_server() {
    let env = FakeSshEnv::new().expect("fake ssh env");

    let mut client = Client::connect(env.remote_config(80, 24), true).expect("remote connect");
    client
        .attach(Some("info".to_string()), true)
        .expect("remote attach");
    client.detach().expect("remote detach");

    let info_output = env
        .clux_command()
        .args([
            "info",
            "--remote",
            "fakehost",
            "--socket",
            env.remote_socket().to_str().unwrap(),
        ])
        .output()
        .expect("run clux info");
    assert!(info_output.status.success());
    let info_stdout = String::from_utf8_lossy(&info_output.stdout);
    assert!(info_stdout.contains("Server: running"));
    assert!(info_stdout.contains("Mode: remote"));
    assert!(info_stdout.contains("Remote: fakehost"));

    let kill_output = env
        .clux_command()
        .args([
            "kill-server",
            "--remote",
            "fakehost",
            "--socket",
            env.remote_socket().to_str().unwrap(),
        ])
        .output()
        .expect("run clux kill-server");
    assert!(kill_output.status.success());
    let kill_stdout = String::from_utf8_lossy(&kill_output.stdout);
    assert!(kill_stdout.contains("Server stopped"));

    let info_after = env
        .clux_command()
        .args([
            "info",
            "--remote",
            "fakehost",
            "--socket",
            env.remote_socket().to_str().unwrap(),
        ])
        .output()
        .expect("run clux info after shutdown");
    assert!(info_after.status.success());
    let info_after_stdout = String::from_utf8_lossy(&info_after.stdout);
    assert!(info_after_stdout.contains("Server: not running"));
}

#[test]
fn test_remote_tunnel_cleanup_removes_forwarded_socket() {
    let env = FakeSshEnv::new().expect("fake ssh env");
    let before = temp_forward_socket_paths();

    {
        let mut client = Client::connect(env.remote_config(80, 24), true).expect("remote connect");
        client
            .attach(Some("cleanup".to_string()), true)
            .expect("remote attach");
        client.detach().expect("remote detach");
    }

    let after = temp_forward_socket_paths();
    assert_eq!(
        before, after,
        "forwarded ssh socket should be removed after client drop"
    );
}

// ============================================================================
// Debug Test (run with --ignored --nocapture)
// ============================================================================

#[test]
#[ignore]
fn test_debug_dump() {
    let mut client = TestClient::new()
        .size(80, 24)
        .timeout(Duration::from_secs(10))
        .build()
        .expect("Failed to create test client");

    client.split_vertical();
    client.wait_for_update().ok();
    client.split_horizontal();
    client.wait_for_update().ok();

    std::thread::sleep(Duration::from_millis(500));
    client.drain_messages().ok();

    println!("\n{}", client.dump_screen());
    println!("\n=== Server Log (last 30 lines) ===");
    println!("{}", client.dump_server_log(30));
}
