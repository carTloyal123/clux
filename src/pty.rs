//! PTY (pseudo-terminal) management.
//!
//! Handles creating PTYs, spawning shells, and communication with child processes.
//! Uses the nix crate for Unix PTY operations.

// Some methods are kept for future use / API completeness
#![allow(dead_code)]

use std::ffi::CString;
use std::io;
use std::os::unix::io::{AsRawFd, OwnedFd, RawFd};

use nix::fcntl::{fcntl, FcntlArg, OFlag};
use nix::libc;
use nix::pty::{openpty, OpenptyResult, Winsize};
use nix::unistd::{dup2, execvp, fork, setsid, ForkResult, Pid};

use thiserror::Error;

/// PTY-related errors.
#[derive(Error, Debug)]
pub enum PtyError {
    #[error("Failed to open PTY: {0}")]
    OpenPty(#[from] nix::Error),

    #[error("Failed to spawn shell: {0}")]
    Spawn(String),

    #[error("IO error: {0}")]
    Io(#[from] io::Error),

    #[error("Invalid shell path")]
    InvalidShell,
}

/// PTY size in rows, columns, and pixels.
#[derive(Clone, Copy, Debug, Default)]
pub struct PtySize {
    pub rows: u16,
    pub cols: u16,
    pub pixel_width: u16,
    pub pixel_height: u16,
}

impl PtySize {
    /// Create a new PTY size.
    pub fn new(rows: u16, cols: u16) -> Self {
        Self {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        }
    }

    /// Convert to nix Winsize.
    fn to_winsize(&self) -> Winsize {
        Winsize {
            ws_row: self.rows,
            ws_col: self.cols,
            ws_xpixel: self.pixel_width,
            ws_ypixel: self.pixel_height,
        }
    }
}

/// A pseudo-terminal for communicating with a shell.
pub struct Pty {
    /// Master side of the PTY (we read/write here).
    master: OwnedFd,
    /// Child process ID.
    child_pid: Pid,
    /// Current size.
    size: PtySize,
}

impl Pty {
    /// Create a new PTY and spawn a shell.
    ///
    /// # Arguments
    /// * `size` - Initial terminal size
    /// * `shell` - Path to shell (e.g., "/bin/zsh" or "/bin/bash")
    pub fn spawn(size: PtySize, shell: &str) -> Result<Self, PtyError> {
        // Open the PTY pair
        let OpenptyResult { master, slave } = openpty(&size.to_winsize(), None)?;

        // Fork the process
        match unsafe { fork() }? {
            ForkResult::Parent { child } => {
                // Parent process - close slave, keep master
                drop(slave);

                // Set master to non-blocking
                let flags = fcntl(master.as_raw_fd(), FcntlArg::F_GETFL)?;
                let flags = OFlag::from_bits_truncate(flags);
                fcntl(
                    master.as_raw_fd(),
                    FcntlArg::F_SETFL(flags | OFlag::O_NONBLOCK),
                )?;

                Ok(Self {
                    master,
                    child_pid: child,
                    size,
                })
            }
            ForkResult::Child => {
                // Child process - set up PTY and exec shell
                drop(master);

                // Create a new session
                setsid().map_err(|e| PtyError::Spawn(format!("setsid failed: {}", e)))?;

                // Set the slave as controlling terminal
                unsafe {
                    if libc::ioctl(slave.as_raw_fd(), libc::TIOCSCTTY as _, 0) < 0 {
                        // Not fatal, continue anyway
                    }
                }

                // Duplicate slave to stdin/stdout/stderr
                dup2(slave.as_raw_fd(), libc::STDIN_FILENO)
                    .map_err(|e| PtyError::Spawn(format!("dup2 stdin: {}", e)))?;
                dup2(slave.as_raw_fd(), libc::STDOUT_FILENO)
                    .map_err(|e| PtyError::Spawn(format!("dup2 stdout: {}", e)))?;
                dup2(slave.as_raw_fd(), libc::STDERR_FILENO)
                    .map_err(|e| PtyError::Spawn(format!("dup2 stderr: {}", e)))?;

                // Close the original slave fd if it's not 0, 1, or 2
                let slave_fd = slave.as_raw_fd();
                if slave_fd > 2 {
                    drop(slave);
                }

                // Set up environment
                std::env::set_var("TERM", "xterm-256color");

                // Execute the shell
                let shell_cstr = CString::new(shell).map_err(|_| PtyError::InvalidShell)?;
                let shell_name = shell.rsplit('/').next().unwrap_or(shell);
                let arg0 =
                    CString::new(format!("-{}", shell_name)).map_err(|_| PtyError::InvalidShell)?;

                // execvp replaces the process, so this only returns on error
                execvp(&shell_cstr, &[arg0])
                    .map_err(|e| PtyError::Spawn(format!("execvp failed: {}", e)))?;

                unreachable!()
            }
        }
    }

    /// Resize the PTY.
    pub fn resize(&mut self, size: PtySize) -> Result<(), PtyError> {
        let winsize = size.to_winsize();
        unsafe {
            if libc::ioctl(self.master.as_raw_fd(), libc::TIOCSWINSZ as _, &winsize) < 0 {
                return Err(PtyError::Io(io::Error::last_os_error()));
            }
        }
        self.size = size;
        Ok(())
    }

    /// Read available bytes from the PTY (non-blocking).
    ///
    /// Returns the number of bytes read, or 0 if no data available.
    /// Returns an error on actual read failure.
    pub fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        loop {
            match nix::unistd::read(self.master.as_raw_fd(), buf) {
                Ok(n) => return Ok(n),
                Err(nix::Error::EAGAIN) => return Ok(0),
                Err(nix::Error::EINTR) => continue, // Interrupted by signal, retry
                Err(e) => return Err(io::Error::from_raw_os_error(e as i32)),
            }
        }
    }

    /// Try to read bytes without blocking.
    /// Returns None if no data is available.
    pub fn try_read(&mut self, buf: &mut [u8]) -> Option<usize> {
        match self.read(buf) {
            Ok(0) => None,
            Ok(n) => Some(n),
            Err(_) => None,
        }
    }

    /// Write bytes to the PTY.
    pub fn write(&mut self, data: &[u8]) -> io::Result<usize> {
        nix::unistd::write(&self.master, data).map_err(|e| io::Error::from_raw_os_error(e as i32))
    }

    /// Write all bytes to the PTY.
    pub fn write_all(&mut self, data: &[u8]) -> io::Result<()> {
        let mut written = 0;
        while written < data.len() {
            match self.write(&data[written..]) {
                Ok(n) => written += n,
                Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
                Err(e) => return Err(e),
            }
        }
        Ok(())
    }

    /// Get the raw file descriptor for the master PTY.
    /// Used for registering with mio/poll.
    pub fn as_raw_fd(&self) -> RawFd {
        self.master.as_raw_fd()
    }

    /// Get the current PTY size.
    pub fn size(&self) -> PtySize {
        self.size
    }

    /// Get the child process ID.
    pub fn child_pid(&self) -> Pid {
        self.child_pid
    }

    /// Check if the child process is still running.
    pub fn is_alive(&self) -> bool {
        let result =
            nix::sys::wait::waitpid(self.child_pid, Some(nix::sys::wait::WaitPidFlag::WNOHANG));
        match &result {
            Ok(nix::sys::wait::WaitStatus::StillAlive) => {
                log::trace!("is_alive(pid={}): StillAlive", self.child_pid);
                true
            }
            Ok(status) => {
                log::debug!(
                    "is_alive(pid={}): dead, status={:?}",
                    self.child_pid,
                    status
                );
                false
            }
            Err(e) => {
                log::debug!("is_alive(pid={}): error={:?}", self.child_pid, e);
                false
            }
        }
    }
}

impl Drop for Pty {
    fn drop(&mut self) {
        // Send SIGHUP to the child process
        let _ = nix::sys::signal::kill(self.child_pid, nix::sys::signal::Signal::SIGHUP);
    }
}

/// Detect the user's default shell.
pub fn detect_shell() -> String {
    // Try $SHELL environment variable first
    if let Ok(shell) = std::env::var("SHELL") {
        if !shell.is_empty() {
            return shell;
        }
    }

    // Fall back to /bin/zsh on macOS, /bin/bash elsewhere
    #[cfg(target_os = "macos")]
    {
        "/bin/zsh".to_string()
    }

    #[cfg(not(target_os = "macos"))]
    {
        "/bin/bash".to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pty_size() {
        let size = PtySize::new(24, 80);
        assert_eq!(size.rows, 24);
        assert_eq!(size.cols, 80);

        let winsize = size.to_winsize();
        assert_eq!(winsize.ws_row, 24);
        assert_eq!(winsize.ws_col, 80);
    }

    #[test]
    fn test_detect_shell() {
        let shell = detect_shell();
        assert!(!shell.is_empty());
        assert!(shell.starts_with('/'));
    }

    // Note: spawn tests require actually spawning a shell,
    // which is better suited for integration tests.
}
