# clux

A terminal multiplexer inspired by tmux.

## Install

macOS (Apple Silicon and Intel), via Homebrew:

```sh
brew install carTloyal123/clux/clux
```

This installs prebuilt binaries from the [latest release](https://github.com/carTloyal123/clux/releases/latest);
no compiler needed. The formula lives in
[carTloyal123/homebrew-clux](https://github.com/carTloyal123/homebrew-clux) and is
bumped automatically on every release.

## Usage

`clux` starts a local `clux-server` on first use and attaches to a session:
`clux` to begin, `clux ls` to list sessions.

Highlights: real OSC 8 hyperlinks over wrap-joined lines (bare URLs included),
client-side mouse selection that copies through OSC 52 (works locally and over
SSH), and per-pane scrollback (wheel or `<prefix> PageUp`/`PageDown`).

Design notes for contributors live in [AGENTS.md](AGENTS.md).
