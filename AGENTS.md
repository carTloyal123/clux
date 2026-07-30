# AGENTS.md

Notes for agents (and humans) working in this repo.

## Pre-1.0: compatibility is not a constraint

There will be no release until **1.0.0**. Until then:

- **Break the wire protocol freely.** Bump `PROTOCOL_VERSION` in `src/protocol.rs`
  when the format changes and move on. Adding a field to a bincode message is a
  wire break, and that is fine.
- **Server/client version mismatches do not matter.** A stale `clux-server` left
  running against a newer client is a local annoyance, not a compatibility
  problem — `clux kill-server` and reattach. Don't add negotiation shims,
  `#[serde(default)]` padding, or dual-format decoding to keep old builds
  talking to new ones.
- **No deprecation paths for config, CLI flags, or protocol messages.** Rename or
  delete them outright rather than carrying an alias.

Prefer the clean design over the backwards-compatible one. Compatibility work
starts at 1.0.0.

## Minimal and simple: YAGNI, measure twice cut once

The goal is the smallest tree that does the job well. Two rules, and they pull in
opposite directions on purpose:

**YAGNI when writing.** Don't build the general case, the second backend, the
config knob nobody asked for, or the "we'll need this later" API. Speculative code
is not free: it has to be read, kept compiling, and kept consistent with the code
that does ship - and when it drifts, it becomes a trap. Everything in this repo
that has cost real debugging time was speculative: a second renderer that shipped
no pixels, a second input pipeline with its own global state, a hyperlink store
with refcounting nothing calls.

**Measure twice, cut once when deleting.** Before removing something, prove it is
unreachable rather than assuming: grep for callers across `src/`, `tests/` and
`benches/`, check `cargo metadata` for whether the file is even a build target,
and check the CI/release workflows. Note that `pub` items in the library never
trigger dead-code warnings, so the compiler will not tell you. Then delete
outright - no `#[deprecated]`, no commented-out block, no "keep for reference".
Git history is the archive.

Corollary: **don't add `#![allow(dead_code)]` to a module.** It silences the one
signal that would have caught all of the above. If something is genuinely unused
today, either delete it or leave the warning visible as a to-do. Remove the
existing module-level allows as each module gets cleaned up.

## Small files: 200 lines, hard cap

**No `.rs` file exceeds 200 lines, tests included.** A file that has to be paged
through hides things - three ANSI emitters and two input pipelines all lived in
this repo unnoticed because they were buried in thousand-line files.

Rules:

- **New files: 200 lines is a hard cap, no exceptions.** If a module is growing
  past it, that is the signal to split by responsibility, not to keep appending.
- **Touching an oversized file means splitting it**, at least far enough that the
  part you touched lands in a file under the cap. Don't add lines to a file that
  is already over.
- Split by responsibility, not by line count: a module directory (`window/`
  holding `manager.rs`, `panes.rs`, `window.rs`) beats `foo1.rs`/`foo2.rs`. Impl
  blocks can be spread across files in the same module directory; make shared
  fields `pub(super)`.
- Unit tests count toward the file's budget. A module whose tests push it over is
  usually a module doing two things.

Current debt, largest first - these predate the rule and are to be split as they
are touched:

| File | Lines | Suggested split |
| --- | --- | --- |
| `tests/integration.rs` | ~2270 | `tests/` by area: harness, lifecycle, panes, windows, hyperlinks, selection, scrollback, remote |
| `src/server/mod.rs` | ~1840 | `server/` : `lifecycle.rs`, `clients.rs`, `commands.rs`, `panes.rs`, `broadcast.rs` |
| `src/client/screen.rs` | ~1550 | `screen/` : `buffer.rs`, `links.rs`, `selection.rs`, `ansi.rs` |
| `src/bin/clux.rs` | ~1500 | `client/` : `cli.rs`, `attach.rs` (event loop), `input.rs`, `border.rs` |
| `src/protocol.rs` | ~1370 | `protocol/` : `messages.rs`, `rows.rs`, `framing.rs` |
| `src/terminal.rs` | ~1200 | `terminal/` : `state.rs`, `perform.rs` (the VTE impl), `modes.rs` |
| `src/config.rs` | ~980 | `config/` : `keys.rs`, `bindings.rs`, `sections.rs` |

Everything else over the cap (`session`, `grid`, `pane`, `urls`, `client/remote`,
`client/mod`, `cell`, benches) is smaller and can be split in passing.

## One architecture: always client/server

There is no single-process mode, and adding one back is not on the table. The
`clux` client spawns a local `clux-server` on first use; the server owns every
session's PTYs, terminal state, panes and link resolution; the client composites
`PaneUpdate` messages into a `ScreenBuffer` and paints the host terminal.

The server's lifetime is driven by sessions, not clients (`AutoShutdownConfig`):

| Event | Result |
| --- | --- |
| First client, no server running | Client spawns `clux-server` (sibling binary, else `$PATH`) |
| Client detaches | Session lives on, server stays up for reattach |
| Client disconnects or crashes | Same as detach - the session survives |
| Last session closes (`<prefix> q`, or the last shell exits) | Server shuts down after `grace_period` |
| Server started but no session within `first_session_timeout` | Server shuts down (orphan cleanup) |

