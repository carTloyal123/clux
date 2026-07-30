//! The documented default config file, written on first run.

/// The default configuration file with all options documented.
pub const DEFAULT_CONFIG: &str = r#"# Clux Terminal Multiplexer Configuration
# ========================================
#
# This file documents ALL available configuration options.
# Edit any value to customize your setup.

# ==============================================================================
#                              SERVER SETTINGS
# ==============================================================================
# These settings control the clux server process.

[server]
# Log level: "error", "warn", "info", "debug", "trace"
log_level = "info"

# Directory for log files. The server writes to {log_dir}/clux-server.log
# Defaults to ~/.local/state/clux/ if not specified.
# Set to "" (empty string) to disable file logging and only log to stderr.
# log_dir = "~/.local/state/clux"

# ==============================================================================
#                              KEYBINDINGS
# ==============================================================================
#
# Key Syntax:
#   - Modifiers: ctrl, alt, shift, super (cmd on macOS)
#   - Separator: + (e.g., "ctrl+shift+c")
#   - Special keys: enter, escape, tab, space, backspace, delete
#   - Function keys: f1, f2, ... f12
#   - Navigation: up, down, left, right, home, end, pageup, pagedown
#   - Characters: a-z, 0-9, and symbols like -, [, ], ', etc.
#
# Examples:
#   "a"           - The 'a' key
#   "ctrl+c"      - Ctrl+C
#   "alt+enter"   - Alt+Enter
#   "super+v"     - Cmd+V (macOS) / Super+V (Linux)

# ==============================================================================
#                              COMMAND PREFIX
# ==============================================================================
# The prefix key enters "command mode" where the next key triggers an action.
# This is similar to tmux's prefix (Ctrl+B) or screen's (Ctrl+A).
#
# Default: Option+C (Alt+C on Linux)
# After pressing the prefix, press another key to execute a command.

[prefix]
key = "alt+c"

# ==============================================================================
#                             PANE MANAGEMENT
# ==============================================================================
# These keys work AFTER pressing the prefix key.
# Panes let you split your terminal into multiple views.

[keybindings.pane]
# Split the current pane into two
split_horizontal = "-"          # New pane below current
split_vertical = "p"            # New pane to the right

# Close the focused pane
close = "w"

# Navigate between panes (vim-style)
navigate_up = "k"
navigate_down = "j"
navigate_left = "h"
navigate_right = "l"

# Navigate between panes (arrow keys)
navigate_up_arrow = "up"
navigate_down_arrow = "down"
navigate_left_arrow = "left"
navigate_right_arrow = "right"

# ==============================================================================
#                            WINDOW MANAGEMENT
# ==============================================================================
# These keys work AFTER pressing the prefix key.
# Windows are like browser tabs - each has its own pane layout.

[keybindings.window]
# Create and close windows
new = "n"                       # Create a new window
close = "x"                     # Close the current window

# Navigate between windows
next = "]"                      # Switch to next window
previous = "'"                  # Switch to previous window
previous_alt = "["              # Alternative key for previous

# Jump directly to a window by number
select_1 = "1"                  # Switch to window 1
select_2 = "2"                  # Switch to window 2
select_3 = "3"                  # Switch to window 3
select_4 = "4"                  # Switch to window 4
select_5 = "5"                  # Switch to window 5
select_6 = "6"                  # Switch to window 6
select_7 = "7"                  # Switch to window 7
select_8 = "8"                  # Switch to window 8
select_9 = "9"                  # Switch to window 9
select_10 = "0"                 # Switch to window 10 (0 = 10)

# ==============================================================================
#                               APPLICATION
# ==============================================================================
# These keys work AFTER pressing the prefix key.

[keybindings.app]
quit = "q"                      # Exit Clux entirely
send_prefix = "c"               # Send the prefix key to the terminal
                                # (useful if an app needs Alt+C)

# ==============================================================================
#                            DIRECT KEYBINDINGS
# ==============================================================================
# These keys work WITHOUT pressing the prefix first.
# Use with caution to avoid conflicts with terminal applications.

[keybindings.direct]
# Scrollback navigation
scroll_up = "shift+pageup"      # Scroll up through history
scroll_down = "shift+pagedown"  # Scroll down through history

# Clipboard operations
paste = "super+v"               # Paste from clipboard (Cmd+V on macOS)
paste_alt = "ctrl+shift+v"      # Alternative paste binding
"#;
