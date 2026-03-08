//! Clux - A terminal multiplexer focused on UX
//!
//! Main entry point and event loop.

mod cell;
mod clipboard;
mod config;
mod event;
mod grid;
mod hyperlink;
mod pane;
mod pty;
mod render;
mod scrollback;
mod selection;
mod terminal;
mod window;

use std::collections::HashMap;
use std::io::{self, ErrorKind, Write};
use std::time::{Duration, Instant};

use mio::unix::SourceFd;
use mio::{Events, Interest, Poll, Token};

use crate::config::Config;
use crate::event::{
    process_event_with_handler, EventAction, KeybindingHandler, PaneDirection, SelectMode,
};
use crate::pane::{Direction, PaneId, SplitDirection};
use crate::pty::detect_shell;
use crate::render::Renderer;
use crate::selection::{find_word_bounds, Point, Selection, SelectionMode};
use crate::window::{WindowId, WindowManager};

/// Base token for PTY events.
/// Token = PTY_TOKEN_BASE + (window_id * 1000) + pane_id
/// This ensures unique tokens across all windows and panes.
const PTY_TOKEN_BASE: usize = 10000;

/// Maximum bytes to read from PTY per frame.
const MAX_READ_PER_FRAME: usize = 64 * 1024;

/// Generate a unique token for a (window_id, pane_id) pair.
fn pty_token(window_id: WindowId, pane_id: PaneId) -> Token {
    Token(PTY_TOKEN_BASE + (window_id.0 as usize * 1000) + pane_id.0 as usize)
}

fn main() -> anyhow::Result<()> {
    // Handle CLI flags before doing anything else
    let args: Vec<String> = std::env::args().collect();

    // Check for --debug flag to enable file logging
    let debug_mode = args.iter().any(|a| a == "--debug");

    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("clux - A terminal multiplexer focused on UX");
        println!();
        println!("USAGE:");
        println!("    clux [OPTIONS]");
        println!();
        println!("OPTIONS:");
        println!("    --dump-config    Print the default configuration to stdout");
        println!("    --show-config    Show the currently loaded configuration");
        println!("    --help, -h       Show this help message");
        println!();
        println!("KEYBINDINGS:");
        println!("    Alt+C            Enter command mode (prefix key)");
        println!("    After prefix:");
        println!("      -              Split pane horizontally");
        println!("      p              Split pane vertically");
        println!("      h/j/k/l        Navigate panes (vim-style)");
        println!("      arrows         Navigate panes");
        println!("      w              Close pane");
        println!("      n              New window");
        println!("      x              Close window");
        println!("      ]/[            Next/previous window");
        println!("      1-9, 0         Select window by number");
        println!("      q              Quit clux");
        println!();
        println!("CONFIG:");
        println!("    ~/.config/clux/config.toml    Primary config location");
        println!("    ~/.cluxrc                     Fallback config location");
        println!();
        println!(
            "    Run 'clux --dump-config > ~/.config/clux/config.toml' to create a config file."
        );
        return Ok(());
    }

    if args.iter().any(|a| a == "--dump-config") {
        print!("{}", Config::default_toml());
        return Ok(());
    }

    // Load configuration (before logging so --show-config works without log noise)
    let (config, config_source) = Config::load();

    if args.iter().any(|a| a == "--show-config") {
        config.display(&config_source);
        return Ok(());
    }

    // Initialize logging
    if debug_mode {
        // Log to file for debugging
        use std::fs::File;
        let log_file = File::create("debug.log").expect("Failed to create debug.log");
        env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("debug"))
            .target(env_logger::Target::Pipe(Box::new(log_file)))
            .init();
    } else {
        env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn")).init();
    }

    log::info!("Configuration loaded from: {}", config_source);
    log::info!("Prefix key: {}", config.prefix.key);

    // Create keybinding handler
    let keybinding_handler = KeybindingHandler::new(&config);

    // Initialize clipboard
    if let Err(e) = clipboard::init() {
        log::warn!("Failed to initialize clipboard: {:?}", e);
    }

    // Get terminal size
    let (cols, rows) = crossterm::terminal::size()?;
    log::info!("Terminal size: {}x{}", cols, rows);

    // Reserve space for the Clux border (1 char on each side)
    // Inner dimensions for panes
    let inner_cols = cols.saturating_sub(2);
    let inner_rows = rows.saturating_sub(2);
    log::info!("Inner size (after border): {}x{}", inner_cols, inner_rows);

    // Detect refresh rate
    let fps = Renderer::detect_refresh_rate();
    log::info!("Target refresh rate: {}Hz", fps);

    // Detect shell
    let shell = detect_shell();
    log::info!("Spawning shell: {}", shell);

    // Create window manager with inner dimensions (leaving room for border)
    let mut window_manager = WindowManager::new(inner_cols, inner_rows, &shell)?;

    // Create renderer
    let mut renderer = Renderer::new();

    // Enter alternate screen
    renderer.enter()?;

    // Set up mio poll
    let mut poll = Poll::new()?;
    let mut events = Events::with_capacity(128);

    // Track PTY token to (WindowId, PaneId) mapping
    let mut token_to_pane: HashMap<Token, (WindowId, PaneId)> = HashMap::new();

    // Register initial pane's PTY
    {
        let window_id = window_manager.active_id();
        let pane = window_manager.focused_pane().unwrap();
        let token = pty_token(window_id, pane.id);
        let fd = pane.fd();
        poll.registry()
            .register(&mut SourceFd(&fd), token, Interest::READABLE)?;
        token_to_pane.insert(token, (window_id, pane.id));
    }

    // Enable mouse capture
    crossterm::execute!(
        io::stdout(),
        crossterm::event::EnableMouseCapture,
        crossterm::event::EnableBracketedPaste
    )?;

    // Main event loop
    let result = run_event_loop(
        &mut poll,
        &mut events,
        &mut window_manager,
        &mut renderer,
        &mut token_to_pane,
        &keybinding_handler,
    );

    // Cleanup
    let _ = crossterm::execute!(
        io::stdout(),
        crossterm::event::DisableMouseCapture,
        crossterm::event::DisableBracketedPaste
    );
    let _ = renderer.leave();

    result
}