Those five rows are covered by the `Server Lifecycle Tests` section of
`tests/integration.rs` against real server processes. If you change session or
client teardown, run them.

A second renderer is how clux ended up shipping a client that silently dropped
OSC 8 hyperlinks: the working implementation lived in an entry point that was not
a build target. Deleted in that cleanup:

- `src/main.rs` - standalone entry point, shadowed by the explicit
  `[[bin]] name = "clux"` and therefore never compiled
- `src/render.rs` - the renderer only that entry point used

Anything worth keeping from them is in git history; the synchronized-output
handling was ported to the client (`BEGIN_SYNC_UPDATE`/`END_SYNC_UPDATE`, DECSET
2026 rather than the legacy iTerm2 DCS form those files used).

A follow-up audit removed the rest of what that architecture left behind:
`src/event.rs` (1,187 lines, of which one function shipped - now
`client::mouse::encode_mouse_sgr`), `Terminal::render_row`/`render_row_plain` (a
third ANSI emitter), the hyperlink store's unused refcounting and `open_url`
process-spawn path, and the speculative `CursorShape` / `needs_full_redraw`
protocol fields.

## Remote access: one path, no Python

Remote mode reaches the server through `ssh -L localsock:remotesock`. When that
forwarding is unavailable the client falls back to `clux-server --stdio-bridge
<socket>` over `ssh -T`, which pumps the ssh pipe against the server's Unix
socket.

The bridge is a **mode of `clux-server`**, not a helper binary, because the
bootstrap already installs `clux-server` on the remote host: one binary, one
implementation, nothing extra to ship or install. It replaced a Python script that
the client used to write to the remote host at runtime via a heredoc - an
undeclared dependency on remote Python 3 and a second implementation of the same
idea. Don't reintroduce a remote scripting-language dependency.

`src/bin/clux-test-forwarder.rs` is a **test fixture, not a product binary**: the
remote-SSH tests use it as a stand-in for `ssh -L` socket forwarding. Releases ship
`clux` and `clux-server` only.

`src/selection.rs` and `src/clipboard.rs` are now both live: mouse selection and
OSC 52 copy run in the client, with no server round-trip. See
[docs/SELECTION.md](docs/SELECTION.md).

## Storage model: one buffer, screen as a window

All terminal content lives in `src/buffer/`: a deque of fixed-width pages, where the
last `screen_rows` rows are the active screen and everything before is history. There
is no separate grid or scrollback - that split is what made resize unable to re-wrap
history and forced a trait to read rows uniformly. See
[docs/PAGED_BUFFER.md](docs/PAGED_BUFFER.md).

Rules that keep it that way:

- **Read rows through `Terminal::view_row` or `Buffer::row_cells`**, never by
  reaching into pages. `terminal.rs` is the only module that touches storage
  internals; keeping it that way is what made replacing the storage a contained
  change (~50 call sites, one file).
- **The screen is a window, not a thing.** Anything that wants "row 3" means row 3
  of the active area or of the viewport - be explicit about which.
- **Absolute row numbers are the only stable reference.** A `Pin` is an absolute row;
  page indices and viewport rows are not stable across scrolling or reflow.
- **Benchmark storage changes.** `benches/buffer.rs` and `benches/terminal.rs` exist
  because two performance regressions in this work (an O(pages) row lookup, and
  double dirty-marking) were invisible to the tests and obvious to the benches.

## Dependencies: stay lean

Every dependency has to justify itself against doing it in-tree. Prefer
**terminal-native protocols over platform toolkits**: the host terminal is already
a capable, cross-platform API, and a multiplexer's client often runs somewhere
without a window server at all.

Applied so far:

- **Clipboard is OSC 52, not `arboard`.** The client writes an OSC 52 sequence to
  the host terminal, which owns the system clipboard. That works identically on
  macOS, Linux and Windows terminals, and - unlike a native clipboard crate - it
  still works when the client itself is on the far end of an SSH session. It also
  removed the whole `arboard`/`objc2-app-kit` tree. Revisit only if manual testing
  turns up terminals where it falls short (some disable clipboard writes by
  default); the fallback would be conditional, not the default.
- **Base64 for OSC 52 is ~20 lines in `src/clipboard.rs`** rather than a crate.
- **Paste needs no code.** The host terminal pastes into the pty as bracketed
  paste; the client just forwards `Event::Paste`.

## Known defect: the server writes to clients synchronously

`ClientConnection::send_message` flips the socket to blocking mode for each write,
on a single-threaded event loop. A client that stops reading therefore stalls the
**whole server**, every session included, until it resumes or disconnects. It is
observable: a client that attaches and then does not drain makes `handle_attach`
block for as long as it stays quiet, and input it already sent can be lost when
it disconnects.

The fix is a per-client output queue plus `Interest::WRITABLE` registration, not
more blocking writes. Until then, don't write tests that attach a client and go
quiet - drain messages the way the real client loop does.
