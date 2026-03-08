# Phase 7: Detached Sessions - Comprehensive Implementation Plan

## Executive Summary

This phase transforms clux from a single-process terminal multiplexer into a robust client-server architecture enabling session persistence, detachment, and multi-client collaboration.

## Lessons from tmux and Zellij

### tmux Architecture Insights

Based on research from [tmux source code](https://github.com/tmux/tmux) and [Tao of tmux](https://tao-of-tmux.readthedocs.io/en/latest/manuscript/04-server.html):

1. **Socket Communication**: tmux uses Unix domain sockets in `/tmp` with a lock file mechanism to prevent race conditions during server startup.

2. **Message Protocol**: Uses structured message types (`MSG_IDENTIFY_*`, `MSG_READY`, `MSG_DETACH`, etc.) with clear state transitions (Wait → Attached).

3. **Control Mode**: A text-based protocol (activated with `-C`) wraps all output in `%begin`/`%end` blocks with timestamps and command numbers for reliable parsing.

4. **Flow Control**: Implements `%pause` notifications to prevent overwhelming slow clients - critical for network latency handling.

5. **Known Performance Pitfall**: [Issue #2551](https://github.com/tmux/tmux/issues/2551) documents how excessive `malloc_trim()` calls during grid destruction causes 100% CPU usage with fragmented heaps. **Lesson: Batch memory operations, don't call expensive operations per-grid.**

### Zellij Architecture Insights

From [Zellij's architecture](https://deepwiki.com/zellij-org/zellij):

1. **Protocol Buffers**: Uses `prost` crate for efficient binary serialization - more compact than text protocols.

2. **Typed Message Enums**: `ClientToServerMsg` and `ServerToClientMsg` with clear separation of concerns.

3. **Selective Rendering**: Only changed lines are converted to output - critical for performance.

4. **Thread Specialization**: Dedicated threads for screen rendering, terminal I/O, and plugins prevent blocking.

5. **Error Context Propagation**: Every IPC message includes context for debugging.

## Design Decisions

### Protocol: Binary vs Text

| Approach | Pros | Cons |
|----------|------|------|
| **Text (like tmux -C)** | Human-readable, easy debugging, SSH-friendly | Parsing overhead, escaping complexity |
| **Binary (bincode/protobuf)** | Compact, fast, type-safe | Harder to debug, versioning concerns |

**Decision: Binary with bincode**
- Rust-native, zero-copy deserialization possible
- Simpler than protobuf (no schema files)
- Add `--debug-protocol` flag that logs messages in human-readable format

### Screen Updates: Full vs Incremental

| Approach | Bandwidth | Complexity | Latency |
|----------|-----------|------------|---------|
| **Full screen each frame** | High (80x24 = ~4KB/frame) | Low | Consistent |
| **Damage-based incremental** | Low (only changed rows) | Medium | Variable |
| **Diff-based (like rsync)** | Lowest | High | Higher |

**Decision: Damage-based incremental**
- Already have `DamageTracker` from Phase 4
- Send only dirty row indices + their content
- Fall back to full screen on client reconnect or desync

### Multi-Client Synchronization

When multiple clients attach to the same session:

1. **Input**: Any client's input goes to the focused pane (like tmux)
2. **Output**: Broadcast PTY output to all clients
3. **Resize**: Use smallest client dimensions (or per-client viewports - more complex)
4. **Cursor**: All clients see the same cursor position

**Decision: Smallest-client-wins for resize** (tmux behavior, simpler)

### Session Persistence

| Approach | Pros | Cons |
|----------|------|------|
| **Memory only** | Simple, fast | Lost on server crash |
| **Periodic snapshots** | Crash recovery | Disk I/O, complexity |
| **Write-ahead log** | Full recovery | High complexity |

**Decision: Memory only for MVP**, add snapshots in future phase

## Architecture

```
┌─────────────────────────────────────────────────────────────────┐
│                         clux-server                             │
│                                                                 │
│  ┌─────────────────────────────────────────────────────────┐   │
│  │                    Main Event Loop                       │   │
│  │  mio::Poll watching:                                     │   │
│  │    - Unix socket listener (new client connections)       │   │
│  │    - Client sockets (input from attached clients)        │   │
│  │    - PTY file descriptors (output from shells)           │   │
│  └─────────────────────────────────────────────────────────┘   │
│                              │                                  │
│              ┌───────────────┼───────────────┐                  │
│              ▼               ▼               ▼                  │
│  ┌───────────────┐  ┌───────────────┐  ┌───────────────┐       │
│  │   Session 1   │  │   Session 2   │  │   Session N   │       │
│  │  "default"    │  │   "work"      │  │   "servers"   │       │
│  │               │  │               │  │               │       │
│  │ WindowManager │  │ WindowManager │  │ WindowManager │       │
│  │  └─ Windows   │  │  └─ Windows   │  │  └─ Windows   │       │
│  │     └─ Panes  │  │     └─ Panes  │  │     └─ Panes  │       │
│  │        └─ PTY │  │        └─ PTY │  │        └─ PTY │       │
│  │               │  │               │  │               │       │
│  │ Clients: [1,3]│  │ Clients: [2]  │  │ Clients: []   │       │
│  └───────────────┘  └───────────────┘  └───────────────┘       │
│                                                                 │
└─────────────────────────────────────────────────────────────────┘
        │ Unix Socket: /tmp/clux-{uid}/clux.sock
        │ (or $CLUX_TMPDIR)
        ▼
┌─────────────────────────────────────────────────────────────────┐
│                          clux (client)                          │
│                                                                 │
│  ┌─────────────────────────────────────────────────────────┐   │
│  │                    Main Event Loop                       │   │
│  │  mio::Poll watching:                                     │   │
│  │    - Server socket (screen updates, notifications)       │   │
│  │    - stdin (keyboard/mouse input from user)              │   │
│  │    - signals (SIGWINCH for resize)                       │   │
│  └─────────────────────────────────────────────────────────┘   │
│                              │                                  │
│              ┌───────────────┴───────────────┐                  │
│              ▼                               ▼                  │
│  ┌───────────────────────┐      ┌───────────────────────┐      │
│  │   Input Handler       │      │   Screen Renderer     │      │
│  │   - Keyboard → Server │      │   - Server → Terminal │      │
│  │   - Resize → Server   │      │   - Damage tracking   │      │
│  └───────────────────────┘      └───────────────────────┘      │
│                                                                 │
└─────────────────────────────────────────────────────────────────┘
```

## Protocol Specification

### Message Types

```rust
// src/protocol.rs

use serde::{Deserialize, Serialize};

/// Messages from client to server
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ClientMessage {
    /// Initial handshake with client info
    Hello {
        version: u32,
        term_cols: u16,
        term_rows: u16,
        term_type: String,  // $TERM value
    },
    
    /// Attach to a session (create if doesn't exist)
    Attach {
        session_name: Option<String>,  // None = default session
        create: bool,                   // Create if missing?
    },
    
    /// Detach from current session
    Detach,
    
    /// Keyboard/mouse input
    Input(Vec<u8>),
    
    /// Terminal resize
    Resize { cols: u16, rows: u16 },
    
    /// Command-mode action (split, new window, etc.)
    Command(CommandAction),
    
    /// List all sessions
    ListSessions,
    
    /// Kill a session
    KillSession { name: String },
    
    /// Rename current session
    RenameSession { new_name: String },
    
    /// Heartbeat/keepalive
    Ping,
}

/// Command actions (subset of EventAction for client commands)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum CommandAction {
    SplitHorizontal,
    SplitVertical,
    ClosePane,
    NavigatePane(Direction),
    NewWindow,
    CloseWindow,
    NextWindow,
    PrevWindow,
    SelectWindow(usize),
    Quit,
}

/// Messages from server to client
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ServerMessage {
    /// Handshake response
    HelloAck {
        version: u32,
        server_pid: u32,
    },
    
    /// Successfully attached to session
    Attached {
        session_id: u32,
        session_name: String,
        needs_full_redraw: bool,
    },
    
    /// Detached from session
    Detached { reason: DetachReason },
    
    /// Full screen content (on attach or resync)
    FullScreen {
        rows: Vec<RenderedRow>,
        cursor: CursorState,
        status_line: String,
    },
    
    /// Incremental update (changed rows only)
    Update {
        changed_rows: Vec<(u16, RenderedRow)>,  // (row_index, content)
        cursor: CursorState,
        status_line: Option<String>,  // Only if changed
    },
    
    /// List of sessions
    SessionList(Vec<SessionInfo>),
    
    /// Error response
    Error { message: String },
    
    /// Heartbeat response
    Pong,
    
    /// Server is shutting down
    Shutdown,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum DetachReason {
    ClientRequested,
    SessionClosed,
    ServerShutdown,
    Replaced,  // Another client with same ID attached
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RenderedRow {
    /// Pre-rendered ANSI string for this row
    /// Includes colors, attributes, already escaped
    pub content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CursorState {
    pub row: u16,
    pub col: u16,
    pub visible: bool,
    pub shape: CursorShape,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum CursorShape {
    Block,
    Underline,
    Bar,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionInfo {
    pub id: u32,
    pub name: String,
    pub created_at: u64,  // Unix timestamp
    pub windows: usize,
    pub attached_clients: usize,
}
```

### Wire Format

```
┌─────────────────────────────────────────┐
│  Length (4 bytes, little-endian u32)    │
├─────────────────────────────────────────┤
│  Payload (bincode-serialized message)   │
│  ... variable length ...                │
└─────────────────────────────────────────┘
```

Length-prefixed framing allows:
- Reading exact message boundaries
- Handling partial reads/writes
- Detecting truncated messages

## File Structure

```
src/
├── main.rs              # CLI dispatcher (client commands)
├── lib.rs               # Shared library code
├── protocol.rs          # NEW: Message types and serialization
├── session.rs           # NEW: Session struct
├── server/
│   ├── mod.rs           # NEW: Server module
│   ├── listener.rs      # NEW: Socket listener, client accept
│   ├── client_conn.rs   # NEW: Per-client connection state
│   └── broadcaster.rs   # NEW: Multi-client output broadcast
├── client/
│   ├── mod.rs           # NEW: Client module  
│   ├── connection.rs    # NEW: Server connection
│   └── renderer.rs      # NEW: Screen rendering from ServerMessage
├── bin/
│   ├── clux-server.rs   # NEW: Server binary entry point
│   └── clux.rs          # MOVED: Client binary (was main.rs logic)
├── cell.rs              # MODIFY: Add Serialize/Deserialize
├── config.rs            # Existing
├── event.rs             # Existing (shared by client)
├── grid.rs              # Existing
├── hyperlink.rs         # Existing
├── pane.rs              # Existing
├── pty.rs               # Existing (server-only)
├── render.rs            # MODIFY: Extract row rendering for protocol
├── scrollback.rs        # Existing
├── selection.rs         # Existing
├── terminal.rs          # Existing
└── window.rs            # Existing
```

## Implementation Phases

### Phase 7.1: Protocol Foundation (Day 1)

**Goal**: Define and test message serialization

1. Add `bincode = "1.3"` to Cargo.toml
2. Create `src/protocol.rs` with all message types
3. Add `Serialize`/`Deserialize` to `Cell`, `Color`, `CellFlags`
4. Write unit tests for round-trip serialization
5. Implement length-prefixed read/write helpers

**Acceptance Criteria**:
- All message types serialize/deserialize correctly
- Benchmark: serialization < 1ms for 80x24 screen

### Phase 7.2: Session Abstraction (Day 1)

**Goal**: Wrap WindowManager in Session struct

1. Create `src/session.rs`:
   ```rust
   pub struct Session {
       pub id: SessionId,
       pub name: String,
       pub window_manager: WindowManager,
       pub created_at: Instant,
       attached_clients: Vec<ClientId>,
   }
   ```
2. Add methods for client attach/detach
3. Implement session naming and lookup

**Acceptance Criteria**:
- Session wraps WindowManager cleanly
- Multiple sessions can coexist in memory

### Phase 7.3: Server Skeleton (Day 2)

**Goal**: Server process that accepts connections

1. Create `src/bin/clux-server.rs` entry point
2. Create `src/server/mod.rs` with Server struct
3. Implement Unix socket listener with mio
4. Handle client accept, store in HashMap
5. Implement lock file for single-server guarantee
6. Add signal handling (SIGTERM, SIGINT)

**Acceptance Criteria**:
- `clux-server` starts and listens on socket
- Multiple clients can connect (no message handling yet)
- Clean shutdown on signals

### Phase 7.4: Client Skeleton (Day 2)

**Goal**: Client process that connects to server

1. Create `src/bin/clux.rs` entry point (move logic from main.rs)
2. Create `src/client/mod.rs` with Client struct
3. Implement socket connection with retry
4. Auto-start server if not running
5. Send Hello, receive HelloAck

**Acceptance Criteria**:
- `clux` connects to running server
- `clux` starts server automatically if needed
- Hello/HelloAck handshake works

### Phase 7.5: Input Forwarding (Day 3)

**Goal**: Keyboard input flows client → server → PTY

1. Client: capture keyboard events, send as `Input` messages
2. Server: receive Input, write to focused pane's PTY
3. Handle resize events (client → server → PTY resize)

**Acceptance Criteria**:
- Typing in client appears in shell
- Terminal resize propagates correctly

### Phase 7.6: Output Rendering (Day 3-4)

**Goal**: PTY output flows server → client → screen

1. Server: read PTY output, parse through VTE, update terminal state
2. Server: detect dirty rows via DamageTracker
3. Server: serialize dirty rows as `Update` messages
4. Server: send `FullScreen` on initial attach
5. Client: receive messages, render to host terminal

**Acceptance Criteria**:
- Shell output displays correctly
- Colors and attributes preserved
- Incremental updates work (not full screen every frame)

### Phase 7.7: Session Management (Day 4)

**Goal**: Full session lifecycle

1. Implement `Attach` with session creation
2. Implement `Detach` (client-initiated)
3. Implement `ListSessions`
4. Implement `KillSession`
5. Add session cleanup when last client detaches (optional auto-close)

**Acceptance Criteria**:
- `clux` attaches to default session
- `clux new work` creates named session
- `clux attach work` attaches to existing
- `clux list` shows all sessions
- `clux kill work` terminates session

### Phase 7.8: Multi-Client Support (Day 5)

**Goal**: Multiple clients on same session

1. Broadcast PTY output to all attached clients
2. Handle resize: use smallest client dimensions
3. Handle detach: notify remaining clients of resize
4. Test concurrent input from multiple clients

**Acceptance Criteria**:
- Two terminals can attach to same session
- Both see same output
- Input from either works
- Resize uses smallest dimensions

### Phase 7.9: Command Mode (Day 5)

**Goal**: Keybindings work through client-server

1. Client: detect prefix key, enter command mode
2. Client: send `Command` messages for actions
3. Server: execute commands, update session state
4. Add new keybinding: `d` for detach

**Acceptance Criteria**:
- All existing keybindings work
- Prefix + d detaches
- Split/navigate/window commands work

### Phase 7.10: Polish and Edge Cases (Day 6)

**Goal**: Production-ready robustness

1. Handle client disconnect gracefully
2. Handle server crash (client shows error, exits)
3. Add `--debug-protocol` flag for troubleshooting
4. Add heartbeat/keepalive for connection health
5. Test with high-latency (simulated)
6. Memory leak testing with valgrind

**Acceptance Criteria**:
- No crashes on disconnect
- Clean error messages
- Reasonable memory usage over time

## Performance Targets

| Metric | Target | Measurement |
|--------|--------|-------------|
| Input latency | < 5ms | Time from keypress to PTY write |
| Render latency | < 16ms | Time from PTY read to client display |
| Full screen serialize | < 2ms | 80x24 screen to wire format |
| Incremental update | < 0.5ms | Single row change to wire |
| Memory per session | < 50MB | With 10k scrollback |
| Idle CPU | < 1% | No activity |

## Risk Mitigation

### Race Conditions

**Risk**: Multiple clients sending input simultaneously

**Mitigation**: 
- Server processes input sequentially (single-threaded)
- Input order preserved per-client via socket ordering

### Memory Leaks

**Risk**: Sessions accumulate memory over time (like tmux #2551)

**Mitigation**:
- Don't call `malloc_trim()` per-operation
- Batch cleanup operations
- Add metrics logging for memory usage

### Socket Buffer Overflow

**Risk**: Slow client can't keep up with fast PTY output

**Mitigation**:
- Implement backpressure: pause sending if client buffer > threshold
- Drop frames rather than queue infinitely
- Log warnings when dropping frames

### Partial Writes

**Risk**: Large messages may not write atomically

**Mitigation**:
- Use write buffer per client
- Track write progress
- Only process next message when previous fully sent

### Server Crash Recovery

**Risk**: Server dies, sessions lost

**Mitigation**: (future phase)
- Periodic session state snapshots
- Server restart reads snapshots
- Clients auto-reconnect

## CLI Design

```bash
# Default: attach to "default" session (create if needed)
clux

# Create new session with name
clux new [session-name]

# Attach to existing session
clux attach <session-name>

# List all sessions
clux list
# Output:
# NAME      WINDOWS  CREATED      ATTACHED
# default   2        10 min ago   1 client
# work      3        2 hours ago  0 clients

# Kill a session
clux kill <session-name>

# Kill server (all sessions)
clux kill-server

# Show server status
clux info
# Output:
# Server PID: 12345
# Socket: /tmp/clux-501/clux.sock
# Sessions: 3
# Clients: 2

# Start server explicitly (usually auto-started)
clux-server

# Server with debug logging
clux-server --debug
```

## New Keybindings

| Key (after prefix) | Action | Description |
|-------------------|--------|-------------|
| `d` | Detach | Disconnect from session (session continues) |
| `$` | Rename | Rename current session |
| `s` | Switch | Show session switcher (future) |

## Testing Strategy

### Unit Tests

- Protocol serialization round-trips
- Session attach/detach logic
- Multi-client resize calculation

### Integration Tests

- Start server, connect client, verify handshake
- Send input, verify PTY receives
- Read PTY output, verify client receives
- Detach and reattach
- Multiple clients on same session

### Stress Tests

- 100 rapid connect/disconnect cycles
- High-volume PTY output (`cat /dev/urandom | base64`)
- 10 clients on same session

### Manual Tests

- SSH to remote, run clux, close terminal, reattach
- Run vim in session, detach, reattach, verify state
- Two terminals side-by-side on same session

## Dependencies Update

```toml
# Cargo.toml additions
bincode = "1.3"
# serde already present

# For CLI argument parsing (if not using clap already)
# clap = { version = "4.0", features = ["derive"] }
```

## Migration Path

The existing single-binary `clux` becomes two binaries:

1. `clux` - The client (lightweight, connects to server)
2. `clux-server` - The server (manages sessions, PTYs)

Backward compatibility: Running `clux` without a server starts one automatically, making the transition seamless for users.

## Timeline Estimate

| Phase | Description | Days |
|-------|-------------|------|
| 7.1 | Protocol Foundation | 0.5 |
| 7.2 | Session Abstraction | 0.5 |
| 7.3 | Server Skeleton | 1 |
| 7.4 | Client Skeleton | 1 |
| 7.5 | Input Forwarding | 0.5 |
| 7.6 | Output Rendering | 1.5 |
| 7.7 | Session Management | 1 |
| 7.8 | Multi-Client Support | 1 |
| 7.9 | Command Mode | 0.5 |
| 7.10 | Polish | 1.5 |
| **Total** | | **~9 days** |

## Success Criteria

- [ ] Server starts and manages multiple sessions
- [ ] Client connects, attaches, and renders correctly
- [ ] Detach with Prefix+d, session persists
- [ ] Reattach to existing session
- [ ] `clux list` shows all sessions
- [ ] Multiple clients can attach to same session
- [ ] All existing keybindings work through protocol
- [ ] Performance targets met
- [ ] No memory leaks in 24-hour stress test
- [ ] Clean error handling on disconnect/crash
