//! Unix socket listener for the server.
//!
//! Handles socket creation, binding, and accepting connections.
//! Includes lock file management to prevent multiple servers.

use std::fs;
use std::io::{self, ErrorKind};
use std::os::unix::io::{AsRawFd, RawFd};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};

use nix::libc;

/// A Unix socket listener with lock file support.
pub struct SocketListener {
    /// The underlying listener.
    listener: UnixListener,
    /// Path to the socket file.
    socket_path: PathBuf,
    /// Path to the lock file.
    lock_path: PathBuf,
}

impl SocketListener {
    /// Bind to a Unix socket at the given path.
    ///
    /// Creates a lock file to prevent multiple servers from binding
    /// to the same socket.
    pub fn bind(path: &Path) -> io::Result<Self> {
        let socket_path = path.to_path_buf();
        let lock_path = path.with_extension("lock");

        // Create parent directory if needed
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }

        // Try to acquire lock
        // We use a simple approach: create an exclusive lock file
        // A more robust approach would use flock(), but this works for our purposes
        if lock_path.exists() {
            // Check if the lock is stale (no process holding it)
            if Self::is_lock_stale(&lock_path, &socket_path) {
                log::info!("Removing stale lock file: {:?}", lock_path);
                let _ = fs::remove_file(&lock_path);
                let _ = fs::remove_file(&socket_path);
            } else {
                return Err(io::Error::new(
                    ErrorKind::AddrInUse,
                    format!("Server already running (lock file exists: {:?})", lock_path),
                ));
            }
        }

        // Remove old socket if it exists
        if socket_path.exists() {
            fs::remove_file(&socket_path)?;
        }

        // Create lock file with our PID
        fs::write(&lock_path, format!("{}", std::process::id()))?;

        // Bind the socket
        let listener = UnixListener::bind(&socket_path)?;

        // Set non-blocking mode
        listener.set_nonblocking(true)?;

        log::info!("Socket listener bound to {:?}", socket_path);

        Ok(Self {
            listener,
            socket_path,
            lock_path,
        })
    }

    /// Accept a new connection.
    ///
    /// Returns the stream on success, or WouldBlock if no connection pending.
    pub fn accept(&self) -> io::Result<UnixStream> {
        let (stream, _addr) = self.listener.accept()?;
        Ok(stream)
    }

    /// Get the socket path.
    pub fn socket_path(&self) -> &Path {
        &self.socket_path
    }

    /// Check if a lock file is stale (process no longer running).
    fn is_lock_stale(lock_path: &Path, socket_path: &Path) -> bool {
        // Read the PID from the lock file
        if let Ok(contents) = fs::read_to_string(lock_path) {
            if let Ok(pid) = contents.trim().parse::<i32>() {
                // Check if the process is running
                // kill(pid, 0) returns 0 if process exists, -1 otherwise
                let exists = unsafe { libc::kill(pid, 0) } == 0;
                if exists {
                    // Process exists, check if it's actually our server
                    // by trying to connect to the socket
                    if socket_path.exists() {
                        if UnixStream::connect(socket_path).is_ok() {
                            // Socket responds, server is running
                            return false;
                        }
                    }
                    // Process exists but socket doesn't work - stale
                    return true;
                }
            }
        }
        // Can't read PID or process doesn't exist - stale
        true
    }
}

impl AsRawFd for SocketListener {
    fn as_raw_fd(&self) -> RawFd {
        self.listener.as_raw_fd()
    }
}

impl Drop for SocketListener {
    fn drop(&mut self) {
        // Clean up socket and lock files
        if let Err(e) = fs::remove_file(&self.socket_path) {
            log::warn!("Failed to remove socket file: {}", e);
        }
        if let Err(e) = fs::remove_file(&self.lock_path) {
            log::warn!("Failed to remove lock file: {}", e);
        }
        log::info!("Socket listener cleaned up: {:?}", self.socket_path);
    }
}

#[cfg(test)]
mod tests;
