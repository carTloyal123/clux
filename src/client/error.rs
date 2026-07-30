//! Client error type.

use std::io;

use crate::protocol::ServerMessage;

/// Client error type.
#[derive(Debug, thiserror::Error)]
pub enum ClientError {
    #[error("IO error: {0}")]
    Io(#[from] io::Error),

    #[error("Protocol error: {0}")]
    Protocol(#[from] crate::protocol::ProtocolError),

    #[error("Connection failed after {0} attempts")]
    ConnectionFailed(u32),

    #[error("Handshake failed: {0}")]
    HandshakeFailed(String),

    #[error("Session not found: {0}")]
    SessionNotFound(String),

    #[error("Server error: {0}")]
    ServerError(String),

    #[error("Unexpected response: {0:?}")]
    UnexpectedResponse(ServerMessage),

    #[error("ssh is required for --remote mode but was not found in PATH")]
    SshUnavailable,

    #[error("Failed to start remote server: {0}")]
    RemoteStartupFailed(String),

    #[error("SSH tunnel failed: {0}")]
    RemoteTunnelFailed(String),

    #[error("Unsupported remote platform: {os}/{arch}")]
    RemotePlatformUnsupported { os: String, arch: String },

    #[error("No release artifact found for clux-server v{version} ({target}) at {url}")]
    RemoteArtifactUnavailable {
        version: String,
        target: String,
        url: String,
    },

    #[error("Remote bootstrap failed: {0}")]
    RemoteBootstrapFailed(String),

    #[error("Neither curl nor wget is available on the remote host")]
    RemoteMissingDownloadTool,

    #[error("Invalid repository metadata for remote bootstrap: {0}")]
    InvalidRepositoryMetadata(String),

    #[error(
        "Server protocol version {actual} does not support this operation (requires {required})"
    )]
    UnsupportedServerVersion { required: u32, actual: u32 },
}

/// Result type for client operations.
pub type ClientResult<T> = Result<T, ClientError>;