fn run_event_loop(
    poll: &mut Poll,
    events: &mut Events,
    window_manager: &mut WindowManager,
    renderer: &mut Renderer,
    token_to_pane: &mut HashMap<Token, (WindowId, PaneId)>,
    keybinding_handler: &KeybindingHandler,
) -> anyhow::Result<()> {
    let mut running = true;
    let mut needs_render = true;
    let frame_budget = renderer.frame_budget();

    // Selection state (per-pane in the future, global for now)
    let mut selection: Option<Selection> = None;

    // Read buffer
    let mut read_buf = [0u8; 8192];

    while running {
        let frame_start = Instant::now();

        // Calculate poll timeout
        let poll_timeout = if needs_render {
            Some(Duration::ZERO)
        } else {
            Some(frame_budget)
        };

        // Poll for events
        match poll.poll(events, poll_timeout) {
            Ok(()) => {}
            Err(e) if e.kind() == ErrorKind::Interrupted => continue,
            Err(e) => return Err(e.into()),
        }

        // Process PTY output from all panes
        for event in events.iter() {
            if let Some(&(window_id, pane_id)) = token_to_pane.get(&event.token()) {
                if let Some(window) = window_manager.get_window_mut(window_id) {
                    if let Some(pane) = window.pane_manager.get_pane_mut(pane_id) {
                        let mut bytes_read = 0;
                        while bytes_read < MAX_READ_PER_FRAME {
                            match pane.pty.try_read(&mut read_buf) {
                                Some(n) if n > 0 => {
                                    pane.parser.advance(&mut pane.terminal, &read_buf[..n]);
                                    bytes_read += n;
                                    needs_render = true;

                                    if frame_start.elapsed() > frame_budget / 2 {
                                        break;
                                    }
                                }
                                _ => break,
                            }
                        }
                    }
                }
            }
        }

        // Process keyboard/mouse input
        loop {
            match crossterm::event::poll(Duration::ZERO) {
                Ok(true) => {
                    match crossterm::event::read() {
                        Ok(ev) => {
                            match process_event_with_handler(ev, keybinding_handler) {
                                EventAction::SendToPty(bytes) => {
                                    // Clear selection and scroll to bottom on input
                                    selection = None;
                                    let active_window = window_manager.active_id();
                                    log::debug!(
                                        "SendToPty: {} bytes to window {:?}",
                                        bytes.len(),
                                        active_window
                                    );
                                    if let Some(pane) = window_manager.focused_pane_mut() {
                                        let pane_id = pane.id;
                                        let alive = pane.is_alive();
                                        log::debug!(
                                            "SendToPty: focused pane {:?}, alive={}",
                                            pane_id,
                                            alive
                                        );
                                        if pane.terminal.is_scrolled() {
                                            pane.terminal.scroll_to_bottom();
                                        }
                                        if alive {
                                            // Ignore write errors (pane may have just died)
                                            if let Err(e) = pane.pty.write_all(&bytes) {
                                                log::warn!(
                                                    "Write to pane {:?} failed: {}",
                                                    pane_id,
                                                    e
                                                );
                                            } else {
                                                log::debug!(
                                                    "Write to pane {:?} succeeded",
                                                    pane_id
                                                );
                                            }
                                        } else {
                                            log::warn!(
                                                "Skipping write to dead pane {:?} in window {:?}",
                                                pane_id,
                                                active_window
                                            );
                                        }
                                    } else {
                                        log::warn!("No focused pane in window {:?}", active_window);
                                    }
                                    needs_render = true;
                                }
                                EventAction::Resize(cols, rows) => {
                                    // Account for border when resizing
                                    let inner_cols = cols.saturating_sub(2);
                                    let inner_rows = rows.saturating_sub(2);
                                    window_manager.resize(inner_cols, inner_rows)?;
                                    selection = None;
                                    needs_render = true;
                                }
                                EventAction::Scroll(delta) => {
                                    if let Some(pane) = window_manager.focused_pane_mut() {
                                        pane.terminal.scroll_view(delta);
                                    }
                                    needs_render = true;
                                }
                                EventAction::SelectStart { row, col, mode } => {
                                    if let Some(pane) = window_manager.focused_pane() {
                                        let line = screen_to_line(
                                            row as usize,
                                            pane.terminal.scroll_offset,
                                            &pane.rect,
                                        );
                                        let point = Point::new(line, col as usize);

                                        let sel_mode = match mode {
                                            SelectMode::Normal => SelectionMode::Normal,
                                            SelectMode::Word => SelectionMode::Word,
                                            SelectMode::Triple => SelectionMode::Line,
                                        };

                                        let mut sel = Selection::start(point, sel_mode);

                                        if sel_mode == SelectionMode::Word {
                                            if let Some((start, end)) = get_word_bounds_at(
                                                &pane.terminal,
                                                line,
                                                col as usize,
                                            ) {
                                                sel.start = Point::new(line, start);
                                                sel.end = Point::new(line, end);
                                            }
                                        } else if sel_mode == SelectionMode::Line {
                                            sel.start = Point::new(line, 0);
                                            sel.end = Point::new(
                                                line,
                                                pane.terminal.cols().saturating_sub(1),
                                            );
                                        }

                                        selection = Some(sel);
                                        needs_render = true;
                                    }
                                }
                                EventAction::SelectExtend { row, col } => {
                                    if let Some(ref mut sel) = selection {
                                        if let Some(pane) = window_manager.focused_pane() {
                                            let line = screen_to_line(
                                                row as usize,
                                                pane.terminal.scroll_offset,
                                                &pane.rect,
                                            );

                                            match sel.mode {
                                                SelectionMode::Word => {
                                                    if let Some((_, end)) = get_word_bounds_at(
                                                        &pane.terminal,
                                                        line,
                                                        col as usize,
                                                    ) {
                                                        sel.end = Point::new(line, end);
                                                    }
                                                }
                                                SelectionMode::Line => {
                                                    sel.end = Point::new(
                                                        line,
                                                        pane.terminal.cols().saturating_sub(1),
                                                    );
                                                }
                                                _ => {
                                                    sel.end = Point::new(line, col as usize);
                                                }
                                            }
                                            needs_render = true;
                                        }
                                    }
                                }
                                EventAction::SelectEnd { row: _, col: _ } => {
                                    if let Some(ref sel) = selection {
                                        if !sel.is_empty() {
                                            if let Some(pane) = window_manager.focused_pane() {
                                                let text = sel.extract_text(
                                                    &pane.terminal.grid,
                                                    &pane.terminal.scrollback,
                                                    pane.terminal.scroll_offset,
                                                );
                                                if !text.is_empty() {
                                                    if let Err(e) = clipboard::copy(&text) {
                                                        log::warn!(
                                                            "Failed to copy to clipboard: {:?}",
                                                            e
                                                        );
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                                EventAction::CtrlClick { row, col } => {
                                    if let Some(pane) = window_manager.focused_pane() {
                                        let line = screen_to_line(
                                            row as usize,
                                            pane.terminal.scroll_offset,
                                            &pane.rect,
                                        );
                                        if let Some(hyperlink_id) =
                                            get_hyperlink_at(&pane.terminal, line, col as usize)
                                        {
                                            if let Some(url) =
                                                pane.terminal.hyperlinks.get(hyperlink_id)
                                            {
                                                if let Err(e) =
                                                    hyperlink::HyperlinkStore::open_url(url)
                                                {
                                                    log::warn!("Failed to open URL: {:?}", e);
                                                }
                                            }
                                        }
                                    }
                                }
                                EventAction::Paste => match clipboard::paste() {
                                    Ok(text) => {
                                        if let Some(pane) = window_manager.focused_pane_mut() {
                                            pane.pty.write_all(b"\x1b[200~")?;
                                            pane.pty.write_all(text.as_bytes())?;
                                            pane.pty.write_all(b"\x1b[201~")?;
                                        }
                                    }
                                    Err(e) => {
                                        log::warn!("Failed to paste from clipboard: {:?}", e);
                                    }
                                },
                                EventAction::SplitHorizontal => {
                                    let window_id = window_manager.active_id();
                                    match window_manager.split(SplitDirection::Horizontal) {
                                        Ok(new_id) => {
                                            // Register new pane's PTY
                                            let pane = window_manager
                                                .active_window()
                                                .pane_manager
                                                .get_pane(new_id)
                                                .unwrap();
                                            let token = pty_token(window_id, new_id);
                                            let fd = pane.fd();
                                            poll.registry().register(
                                                &mut SourceFd(&fd),
                                                token,
                                                Interest::READABLE,
                                            )?;
                                            token_to_pane.insert(token, (window_id, new_id));
                                            log::info!("Split horizontal, new pane: {:?}", new_id);
                                        }
                                        Err(e) => {
                                            log::error!("Failed to split horizontal: {:?}", e);
                                        }
                                    }
                                    selection = None;
                                    needs_render = true;
                                }
                                EventAction::SplitVertical => {
                                    let window_id = window_manager.active_id();
                                    match window_manager.split(SplitDirection::Vertical) {
                                        Ok(new_id) => {
                                            // Register new pane's PTY
                                            let pane = window_manager
                                                .active_window()
                                                .pane_manager
                                                .get_pane(new_id)
                                                .unwrap();
                                            let token = pty_token(window_id, new_id);
                                            let fd = pane.fd();
                                            poll.registry().register(
                                                &mut SourceFd(&fd),
                                                token,
                                                Interest::READABLE,
                                            )?;
                                            token_to_pane.insert(token, (window_id, new_id));
                                            log::info!("Split vertical, new pane: {:?}", new_id);
                                        }
                                        Err(e) => {
                                            log::error!("Failed to split vertical: {:?}", e);
                                        }
                                    }
                                    selection = None;
                                    needs_render = true;
                                }
                                EventAction::ClosePane => {
                                    let window_id = window_manager.active_id();
                                    let old_id = window_manager.focused_pane_id();
                                    if let Some(token) = token_to_pane
                                        .iter()
                                        .find(|(_, &(wid, pid))| wid == window_id && pid == old_id)
                                        .map(|(t, _)| *t)
                                    {
                                        token_to_pane.remove(&token);
                                    }

                                    if window_manager.active_pane_count() == 1 {
                                        // Last pane in window - close the window
                                        token_to_pane.retain(|_, (wid, _)| *wid != window_id);

                                        if window_manager.close_active_window().is_some() {
                                            log::info!(
                                                "Closed window {:?} (last pane closed)",
                                                window_id
                                            );
                                        } else {
                                            // Was the last window - exit app
                                            running = false;
                                        }
                                    } else {
                                        window_manager.close_focused_pane();
                                        log::info!("Closed pane: {:?}", old_id);
                                    }
                                    selection = None;
                                    needs_render = true;
                                }
                                EventAction::NavigatePane(direction) => {
                                    let dir = match direction {
                                        PaneDirection::Up => Direction::Up,
                                        PaneDirection::Down => Direction::Down,
                                        PaneDirection::Left => Direction::Left,
                                        PaneDirection::Right => Direction::Right,
                                    };
                                    window_manager.navigate_pane(dir);
                                    selection = None;
                                    needs_render = true;
                                }
                                EventAction::FocusPaneAt { row, col } => {
                                    let pm = &mut window_manager.active_window_mut().pane_manager;
                                    if let Some(id) = pm.pane_at(col, row) {
                                        pm.focus(id);
                                        selection = None;
                                        needs_render = true;
                                    }
                                }
                                EventAction::NewWindow => {
                                    match window_manager.create_window() {
                                        Ok(new_window_id) => {
                                            // Register initial pane's PTY for new window
                                            let pane = window_manager.focused_pane().unwrap();
                                            let token = pty_token(new_window_id, pane.id);
                                            let fd = pane.fd();
                                            poll.registry().register(
                                                &mut SourceFd(&fd),
                                                token,
                                                Interest::READABLE,
                                            )?;
                                            token_to_pane.insert(token, (new_window_id, pane.id));
                                            log::info!("Created window {:?}", new_window_id);
                                        }
                                        Err(e) => log::error!("Failed to create window: {:?}", e),
                                    }
                                    selection = None;
                                    needs_render = true;
                                }
                                EventAction::CloseWindow => {
                                    let window_id = window_manager.active_id();
                                    // Remove all PTY tokens for this window
                                    token_to_pane.retain(|_, (wid, _)| *wid != window_id);

                                    if window_manager.close_active_window().is_none() {
                                        // Was last window - exit
                                        running = false;
                                    }
                                    selection = None;
                                    needs_render = true;
                                }
                                EventAction::NextWindow => {
                                    window_manager.next_window();
                                    selection = None;
                                    needs_render = true;
                                }
                                EventAction::PrevWindow => {
                                    window_manager.prev_window();
                                    selection = None;
                                    needs_render = true;
                                }
                                EventAction::SelectWindow(index) => {
                                    window_manager.select_window(index);
                                    selection = None;
                                    needs_render = true;
                                }
                                EventAction::Exit => {
                                    running = false;
                                }
                                EventAction::None => {}
                            }
                        }
                        Err(e) if e.kind() == ErrorKind::Interrupted => continue,
                        Err(e) => return Err(e.into()),
                    }
                }
                Ok(false) => break,
                Err(e) if e.kind() == ErrorKind::Interrupted => continue,
                Err(e) => return Err(e.into()),
            }
        }

        // Check for dead panes across all windows
        // Process one dead pane at a time to avoid state corruption
        let dead_panes = window_manager.check_dead_panes();
        if !dead_panes.is_empty() {
            log::info!("Found {} dead panes: {:?}", dead_panes.len(), dead_panes);
        }
        for (window_id, pane_id) in dead_panes {
            // Skip if window no longer exists (may have been closed in previous iteration)
            if window_manager.get_window(window_id).is_none() {
                // Just clean up the token if it exists
                token_to_pane.retain(|_, (wid, pid)| !(*wid == window_id && *pid == pane_id));
                continue;
            }

            // Remove PTY token for this pane
            if let Some(token) = token_to_pane
                .iter()
                .find(|(_, &(wid, pid))| wid == window_id && pid == pane_id)
                .map(|(t, _)| *t)
            {
                token_to_pane.remove(&token);
            }

            // Get window's pane count before any modifications
            let pane_count = window_manager
                .get_window(window_id)
                .map(|w| w.pane_manager.pane_count())
                .unwrap_or(0);

            if pane_count <= 1 {
                // Last pane in this window - close the window
                // First remove all tokens for this window
                token_to_pane.retain(|_, (wid, _)| *wid != window_id);

                if window_manager.close_window(window_id) {
                    // Window closed, continue with other windows
                    log::info!("Closed window {:?} (last pane exited)", window_id);
                } else {
                    // Was the last window - exit app
                    running = false;
                }
            } else {
                // Close just this pane
                if let Some(window) = window_manager.get_window_mut(window_id) {
                    window.pane_manager.close_pane(pane_id);
                    log::info!("Closed dead pane {:?} in window {:?}", pane_id, window_id);
                }
            }
            needs_render = true;
        }

        // Render if needed
        if needs_render && renderer.should_render() {
            render_all_panes(window_manager, renderer, selection.as_ref())?;
            needs_render = false;
        }

        // Sleep for remaining frame budget
        let elapsed = frame_start.elapsed();
        if elapsed < frame_budget {
            std::thread::sleep(frame_budget - elapsed);
        }
    }

    Ok(())
}

/// Render all panes with borders.
fn render_all_panes(
    window_manager: &WindowManager,
    _renderer: &mut Renderer,
    selection: Option<&Selection>,
) -> io::Result<()> {
    use crossterm::{cursor::MoveTo, queue, terminal::Clear};

    let stdout = io::stdout();
    let mut stdout = stdout.lock();

    // Get terminal size for border
    let (term_cols, term_rows) = crossterm::terminal::size()?;

    // Begin synchronized update
    write!(stdout, "\x1bP=1s\x1b\\")?;
    queue!(stdout, crossterm::cursor::Hide)?;
    queue!(stdout, Clear(crossterm::terminal::ClearType::All))?;

    // Get the active window's pane manager
    let pane_manager = &window_manager.active_window().pane_manager;

    // Render each pane content first (offset by 1,1 for the border)
    let focused_id = pane_manager.focused_id();

    for pane in pane_manager.panes() {
        let is_focused = pane.id == focused_id;

        // Render pane content with offset for border
        render_pane_content_with_offset(
            &mut stdout,
            pane,
            if is_focused { selection } else { None },
            1, // x offset for left border
            1, // y offset for top border
        )?;
    }

    // Draw all borders in one pass (outer border + pane separators)
    render_all_borders(
        &mut stdout,
        pane_manager,
        window_manager,
        term_cols,
        term_rows,
    )?;

    // Show cursor in focused pane if not scrolled
    if let Some(pane) = pane_manager.focused_pane() {
        if pane.terminal.scroll_offset == 0 && selection.is_none() {
            // Add offset for border
            let cursor_row = 1 + pane.rect.y + pane.terminal.cursor.row as u16;
            let cursor_col = 1 + pane.rect.x + pane.terminal.cursor.col as u16;
            queue!(
                stdout,
                MoveTo(cursor_col, cursor_row),
                crossterm::cursor::Show
            )?;
        }
    }

    // End synchronized update
    write!(stdout, "\x1bP=2s\x1b\\")?;
    stdout.flush()?;

    Ok(())
}

/// Build the window indicator string for the top border.
/// Format: [1:name] [2:name*] where * marks the active window.
fn build_window_indicator(window_manager: &WindowManager) -> String {
    let windows = window_manager.windows();
    let active_idx = window_manager.active_index();

    // Only show indicator if there's more than one window
    if windows.len() <= 1 {
        return String::new();
    }

    let mut result = String::new();
    for (idx, window) in windows.iter().enumerate() {
        let is_active = idx == active_idx;
        let marker = if is_active { "*" } else { "" };
        let tab = format!("[{}:{}{}]", idx + 1, window.name, marker);
        if !result.is_empty() {
            result.push(' ');
        }
        result.push_str(&tab);
    }
    result
}

/// Render all borders: outer Clux border and internal pane separators.
fn render_all_borders(
    stdout: &mut impl Write,
    pane_manager: &pane::PaneManager,
    window_manager: &WindowManager,
    cols: u16,
    rows: u16,
) -> io::Result<()> {
    use crossterm::{
        cursor::MoveTo,
        queue,
        style::{Color, Print, ResetColor, SetForegroundColor},
    };

    let border_color = Color::Rgb {
        r: 147,
        g: 112,
        b: 219,
    }; // Medium purple

    queue!(stdout, SetForegroundColor(border_color))?;

    // Build a map of border positions to determine junction types
    // Inner area is cols-2 x rows-2 (excluding outer border)
    let inner_cols = cols.saturating_sub(2) as usize;
    let inner_rows = rows.saturating_sub(2) as usize;

    // Collect all internal border positions from panes
    let mut h_borders: Vec<(u16, u16, u16)> = Vec::new(); // (y, x_start, x_end) - horizontal borders
    let mut v_borders: Vec<(u16, u16, u16)> = Vec::new(); // (x, y_start, y_end) - vertical borders

    for pane in pane_manager.panes() {
        let rect = &pane.rect;
        // Right edge of pane (if not at screen right edge)
        if (rect.x + rect.width) < inner_cols as u16 {
            v_borders.push((rect.x + rect.width, rect.y, rect.y + rect.height));
        }
        // Bottom edge of pane (if not at screen bottom edge)
        if (rect.y + rect.height) < inner_rows as u16 {
            h_borders.push((rect.y + rect.height, rect.x, rect.x + rect.width));
        }
    }

    // Draw outer border corners
    queue!(stdout, MoveTo(0, 0), Print("╭"))?;
    queue!(stdout, MoveTo(cols.saturating_sub(1), 0), Print("╮"))?;
    queue!(stdout, MoveTo(0, rows.saturating_sub(1)), Print("╰"))?;
    queue!(
        stdout,
        MoveTo(cols.saturating_sub(1), rows.saturating_sub(1)),
        Print("╯")
    )?;

    // Build window indicator string for top border: [1:name] [2:name*] ...
    let window_indicator = build_window_indicator(window_manager);
    let indicator_chars: Vec<char> = window_indicator.chars().collect();
    let indicator_len = indicator_chars.len() as u16;

    // Draw top border with window indicator (check for vertical border junctions)

    for x in 1..cols.saturating_sub(1) {
        let inner_x = x - 1;
        let has_v_border = v_borders
            .iter()
            .any(|(vx, vy_start, _)| *vx == inner_x && *vy_start == 0);

        // Check if we're in the indicator area
        let indicator_pos = x - 1;
        if indicator_pos < indicator_len && !has_v_border {
            let c = indicator_chars[indicator_pos as usize];
            queue!(stdout, MoveTo(x, 0), Print(c))?;
        } else if has_v_border {
            queue!(stdout, MoveTo(x, 0), Print("┬"))?;
        } else {
            queue!(stdout, MoveTo(x, 0), Print("─"))?;
        }
    }

    // Draw bottom border with "clux" label (check for vertical border junctions)
    let bottom_row = rows.saturating_sub(1);
    let label = " clux ";
    let label_chars: Vec<char> = label.chars().collect();
    let label_len = label_chars.len() as u16;
    let border_width = cols.saturating_sub(2);

    // Find positions where vertical borders meet the bottom
    let v_junction_positions: Vec<u16> = v_borders
        .iter()
        .filter(|(_, _, vy_end)| *vy_end == inner_rows as u16)
        .map(|(vx, _, _)| *vx)
        .collect();

    // Calculate label position, avoiding any junction positions
    let mut label_start = (border_width.saturating_sub(label_len)) / 2;

    // Check if any junction would overlap the label, and if so, move label
    for &jpos in &v_junction_positions {
        if jpos >= label_start && jpos < label_start + label_len {
            // Junction overlaps label - try moving label to the right of the junction
            let new_start = jpos + 2;
            if new_start + label_len <= border_width {
                label_start = new_start;
            } else {
                // Try moving it to the left of the junction instead
                if jpos >= label_len + 2 {
                    label_start = jpos - label_len - 1;
                }
            }
        }
    }

    for x in 1..cols.saturating_sub(1) {
        let inner_x = x - 1;
        let has_v_border = v_junction_positions.contains(&inner_x);

        if has_v_border {
            queue!(stdout, MoveTo(x, bottom_row), Print("┴"))?;
        } else {
            // Check if we're in the label area
            let pos = inner_x;
            if pos >= label_start && pos < label_start + label_len {
                let label_idx = (pos - label_start) as usize;
                queue!(stdout, MoveTo(x, bottom_row), Print(label_chars[label_idx]))?;
            } else {
                queue!(stdout, MoveTo(x, bottom_row), Print("─"))?;
            }
        }
    }

    // Draw left border (check for horizontal border junctions)
    for y in 1..rows.saturating_sub(1) {
        let inner_y = y - 1;
        let has_h_border = h_borders
            .iter()
            .any(|(hy, hx_start, _)| *hy == inner_y && *hx_start == 0);
        if has_h_border {
            queue!(stdout, MoveTo(0, y), Print("├"))?;
        } else {
            queue!(stdout, MoveTo(0, y), Print("│"))?;
        }
    }

    // Draw right border (check for horizontal border junctions)
    let right_col = cols.saturating_sub(1);
    for y in 1..rows.saturating_sub(1) {
        let inner_y = y - 1;
        let has_h_border = h_borders
            .iter()
            .any(|(hy, _, hx_end)| *hy == inner_y && *hx_end == inner_cols as u16);
        if has_h_border {
            queue!(stdout, MoveTo(right_col, y), Print("┤"))?;
        } else {
            queue!(stdout, MoveTo(right_col, y), Print("│"))?;
        }
    }

    // Draw internal vertical borders
    for (vx, vy_start, vy_end) in &v_borders {
        let screen_x = vx + 1; // offset for outer border
        for y in *vy_start..*vy_end {
            let screen_y = y + 1;
            // Check for intersection with horizontal border
            let has_h_intersection = h_borders
                .iter()
                .any(|(hy, hx_start, hx_end)| *hy == y && *hx_start <= *vx && *vx <= *hx_end);
            if has_h_intersection {
                queue!(stdout, MoveTo(screen_x, screen_y), Print("┼"))?;
            } else {
                queue!(stdout, MoveTo(screen_x, screen_y), Print("│"))?;
            }
        }
    }

    // Draw internal horizontal borders
    for (hy, hx_start, hx_end) in &h_borders {
        let screen_y = hy + 1; // offset for outer border
        for x in *hx_start..*hx_end {
            let screen_x = x + 1;
            // Check if already drawn as intersection
            let has_v_intersection = v_borders
                .iter()
                .any(|(vx, vy_start, vy_end)| *vx == x && *vy_start <= *hy && *hy <= *vy_end);
            if !has_v_intersection {
                queue!(stdout, MoveTo(screen_x, screen_y), Print("─"))?;
            }
        }
    }

    queue!(stdout, ResetColor)?;
    Ok(())
}

/// Draw the purple Clux border around the terminal.
#[allow(dead_code)]
fn render_clux_border(stdout: &mut impl Write, cols: u16, rows: u16) -> io::Result<()> {
    use crossterm::{
        cursor::MoveTo,
        queue,
        style::{Color, Print, ResetColor, SetForegroundColor},
    };

    // Use a nice purple/magenta color for the border
    let border_color = Color::Rgb {
        r: 147,
        g: 112,
        b: 219,
    }; // Medium purple

    queue!(stdout, SetForegroundColor(border_color))?;

    // Top border with corners
    queue!(stdout, MoveTo(0, 0), Print("╭"))?;
    for _ in 1..cols.saturating_sub(1) {
        queue!(stdout, Print("─"))?;
    }
    queue!(stdout, MoveTo(cols.saturating_sub(1), 0), Print("╮"))?;

    // Side borders
    for row in 1..rows.saturating_sub(1) {
        queue!(stdout, MoveTo(0, row), Print("│"))?;
        queue!(stdout, MoveTo(cols.saturating_sub(1), row), Print("│"))?;
    }

    // Bottom border with corners and "clux" label
    let bottom_row = rows.saturating_sub(1);
    queue!(stdout, MoveTo(0, bottom_row), Print("╰"))?;

    // Calculate position for centered "clux" label
    let label = " clux ";
    let label_len = label.chars().count() as u16;
    let border_width = cols.saturating_sub(2); // width between corners
    let label_start = (border_width.saturating_sub(label_len)) / 2;

    // Draw bottom border character by character
    for i in 1..cols.saturating_sub(1) {
        let pos = i - 1; // position in the inner border (0-based)
        if pos >= label_start && pos < label_start + label_len {
            // We're in the label area - print the appropriate label character
            let label_char_idx = (pos - label_start) as usize;
            if let Some(c) = label.chars().nth(label_char_idx) {
                queue!(stdout, Print(c))?;
            } else {
                queue!(stdout, Print("─"))?;
            }
        } else {
            queue!(stdout, Print("─"))?;
        }
    }

    queue!(
        stdout,
        MoveTo(cols.saturating_sub(1), bottom_row),
        Print("╯")
    )?;

    queue!(stdout, ResetColor)?;
    Ok(())
}

/// Render a single pane's content with offset for the Clux border.
fn render_pane_content_with_offset(
    stdout: &mut impl Write,
    pane: &pane::Pane,
    selection: Option<&Selection>,
    x_offset: u16,
    y_offset: u16,
) -> io::Result<()> {
    use crossterm::{
        cursor::MoveTo,
        queue,
        style::{
            Attribute, Color, Print, ResetColor, SetAttribute, SetBackgroundColor,
            SetForegroundColor,
        },
    };

    let term = &pane.terminal;
    let rect = pane.rect;
    let scroll_offset = term.scroll_offset;

    for row_idx in 0..rect.height as usize {
        let screen_row = y_offset + rect.y + row_idx as u16;
        let screen_col = x_offset + rect.x;
        queue!(stdout, MoveTo(screen_col, screen_row))?;

        let line = screen_to_line(row_idx, scroll_offset, &rect);

        // Get the cells for this row
        let cells: Vec<cell::Cell> = if scroll_offset > 0 && row_idx < scroll_offset {
            // Scrollback row
            let sb_idx = scroll_offset - row_idx - 1;
            term.scrollback
                .get(sb_idx)
                .map(|l| l.cells().to_vec())
                .unwrap_or_else(|| vec![cell::Cell::default(); rect.width as usize])
        } else {
            // Grid row
            let grid_row = if scroll_offset > 0 {
                row_idx - scroll_offset
            } else {
                row_idx
            };
            term.grid
                .row(grid_row)
                .map(|r| {
                    (0..rect.width as usize)
                        .map(|c| r.get(c).copied().unwrap_or_default())
                        .collect()
                })
                .unwrap_or_else(|| vec![cell::Cell::default(); rect.width as usize])
        };

        // Render cells
        let mut current_fg: Option<cell::Color> = None;
        let mut current_bg: Option<cell::Color> = None;
        let mut buffer = String::with_capacity(rect.width as usize);

        for (col_idx, cell) in cells.iter().take(rect.width as usize).enumerate() {
            let is_selected = selection
                .map(|sel| sel.active && sel.contains(Point::new(line, col_idx)))
                .unwrap_or(false);

            let style_changed =
                current_fg.as_ref() != Some(&cell.fg) || current_bg.as_ref() != Some(&cell.bg);

            if style_changed || (is_selected && buffer.len() > 0) {
                if !buffer.is_empty() {
                    queue!(stdout, Print(&buffer))?;
                    buffer.clear();
                }

                if is_selected {
                    queue!(
                        stdout,
                        SetForegroundColor(Color::Black),
                        SetBackgroundColor(Color::White)
                    )?;
                } else {
                    apply_cell_colors(stdout, cell)?;
                }
                current_fg = Some(cell.fg);
                current_bg = Some(cell.bg);
            }

            buffer.push(cell.c);
        }

        // Pad with spaces
        while buffer.len() < rect.width as usize {
            buffer.push(' ');
        }

        if !buffer.is_empty() {
            queue!(stdout, Print(&buffer))?;
        }
        queue!(stdout, ResetColor, SetAttribute(Attribute::Reset))?;
    }

    // Render scroll indicator if scrolled
    if scroll_offset > 0 {
        let indicator = format!("[{}/{}]", scroll_offset, term.scrollback.len());
        let col = x_offset + rect.x + rect.width.saturating_sub(indicator.len() as u16);
        let row = y_offset + rect.y;
        queue!(
            stdout,
            MoveTo(col, row),
            SetForegroundColor(Color::Black),
            SetBackgroundColor(Color::White),
            Print(&indicator),
            ResetColor
        )?;
    }

    Ok(())
}

/// Apply cell colors to stdout.
fn apply_cell_colors(stdout: &mut impl Write, cell: &cell::Cell) -> io::Result<()> {
    use crossterm::{
        queue,
        style::{Color, SetBackgroundColor, SetForegroundColor},
    };

    match cell.fg.kind {
        cell::ColorKind::Default => {
            queue!(stdout, SetForegroundColor(Color::Reset))?;
        }
        cell::ColorKind::Indexed => {
            queue!(stdout, SetForegroundColor(Color::AnsiValue(cell.fg.r)))?;
        }
        cell::ColorKind::Rgb => {
            queue!(
                stdout,
                SetForegroundColor(Color::Rgb {
                    r: cell.fg.r,
                    g: cell.fg.g,
                    b: cell.fg.b
                })
            )?;
        }
    }

    match cell.bg.kind {
        cell::ColorKind::Default => {
            queue!(stdout, SetBackgroundColor(Color::Reset))?;
        }
        cell::ColorKind::Indexed => {
            queue!(stdout, SetBackgroundColor(Color::AnsiValue(cell.bg.r)))?;
        }
        cell::ColorKind::Rgb => {
            queue!(
                stdout,
                SetBackgroundColor(Color::Rgb {
                    r: cell.bg.r,
                    g: cell.bg.g,
                    b: cell.bg.b
                })
            )?;
        }
    }

    Ok(())
}

/// Render border around a pane.
#[allow(dead_code)]
fn render_pane_border(
    stdout: &mut impl Write,
    rect: &pane::Rect,
    focused: bool,
    x_offset: u16,
    y_offset: u16,
) -> io::Result<()> {
    use crossterm::{
        cursor::MoveTo,
        queue,
        style::{Color, Print, ResetColor, SetForegroundColor},
    };

    // Get terminal size to know where the outer border is
    let (term_cols, term_rows) = crossterm::terminal::size().unwrap_or((80, 24));

    let border_color = if focused {
        Color::Rgb {
            r: 147,
            g: 112,
            b: 219,
        } // Purple for focused (match outer border)
    } else {
        Color::DarkGrey
    };

    queue!(stdout, SetForegroundColor(border_color))?;

    // Calculate absolute positions
    let right_x = x_offset + rect.x + rect.width;
    let bottom_y = y_offset + rect.y + rect.height;
    let at_right_edge = right_x >= term_cols.saturating_sub(1);
    let at_bottom_edge = bottom_y >= term_rows.saturating_sub(1);

    // Draw right border (vertical line) if not at the outer right edge
    if !at_right_edge {
        for y in (y_offset + rect.y)..(y_offset + rect.y + rect.height) {
            queue!(stdout, MoveTo(right_x, y), Print("│"))?;
        }
    }

    // Draw bottom border (horizontal line) if not at the outer bottom edge
    if !at_bottom_edge {
        for x in (x_offset + rect.x)..(x_offset + rect.x + rect.width) {
            queue!(stdout, MoveTo(x, bottom_y), Print("─"))?;
        }
    }

    // Draw corner/junction character
    if !at_right_edge && !at_bottom_edge {
        // Interior corner - use cross or appropriate junction
        queue!(stdout, MoveTo(right_x, bottom_y), Print("┼"))?;
    } else if at_right_edge && !at_bottom_edge {
        // Connects to right outer border - use right T-junction
        queue!(stdout, MoveTo(right_x, bottom_y), Print("┤"))?;
    } else if !at_right_edge && at_bottom_edge {
        // Connects to bottom outer border - use bottom T-junction
        queue!(stdout, MoveTo(right_x, bottom_y), Print("┴"))?;
    }
    // If at both edges, no corner needed - outer border handles it

    // Draw top junction if pane doesn't start at top
    if rect.y > 0 && !at_right_edge {
        let top_y = y_offset + rect.y;
        // Check if there's a pane above (use T-junction) or it's the outer border
        if top_y == y_offset {
            // At top outer border
            queue!(stdout, MoveTo(right_x, top_y - 1), Print("┬"))?;
        }
    }

    // Draw left junction if pane doesn't start at left
    if rect.x > 0 && !at_bottom_edge {
        let left_x = x_offset + rect.x;
        // Check if there's a pane to the left
        if left_x == x_offset {
            // At left outer border
            queue!(stdout, MoveTo(left_x - 1, bottom_y), Print("├"))?;
        }
    }

    queue!(stdout, ResetColor)?;

    Ok(())
}

/// Convert screen row to line coordinate.
fn screen_to_line(screen_row: usize, scroll_offset: usize, _rect: &pane::Rect) -> i32 {
    if scroll_offset == 0 {
        screen_row as i32
    } else {
        let scrollback_rows_visible = scroll_offset;
        if screen_row < scrollback_rows_visible {
            -((scrollback_rows_visible - screen_row) as i32)
        } else {
            (screen_row - scrollback_rows_visible) as i32
        }
    }
}

/// Get word bounds at a given position.
fn get_word_bounds_at(term: &terminal::Terminal, line: i32, col: usize) -> Option<(usize, usize)> {
    let cells = if line < 0 {
        let sb_idx = (-line - 1) as usize;
        term.scrollback.get(sb_idx)?.cells().to_vec()
    } else {
        let row = term.grid.row(line as usize)?;
        (0..term.cols())
            .map(|c| row.get(c).copied().unwrap_or_default())
            .collect()
    };

    Some(find_word_bounds(&cells, col))
}

/// Get hyperlink ID at a given position.
fn get_hyperlink_at(term: &terminal::Terminal, line: i32, col: usize) -> Option<cell::HyperlinkId> {
    if line < 0 {
        let sb_idx = (-line - 1) as usize;
        let sb_line = term.scrollback.get(sb_idx)?;
        sb_line.cells().get(col)?.hyperlink
    } else {
        let row = term.grid.row(line as usize)?;
        row.get(col)?.hyperlink
    }
}
