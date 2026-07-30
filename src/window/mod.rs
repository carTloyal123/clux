//! Windows: named pane layouts within a session.

mod manager;
mod panes;
mod window;

pub use manager::WindowManager;
pub use window::{Window, WindowId};
