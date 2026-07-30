//! Registering pane PTYs with the poll loop.

use mio::unix::SourceFd;
use mio::{Interest, Token};

use super::{Server, ServerResult, PTY_TOKEN_BASE};
use crate::pane::PaneId;
use crate::session::SessionId;

impl Server {
    /// Register all PTYs for a session with mio.
    pub(super) fn register_session_ptys(&mut self, session_id: SessionId) -> ServerResult<()> {
        let session = match self.sessions.get(session_id) {
            Some(s) => s,
            None => return Ok(()),
        };

        // Get all pane IDs and their PTY fds
        let pane_info: Vec<(PaneId, i32)> = session
            .window_manager
            .all_panes()
            .iter()
            .map(|pane| (pane.id, pane.pty.as_raw_fd()))
            .collect();

        for (pane_id, fd) in pane_info {
            let token = Token(PTY_TOKEN_BASE + session_id.0 as usize * 1000 + pane_id.0 as usize);

            // Register the PTY fd with mio
            self.poll
                .registry()
                .register(&mut SourceFd(&fd), token, Interest::READABLE)?;

            self.token_to_pty.insert(token, (session_id, pane_id));
            log::debug!(
                "Registered PTY for session {:?} pane {:?} with token {:?}",
                session_id,
                pane_id,
                token
            );
        }

        Ok(())
    }
    /// Register a single new pane's PTY with mio.
    pub(super) fn register_new_pane_pty(
        &mut self,
        session_id: SessionId,
        pane_id: PaneId,
    ) -> ServerResult<()> {
        let session = match self.sessions.get(session_id) {
            Some(s) => s,
            None => return Ok(()),
        };

        // Find the pane and get its PTY fd
        let fd = session
            .window_manager
            .all_panes()
            .iter()
            .find(|p| p.id == pane_id)
            .map(|p| p.pty.as_raw_fd());

        if let Some(fd) = fd {
            let token = Token(PTY_TOKEN_BASE + session_id.0 as usize * 1000 + pane_id.0 as usize);

            self.poll
                .registry()
                .register(&mut SourceFd(&fd), token, Interest::READABLE)?;

            self.token_to_pty.insert(token, (session_id, pane_id));

            log::debug!(
                "Registered new PTY for session {:?} pane {:?} with token {:?}",
                session_id,
                pane_id,
                token
            );
        }

        Ok(())
    }
    /// Deregister a pane's PTY from mio.
    pub(super) fn deregister_pane_pty(&mut self, session_id: SessionId, pane_id: PaneId) {
        let token = Token(PTY_TOKEN_BASE + session_id.0 as usize * 1000 + pane_id.0 as usize);

        // Remove from tracking
        self.token_to_pty.remove(&token);

        // Note: mio deregistration happens automatically when the fd is closed
        log::debug!(
            "Deregistered PTY for session {:?} pane {:?}",
            session_id,
            pane_id
        );
    }
}
