# Release Procedure

1. Run the full manual matrix in `TEST_PLAN.md` on Windows 10 22H2 and current Windows 11.
2. Confirm CI is green.
3. Update `CHANGELOG.md` and versions in `package.json` + `native/Cargo.toml` together.
4. Tag `vX.Y.Z`.
5. Download the npm tarball produced by the `release-artifact` workflow.
6. Inspect it with `npm pack --dry-run` / `tar -tf` and smoke-test installation on a clean Windows VM.
7. Publish explicitly with `npm publish <tarball>` from an authenticated release workstation or a separately configured trusted-publishing workflow.

The repository deliberately does **not** auto-publish to npm from every tag. Publishing is an explicit release action after the real-desktop capture matrix has passed.

## Dependency lock

Before creating a public release tag, generate `native/Cargo.lock` with the pinned root Rust toolchain on the Windows build runner, review it, and commit it. Release builds must then use `cargo ... --locked`; do not hand-author or copy a lockfile from an unrelated environment.
