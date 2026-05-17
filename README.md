# LSENext

LSENext is a modern Windows 11-oriented Link Shell Extension successor focused on fast Explorer context-menu creation of symbolic links and directory junctions.

## v0.0.1 Scope

- Pick one or more link sources from Explorer.
- Drop file/directory symbolic links into a target directory.
- Drop directory junctions for directory sources.
- Preserve picked state per user at `%LOCALAPPDATA%\LSENext\state.json`.
- Build x64 and arm64 Windows artifacts from GitHub Actions.

## Build

```powershell
cargo test --workspace
.\scripts\package.ps1 -Architecture x64
.\scripts\package.ps1 -Architecture arm64
```

Artifacts are written to `artifacts\`.
