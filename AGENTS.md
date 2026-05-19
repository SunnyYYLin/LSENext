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

## Build targets

- Windows x64: `x86_64-pc-windows-msvc`
- Windows arm64: `aarch64-pc-windows-msvc`

## Product scope

- Explorer context-menu commands for picking link sources and creating symbolic links or junctions.
- Persist picked sources in `%LOCALAPPDATA%\LSENext\state.json`.
- Keep the helper, shell DLL, and packaging outputs consistent across architectures.
