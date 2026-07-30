# Selection and copy

How mouse selection works, why it lives in the client, and what it deliberately
does not do yet.

## Why clux has to implement this at all

Clux enables mouse reporting (modes 1000/1002/1003) so applications inside panes
can use the mouse. That takes the host terminal's own selection away from you
inside a clux session — Ghostty hands the events to clux instead of drawing a
selection. So selecting has to work in clux or it does not work at all.

## Where it lives

Entirely in the client. The client already owns the composited screen
(`ScreenBuffer`), the layout, and the mouse events, so selection needs **no
server round-trip and no new messages** — selecting and copying never touch the
server.

The one thing the client cannot know by itself is where a pane's logical lines
end, because every row it paints looks hard-wrapped. So `PaneRow` carries a
`wrapped` flag (protocol v6, one byte per row), and the client joins wrapped rows
when extracting text. Without it, copying a long path or URL splices a newline
into the middle of it — the same root cause as the hyperlink problem in
[HYPERLINKS.md](HYPERLINKS.md).

## Behaviour

| Gesture | Result |
| --- | --- |
| Left drag, app not using the mouse | Select text |
| Shift + left drag, app using the mouse | Select text (xterm/Ghostty convention) |
| Alt + left drag | Rectangular (block) selection |
| Release | Copy to the host clipboard (`copy_on_select`, default on) |
| Any keypress | Selection cleared |
| Click a divider or the border | Selection cleared |
| Layout change / resize | Selection cleared (positions are meaningless afterwards) |

Selections are anchored to the pane where the drag started: dragging across a
divider extends within that pane rather than splicing in the neighbour's text.
Rows that soft-wrap join without a newline; hard line ends are trimmed of
trailing blanks. Block selections stay columnar, one line per row.

Highlighting is applied at render time (`INVERSE` on the selected cells) rather
than baked into the stored cells, so clearing a selection needs no restore and new
output on other rows does not disturb it.

## Copy: OSC 52, no native clipboard

The client writes `ESC ] 52 ; c ; <base64> ESC \` to the host terminal, which owns
the system clipboard. That is one code path for every platform, and it keeps
working when the client itself is on the far end of an SSH session — a native
clipboard crate cannot do that. Base64 is ~20 lines in `src/clipboard.rs`;
`arboard` and its `objc2-app-kit` dependency tree were removed. See AGENTS.md.

Copies are capped at `MAX_COPY_BYTES` (64 KiB) because terminals cap what they
accept, and a silently dropped sequence is worse than a refusal.

Paste needs no code: the host terminal pastes into the pty as bracketed paste and
the client forwards it.

## Configuration

```toml
[selection]
copy_on_select = true   # copy on mouse release
```

## Not implemented yet

- **Selecting while scrolled back** works (the rows are ordinary pane rows), but
  a selection cannot span the boundary between what is on screen and what is
  above it - drag, scroll, drag again selects only within the current view.
- **Double-click word / triple-click line.** `find_word_bounds` in
  `src/selection.rs` is ready, but crossterm does not report click counts, so this
  needs click-timing in the client first.
- **Explicit copy-mode keybindings.** Selection is mouse-only today.
