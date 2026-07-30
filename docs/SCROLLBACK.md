# Scrollback

Each pane records the lines that scroll off the top of the screen, up to a memory
budget (16 MB by default, about 8,700 rows at 80 columns), and the client can scroll
back through them.

## How it works

Terminal content lives in one paged buffer ([`src/buffer/`](../src/buffer/)); the
screen is the last rows of it and history is everything before. A viewport row maps
onto either, through one function:

```text
offset = 0          offset = 2
┌──────────┐        ┌──────────┐
│ grid[0]  │        │ sb[1]    │  <- older
│ grid[1]  │        │ sb[0]    │
│ grid[2]  │        │ grid[0]  │
│ grid[3]  │        │ grid[1]  │
└──────────┘        └──────────┘
```

Both the live path and the scrolled path go through that one mapping, so the two
views cannot drift apart - the failure mode where a scrolled pane renders stale or
mixed rows. See [PAGED_BUFFER.md](PAGED_BUFFER.md) for the storage design.

The scroll offset lives on the server, with the terminal state. That keeps it
consistent for every attached client and means the client needs no history of its
own: it asks for a scroll and gets ordinary `PaneUpdate` rows back.

## Controls

| Input | Effect |
| --- | --- |
| Mouse wheel | Scroll 3 lines (when the application is not using the mouse; Shift overrides one that is) |
| `<prefix> PageUp` / `PageDown` | Scroll 20 lines |
| `<prefix> End` | Return to the live view |
| Typing anything | Returns to the live view |

`ClientMessage::Scroll { lines }` is positive to go back in history, negative to
come forward, and zero to return to the live view.

## Behaviour worth knowing

- **The view stays pinned.** When new output arrives while you are scrolled back,
  the offset advances with it, so you keep looking at the same content instead of
  drifting a line at a time.
- **No cursor while scrolled.** The cursor belongs to the live view, so it is
  hidden until you return.
- **Alt-screen applications are unaffected.** Full-screen programs (vim, less) do
  not push to the scrollback, so the wheel keeps going to them.
- **Selection works in a scrolled view** - the rows are ordinary pane rows. A
  selection cannot span the scroll boundary; see [SELECTION.md](SELECTION.md).

## Links in history

Link resolution reads the view, not the grid, so a URL you scroll back to is still
clickable - both application OSC 8 links and URLs clux detected itself.

That works because history and the screen are the same storage: `resolve_links`
reads the buffer's viewport, whatever it is currently showing.

## Known gaps

- **No search.** There is no way to search history yet; it would run over the
  buffer's rows the same way link resolution does.
- **Selection stops at the viewport edge.** A drag cannot span the boundary
  between what is on screen and what is scrolled above it - see
  [SELECTION.md](SELECTION.md).
- **Nothing outstanding on reflow.** Resizing re-wraps history along with the
  screen, and the cursor keeps its character.
