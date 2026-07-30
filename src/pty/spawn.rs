//! Spawning a shell in a new PTY, and resizing it.

use std::ffi::CString;
use std::io;
use std::os::unix::io::AsRawFd;

use nix::fcntl::{fcntl, FcntlArg, OFlag};
use nix::libc;
use nix::pty::{openpty, OpenptyResult};
use nix::unistd::{dup2, execvp, fork, setsid, ForkResult};

use super::{PtyError, PtySize};

impl super::Pty {
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
}
