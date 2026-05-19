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
```

Packaging is done in GitHub Actions:

- `.github/workflows/alpha.yml` for alpha pre-releases
- `.github/workflows/release.yml` for manual pre-releases

Artifacts are published from the workflow run and written to `artifacts\` during the job.
