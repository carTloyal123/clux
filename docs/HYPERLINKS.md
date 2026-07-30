# Hyperlinks

Why link-following breaks inside a multiplexer, what clux does about it, and how
to check it is working.

## Why tmux links are bad in Ghostty

Three separate failures stack up. Only the first is specific to Ghostty.

### 1. tmux strips OSC 8 because `xterm-ghostty` has no `Hls`

tmux only emits an OSC 8 hyperlink if the outer terminal's terminfo entry
advertises the extended `Hls` capability (that is what the `hyperlinks` entry in
`terminal-features` sets). Ghostty's terminfo does not have it:

```console
$ infocmp -x xterm-ghostty | tr ',' '\n' | grep -E '^\s*(Hls|Su|Sync|Smulx)'
 Su
 Smulx
 Sync
```

tmux also ships a built-in feature table for terminals it recognises by name, but
in 3.6a that table covers `mintty`, `iTerm2` and `foot` - not Ghostty. So with
`TERM=xterm-ghostty`, tmux silently drops every hyperlink an application emits.

If you are stuck in tmux, this is the workaround:

```tmux
set -as terminal-features ",xterm-ghostty:hyperlinks"
```

### 2. The host terminal cannot see a multiplexer's soft wraps

Ghostty (like WezTerm and iTerm2) also finds bare URLs itself, by matching
against its own grid. A multiplexer paints every row with an absolute cursor
move, so no row in the host's grid is ever marked as a wrap continuation. A URL
that wraps inside a pane therefore looks like two unrelated fragments, and
neither fragment is the real URL. Split panes make it worse: the URL is clipped
at the pane border, and a match can run across the divider into the neighbouring
pane's text.

No `terminal-features` setting fixes this one. The multiplexer is the only
process that knows where its logical lines end.

### 3. clux used to drop hyperlinks entirely

Worth stating plainly: before this change clux was worse than tmux here. The
only OSC 8 emitter in the tree lived in a renderer (`src/render.rs`) that was
reachable only from a standalone entry point (`src/main.rs`) which was not a build
target - both since deleted, see AGENTS.md. The shipped client renders rows
through `src/client/screen.rs`, which ignored `Cell::hyperlink`, and the wire
protocol carried a hyperlink id with no URL table to resolve it against.

That is the concrete cost of a second rendering path, and the reason clux is
client/server only now.

## What clux does now

Link resolution happens server-side, per pane, in `src/urls.rs`:

- **Explicit runs** - cells an application marked with OSC 8.
- **Detected runs** - URL-shaped text found in *wrap-joined logical lines*, so a
  URL that wraps mid-path is one link, and a link never runs past a pane border.
  Cells that already carry an explicit hyperlink are excluded, so the
  application's target always wins over the displayed text.

Runs travel to the client as `PaneRow::links` and are emitted by
`ScreenBuffer::render_row_ansi` as:

```text
ESC ] 8 ; id=<id> ; <url> ESC \   ...text...   ESC ] 8 ; ; ESC \
```

Two details that make it work in practice:

- **Every run of one logical link shares an `id`.** That is what lets the host
  terminal treat fragments split across rows as a single link for hover
  highlighting; Ghostty groups link cells by `(id, uri)`. Ids are salted with the
  pane id, so two panes never collide, and derived deterministically from the
  link's anchor cell so a partial repaint regenerates the same id.
- **When a link wraps, every row it covers is repainted**, not just the dirty
  one. Otherwise the head row keeps whatever fragment it was given before the
  line grew.

Clux emits OSC 8 unconditionally - it does not gate on the host terminal's
terminfo, which is failure 1 above.

## Styling

A URL clux detected itself gets an underline, because nothing else marks it as a
link: the application printed plain text and doesn't know it became clickable.

An application's own OSC 8 link is never restyled. It already chose how its links
should look, and overriding that (tmux and clux's deleted standalone renderer both
forced blue + bold) throws away information the application deliberately encoded
in colour.

Either way the host terminal still applies its own hover highlight.

URLs coming from application output are sanitized: control characters are
stripped and the length is capped. A URL is re-emitted verbatim inside an OSC 8
sequence, so an embedded `ESC` would otherwise let program output inject
arbitrary escape sequences into the host terminal.

## Configuration

Detection of bare URLs is on by default. To leave it to the host terminal:

```toml
[links]
auto_detect = false
```

Explicit OSC 8 hyperlinks from applications are always forwarded.

## Verifying

Inside clux:

```console
$ printf 'go to https://example.com/a/quite/long/path/that/wraps?q=1\n'
```

Cmd-click (Ghostty's default modifier) anywhere on the URL, including past the
wrap point. For an application-emitted link:

```console
$ printf '\033]8;;https://example.com/osc8\033\\CLICKME\033]8;;\033\\\n'
```

Both cases are covered by tests: `src/urls.rs` unit tests for the detector,
`src/client/screen.rs` for the emitted bytes, and `tests/integration.rs`
(`test_plain_url_becomes_a_real_hyperlink`,
`test_wrapped_url_is_one_link_across_rows`,
`test_application_osc8_hyperlink_reaches_the_host_terminal`) end to end against a
real server process.

## Known gaps

- A URL that is partly scrolled off the top of a pane is only linked over the rows
  still on screen. Scroll back to it and the whole link resolves again - see
  [SCROLLBACK.md](SCROLLBACK.md).
- `id=` parameters supplied by an application are not preserved; clux groups by
  interned URL instead, so two identical URLs from one application are treated as
  one logical link.
