# Release Procedure

Publishing is triggered by pushing a version tag. The `release` workflow on `windows-latest` builds the native binary, verifies the package, attaches the npm tarball to the GitHub Release, and publishes `gifshot-win` to npm.

## Prerequisites

- `package.json` and `native/Cargo.toml` versions match.
- `native/Cargo.lock` is committed and reviewed.
- Repository secret `NPM_TOKEN` is an npm **Automation** token (Settings → Secrets and variables → Actions → Repository secrets).
- CI on `main` is green, including `cargo clippy ... -- -D warnings`.
- Manual matrix in `TEST_PLAN.md` has been run on Windows 10 22H2 and current Windows 11 when the change affects capture, clipboard, or tray behavior.

## Cut a release

1. Update `CHANGELOG.md` and bump versions in `package.json` + `native/Cargo.toml` together. Commit and push `main`.
2. Create the GitHub Release / tag `vX.Y.Z` targeting that `main` tip (tag name must be `v` + package version). Curated notes can be supplied with `gh release create`; if the Release does not exist yet, the workflow creates one.
3. The `release` workflow:
   - checks the tag against both version files;
   - runs fmt / test / clippy (`-D warnings`) / source audit;
   - builds with `cargo ... --locked`, stages `vendor/win32-x64/gifshot.exe`, and verifies the package;
   - uploads `gifshot-win-X.Y.Z.tgz` and its SHA-256 to the GitHub Release;
   - runs `npm publish --access public`.
4. Confirm the Release assets and `npm view gifshot-win version`.

Do not publish from a developer workstation. The npm package must contain the CI-built `gifshot.exe`.

## Dependency lock

Before creating a public release tag, generate `native/Cargo.lock` with the pinned root Rust toolchain on Windows, review it, and commit it. Release builds use `cargo ... --locked`; do not hand-author or copy a lockfile from an unrelated environment.

## Rebuild a failed tag

If the workflow fails after the tag exists, fix `main`, delete the GitHub Release and remote tag, then recreate `vX.Y.Z` on the new tip. npm does not allow republishing the same version; a failed publish can reuse `1.0.0`, but a successful publish requires a version bump.
