//! Command-mode actions, and session kill/rename.

use super::client_conn::ClientState;
use super::{Server, ServerResult};
use crate::pane::PaneId;
use crate::protocol::{DetachReason, ServerMessage};
use crate::session::ClientId;

impl Server {
    pub(super) fn handle_command(
        &mut self,
        client_id: ClientId,
        action: crate::protocol::CommandAction,
    ) -> ServerResult<()> {
        // Get the session this client is attached to
        let session_id = {
            let client = match self.clients.get(&client_id) {
                Some(c) => c,
                None => return Ok(()),
            };

            match client.state {
                ClientState::Attached(id) => id,
                _ => return Ok(()),
            }
        };

        // Track post-action work needed (to avoid borrow conflicts)
        enum PostAction {
            None,
            Refresh,
            RegisterPtyAndRefresh(PaneId),
            DeregisterPtyAndRefresh(PaneId),
            CloseSession,
        }

        let post_action = {
            let session = match self.sessions.get_mut(session_id) {
                Some(s) => s,
                None => return Ok(()),
            };

            use crate::protocol::CommandAction::*;
            match action {
                SplitHorizontal => {
                    log::info!("Executing SplitHorizontal command");
                    match session
                        .window_manager
                        .split(crate::pane::SplitDirection::Horizontal)
                    {
                        Ok(new_pane_id) => {
                            log::info!("Split horizontal created new pane {:?}", new_pane_id);
                            PostAction::RegisterPtyAndRefresh(new_pane_id)
                        }
                        Err(e) => {
                            log::error!("Split horizontal failed: {:?}", e);
                            PostAction::None
                        }
                    }
                }
                SplitVertical => {
                    log::info!("Executing SplitVertical command");
                    match session
                        .window_manager
                        .split(crate::pane::SplitDirection::Vertical)
                    {
                        Ok(new_pane_id) => {
                            log::info!("Split vertical created new pane {:?}", new_pane_id);
                            PostAction::RegisterPtyAndRefresh(new_pane_id)
                        }
                        Err(e) => {
                            log::error!("Split vertical failed: {:?}", e);
                            PostAction::None
                        }
                    }
                }
                ClosePane => {
                    let pane_id = session.window_manager.focused_pane_id();
                    session.window_manager.close_focused_pane();
                    PostAction::DeregisterPtyAndRefresh(pane_id)
                }
                NavigatePane(dir) => {
                    let pane_dir = match dir {
                        crate::protocol::Direction::Up => crate::pane::Direction::Up,
                        crate::protocol::Direction::Down => crate::pane::Direction::Down,
                        crate::protocol::Direction::Left => crate::pane::Direction::Left,
                        crate::protocol::Direction::Right => crate::pane::Direction::Right,
                    };
                    session.window_manager.navigate_pane(pane_dir);
                    // Focus change needs screen refresh for visual feedback
                    PostAction::Refresh
                }
                NewWindow => {
                    if session.window_manager.create_window().is_ok() {
                        // The new window is now active, get its focused pane ID
                        let new_pane_id = session.window_manager.focused_pane_id();
                        PostAction::RegisterPtyAndRefresh(new_pane_id)
                    } else {
                        PostAction::None
                    }
                }
                CloseWindow => {
                    session.window_manager.close_active_window();
                    // Window close needs full screen refresh
                    PostAction::Refresh
                }
                NextWindow => {
                    session.window_manager.next_window();
                    // Window switch needs full screen refresh to show new window
                    PostAction::Refresh
                }
                PrevWindow => {
                    session.window_manager.prev_window();
                    // Window switch needs full screen refresh to show new window
                    PostAction::Refresh
                }
                SelectWindow(index) => {
                    session.window_manager.select_window(index);
                    // Window switch needs full screen refresh to show new window
                    PostAction::Refresh
                }
                Quit => PostAction::CloseSession,
            }
        };

        // Execute post-actions after session borrow is released
        match post_action {
            PostAction::None => {}
            PostAction::Refresh => {
                // Send full screen to all clients to show updated state
                self.broadcast_full_screen(session_id)?;
            }
            PostAction::RegisterPtyAndRefresh(pane_id) => {
                self.register_new_pane_pty(session_id, pane_id)?;
                // Send full screen to all clients to show new layout
                self.broadcast_full_screen(session_id)?;
            }
            PostAction::DeregisterPtyAndRefresh(pane_id) => {
                self.deregister_pane_pty(session_id, pane_id);
                // Send full screen to all clients to show updated layout
                self.broadcast_full_screen(session_id)?;
            }

            PostAction::CloseSession => {
                self.sessions.close_session(session_id);
                self.send_to_client(
                    client_id,
                    &ServerMessage::Detached {
                        reason: DetachReason::SessionClosed,
                    },
                )?;
            }
        }

        Ok(())
    }
}
