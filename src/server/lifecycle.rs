//! Server construction, the event loop, and session-driven shutdown.

use std::collections::HashMap;
use std::os::unix::io::AsRawFd;
use std::time::{Duration, Instant};

use mio::unix::SourceFd;
use mio::{Events, Interest, Poll};

use super::listener::SocketListener;
use super::{
    AutoShutdownConfig, Server, ServerConfig, ServerError, ServerResult, CLIENT_TOKEN_BASE,
    LISTENER_TOKEN, PTY_TOKEN_BASE,
};
use crate::session::SessionManager;

impl Server {
    /// Create a new server with the given configuration.
    pub fn new(config: ServerConfig) -> ServerResult<Self> {
        Self::with_auto_shutdown(config, AutoShutdownConfig::default())
    }

    /// Create a new server with custom auto-shutdown configuration.
    pub fn with_auto_shutdown(
        config: ServerConfig,
        auto_shutdown: AutoShutdownConfig,
    ) -> ServerResult<Self> {
        let poll = Poll::new()?;

        // Create socket directory if needed
        if let Some(parent) = config.socket_path.parent() {
            std::fs::create_dir_all(parent).map_err(ServerError::SocketDir)?;
        }

        // Create and register the listener
        let listener = SocketListener::bind(&config.socket_path)?;
        poll.registry().register(
            &mut SourceFd(&listener.as_raw_fd()),
            LISTENER_TOKEN,
            Interest::READABLE,
        )?;

        log::info!("Server listening on {:?}", config.socket_path);
        if auto_shutdown.enabled {
            log::info!(
                "Auto-shutdown enabled: grace_period={:?}, first_session_timeout={:?}",
                auto_shutdown.grace_period,
                auto_shutdown.first_session_timeout
            );
        } else {
            log::info!("Auto-shutdown disabled (daemon mode)");
        }

        Ok(Self {
            sessions: SessionManager::new(config.shell.clone()),
            config,
            poll,
            listener,
            clients: HashMap::new(),
            token_to_client: HashMap::new(),
            token_to_pty: HashMap::new(),
            client_sizes: HashMap::new(),
            next_client_id: 0,
            running: true,
            auto_shutdown,
            started_at: Instant::now(),
            shutdown_pending_since: None,
            session_ever_created: false,
        })
    }

    /// Run the server event loop.
    pub fn run(&mut self) -> ServerResult<()> {
        let mut events = Events::with_capacity(128);
        let timeout = Duration::from_millis(100);

        while self.running {
            self.poll.poll(&mut events, Some(timeout))?;

            for event in events.iter() {
                match event.token() {
                    LISTENER_TOKEN => {
                        self.accept_client()?;
                    }
                    token if token.0 >= CLIENT_TOKEN_BASE && token.0 < PTY_TOKEN_BASE => {
                        if let Some(&client_id) = self.token_to_client.get(&token) {
                            self.handle_client_event(client_id)?;
                        }
                    }
                    token if token.0 >= PTY_TOKEN_BASE => {
                        // TODO: Handle PTY events (Phase 7.6)
                        self.handle_pty_event(token)?;
                    }
                    _ => {}
                }
            }

            // Check for dead clients
            self.cleanup_dead_clients();

            // Check for dead panes (shells that exited)
            self.cleanup_dead_panes();

            // Check auto-shutdown conditions
            self.check_auto_shutdown();
        }

        log::info!("Server shutting down");
        Ok(())
    }
}
