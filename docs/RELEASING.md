# Releasing

`clux` remote bootstrap downloads `clux-server` from GitHub Releases. A release is not just distribution metadata; it is part of the runtime contract for `clux --remote`.

## Release Checklist

1. Update the package version in `Cargo.toml`.
2. Ensure `cargo fmt --check` and `cargo test -q` pass locally.
3. Merge the release commit to `main`.
4. Create an annotated tag:

```bash
git tag -a vX.Y.Z -m "vX.Y.Z"
```

5. Push the tag:

```bash
git push origin vX.Y.Z
```

6. Wait for the `Release` GitHub Actions workflow to finish.
7. Verify the GitHub Release contains:
   - `clux-server-vX.Y.Z-x86_64-unknown-linux-gnu.tar.gz`
   - `clux-server-vX.Y.Z-aarch64-unknown-linux-gnu.tar.gz`
   - `SHA256SUMS`

## Release Contract

Remote bootstrap resolves assets from:

```text
https://github.com/carTloyal123/clux/releases/download/v<version>/clux-server-v<version>-<target>.tar.gz
```

Supported remote targets:

- `x86_64-unknown-linux-gnu`
- `aarch64-unknown-linux-gnu`

Each release archive must contain `clux-server` at archive root.

## Workflow Behavior

The release workflow validates:

- the git tag matches `v<package.version>`
- the package name is `clux`
- `Cargo.toml` repository metadata is `https://github.com/carTloyal123/clux`

If any of these do not match, the release fails before any artifacts are published.

## Verifying A Release

After the release workflow succeeds:

1. Open the GitHub Release page for the tag.
2. Confirm both Linux tarballs are present.
3. Confirm `SHA256SUMS` is present.
4. Download one archive and verify it unpacks to a single `clux-server` binary.
5. Test a first-time remote bootstrap on a clean Ubuntu host:

```bash
clux --remote <host> new
```

## If A Target Build Fails

- check the failing GitHub Actions job logs
- rerun the workflow only after fixing the build on `main`
- push a corrected tag only after `Cargo.toml`, the git tag, and the intended release version all match
