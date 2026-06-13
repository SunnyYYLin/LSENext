# AGENTS.md

This repository is LSENext, a Windows 11-oriented Link Shell Extension successor.

## Project layout

- `crates/lsenext-core`: shared state and link-creation logic
- `crates/lsenext-helper`: small CLI used by Explorer and packaging
- `crates/lsenext-shell`: Explorer shell extension DLL
- `scripts/package.ps1`: builds and packages x64 or arm64 artifacts
- `.github/workflows`: CI and release automation

## Working rules

- Read `REQUIREMENTS.md`, `README.md`, and the relevant crate before changing behavior.
- Keep edits narrow and aligned with the existing Rust + Windows patterns in the repo.
- Do not revert unrelated user changes.
- Prefer ASCII unless a file already uses another encoding or language intentionally.

## Command rules

- Use `cargo test --workspace` for validation unless a narrower test is enough.
- For packaging, use GitHub Actions workflows instead of local packaging.

## Release workflow

### Alpha pre-release (automatic)

- Push to `main` triggers `.github/workflows/alpha.yml`.
- Alpha tag is computed as `v<version>-alpha.<next>` where `<version>` is the `$version` variable in `alpha.yml`.
- After publishing a stable release, bump `$version` in `alpha.yml` to the next minor/patch version.

### Stable release

1. Bump `version` in workspace `Cargo.toml` and all version strings (`lsenext-shell/src/lib.rs`, `lsenext-helper/src/main.rs`).
2. Update `$version` in `.github/workflows/alpha.yml` to the next development version.
3. Commit and push to `main`.
4. Create and push a tag: `git tag vX.Y.Z && git push origin vX.Y.Z`.
5. `.github/workflows/release.yml` triggers automatically: builds, deletes all pre-releases, and creates the stable release.

### Version bump checklist

- `Cargo.toml` → `workspace.package.version`
- `.github/workflows/alpha.yml` → `$version` (next dev version)
- `.github/workflows/release.yml` → uses `${{ github.ref_name }}` dynamically (no change needed)
- `crates/lsenext-shell/src/lib.rs` → `LSENextVersion()` string
- `crates/lsenext-helper/src/main.rs` → `show_about()` string

## Build targets

- Windows x64: `x86_64-pc-windows-msvc`
- Windows arm64: `aarch64-pc-windows-msvc`

## Product scope

- Explorer context-menu commands for picking link sources and creating symbolic links or junctions.
- Persist picked sources in `%LOCALAPPDATA%\LSENext\state.json`.
- Keep the helper, shell DLL, and packaging outputs consistent across architectures.
