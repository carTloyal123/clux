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
mod tests {
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
}
