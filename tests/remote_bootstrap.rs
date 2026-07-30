//! Remote SSH bootstrap and forwarding.

mod common;

use clux::client::Client;
use common::ssh::*;

// Remote SSH Tests
// ============================================================================

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
