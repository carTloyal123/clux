# Releasing

`clux` remote bootstrap downloads `clux-server` from GitHub Releases. A release is not just distribution metadata; it is part of the runtime contract for `clux --remote`.

## Release Checklist

1. Update the package version in `Cargo.toml`.
2. Ensure `cargo fmt --check` and `cargo test -q` pass locally.
3. Merge the release commit to `main`.
4. Wait for the `CI` workflow on `main` to:
   - create the annotated tag `vX.Y.Z` if it does not already exist
   - invoke the release workflow in the same run
5. Verify the GitHub Release contains:
   - `clux-server-vX.Y.Z-x86_64-unknown-linux-gnu.tar.gz`
   - `clux-server-vX.Y.Z-aarch64-unknown-linux-gnu.tar.gz`
   - `SHA256SUMS`

## Automatic Tagging And Release

On pushes to `main`, the `CI` workflow checks `Cargo.toml` and computes `v<version>`.

- If that tag does not exist yet, CI creates and pushes it.
- CI then calls the release workflow directly and publishes the GitHub Release assets.
- If the tag already exists and the GitHub Release already exists, CI skips the automatic release path.
- If the tag already exists but the GitHub Release is missing, CI republishes the release without changing the tag.

This avoids relying on a second workflow triggered by the tag push itself.

## Manual Release Fallback

Manual tag pushes still work:

```bash
git tag -a vX.Y.Z -m "vX.Y.Z"
git push origin vX.Y.Z
```

That path uses the standalone `Release` workflow directly.

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

The CI workflow also validates release readiness before it creates a tag:

- tests pass on `main`
- release-target cross-builds pass on `main`
- the version tag does not already exist

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
- if CI already created the tag, keep `Cargo.toml` and the tag aligned when retrying or cutting the next release
