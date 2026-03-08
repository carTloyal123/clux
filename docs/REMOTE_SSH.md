# Remote SSH Mode

`clux` can now run its UI locally while keeping `clux-server` and all PTYs on a remote Unix machine.

## Architecture

Remote mode uses SSH-managed Unix socket forwarding:

- `clux` runs on your local machine
- `clux-server` runs on the remote machine
- the local client creates an SSH tunnel from a temporary local Unix socket to the remote `clux-server` socket
- rendering, keyboard input, mouse input, and resize handling stay local
- shells, panes, windows, and detached sessions stay remote

This keeps the frontend responsive while preserving the normal detached-session workflow.

## Requirements

Before using remote mode:

- `ssh` must be available locally
- the remote machine must be Unix-like and support Unix sockets
- the remote machine must have either `curl` or `wget`, plus `tar`
- a matching `clux-server` release artifact must exist in `carTloyal123/clux` GitHub Releases for the remote target
- your local SSH config and auth must already work for the target host

## First-Time Use

Create and attach to a remote session:

```bash
clux --remote devbox new
```

Attach to an existing remote session:

```bash
clux --remote devbox attach
clux --remote devbox attach work
```

List remote sessions:

```bash
clux --remote devbox list
```

Show remote server info:

```bash
clux --remote devbox info
```

Stop the remote server cleanly:

```bash
clux --remote devbox kill-server
```

## Socket Paths

By default, the remote server socket is:

```text
/tmp/clux-$UID/clux.sock
```

Override it with `--socket`:

```bash
clux --remote devbox --socket /tmp/clux-alt.sock new
```

In local mode, `--socket` still refers to the local server socket.

## What Happens On Connect

When you run `clux --remote <host> ...`:

1. `clux` probes the remote OS and architecture over SSH.
2. If needed, `clux` downloads and installs a managed `clux-server` under `~/.local/share/clux/server/<version>/`.
3. `clux` starts that managed remote server on the requested socket.
4. `clux` starts a persistent SSH tunnel to the remote server socket.
5. The local client connects to the forwarded local socket and runs the normal handshake.

The temporary forwarded socket is local-only and is removed when the client exits.
The remote managed server install is reused on later connections for the same local `clux` version.

## Release Compatibility

Remote bootstrap currently supports these remote targets:

- `x86_64-unknown-linux-gnu`
- `aarch64-unknown-linux-gnu`

The local `clux` version must have a matching GitHub Release tag in `carTloyal123/clux`, with assets named:

- `clux-server-v<version>-x86_64-unknown-linux-gnu.tar.gz`
- `clux-server-v<version>-aarch64-unknown-linux-gnu.tar.gz`

## Common Workflow

Start work on a remote host:

```bash
clux --remote devbox new work
```

Detach with the normal keybinding:

```text
Alt+C d
```

Reconnect later from your local machine:

```bash
clux --remote devbox attach work
```

## Troubleshooting

If remote mode does not connect:

- verify `ssh devbox` works outside of `clux`
- verify the remote machine has `curl` or `wget`, plus `tar`
- verify a release artifact exists for your remote target
- verify the remote socket path matches the one you expect
- run `clux --remote devbox info` to distinguish "server not running" from attach/session issues

If `kill-server` fails with a protocol/version error, your local `clux` and remote `clux-server` binaries are out of sync.
