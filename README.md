# clux
A terminal emulator inspired by TMUX.

## Hyperlinks

Clux resolves links itself instead of hoping the host terminal can: OSC 8 links
from applications are always forwarded, and bare URLs are turned into real
hyperlinks over wrap-joined logical lines, so a URL that wraps inside a pane is
still one clickable link. See [docs/HYPERLINKS.md](docs/HYPERLINKS.md) for why
this is broken in tmux (especially under Ghostty) and how clux fixes it.

## Selection and copy

Mouse selection is client-side and copies through OSC 52, so it works the same
locally and over SSH with no native clipboard dependency. Wrapped lines copy as
one line. See [docs/SELECTION.md](docs/SELECTION.md).

## Scrollback

Each pane keeps 10,000 lines of history. Wheel or `<prefix> PageUp`/`PageDown`
scrolls back, typing returns to the live view, and the view stays pinned when new
output arrives. See [docs/SCROLLBACK.md](docs/SCROLLBACK.md).
