# Releasing

`clux` remote bootstrap downloads `clux-server` from GitHub Releases. A release is not just distribution metadata; it is part of the runtime contract for `clux --remote`.

## Release Checklist

1. Ensure `cargo fmt --check` and `cargo test -q` pass locally.
2. Merge to `main`.
3. Wait for GitHub Actions to:
   - auto-bump the patch version in `Cargo.toml` for normal merges
   - create the annotated tag `vX.Y.Z` if it does not already exist
   - publish the GitHub Release assets for that tag
4. Verify the GitHub Release contains:
   - `clux-server-vX.Y.Z-x86_64-unknown-linux-gnu.tar.gz`
   - `clux-server-vX.Y.Z-aarch64-unknown-linux-gnu.tar.gz`
   - `SHA256SUMS`

## Automatic Tagging And Release

On pushes to `main`, the default path is:

1. `Auto Bump Version` checks whether the merge already changed `Cargo.toml`'s package version.
2. If the version did not change, it increments the patch version and pushes a bot-authored commit to `main`.
3. The `CI` workflow checks `Cargo.toml` and computes `v<version>`.
4. If that tag does not exist yet, CI creates and pushes it.
5. CI publishes the GitHub Release assets for that version.

If the merged change already updated `Cargo.toml`'s version, the auto-bump workflow does nothing and CI releases exactly the version from the merged commit. That is the path to use for intentional major or minor releases.

- CI builds the Linux release artifacts in a target matrix and uploads them as workflow artifacts.
- If the tag already exists and the GitHub Release already exists, CI skips the automatic release path.
- If the tag already exists but the GitHub Release is missing, CI republishes the release without changing the tag.

This avoids relying on a second workflow triggered by the tag push itself.

## Versioning Policy

- Normal merges to `main` should not edit `Cargo.toml`; automation will bump the patch version.
- Intentional major or minor releases should update `Cargo.toml` in the merged PR, and automation will preserve that explicit version.
- The automation requires GitHub Actions to have permission to push to `main`. If branch protection is enabled, allow `github-actions[bot]` to bypass or satisfy that rule.

## Manual Release Fallback

Manual tag pushes still work:

```bash
git tag -a vX.Y.Z -m "vX.Y.Z"
git push origin vX.Y.Z
```

That path uses the standalone `Release` workflow directly, and that workflow performs its own target-matrix builds because there is no prior CI artifact set to reuse.

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
