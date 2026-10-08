# scyconvert

Local file conversion for Windows and macOS: a Rust engine, a GPUI desktop app and a CLI. AGPL-3.0-only, based on convt. Conversions run on the user's machine; do not add telemetry or network calls to the app or CLI without being asked.

## Layout

- `crates/scyconvert-core`: format table, `Engine` trait, `Registry` that routes multi-hop conversions (BFS, max 3 hops). No native dependencies.
- `crates/scyconvert-engines`: FFmpeg (subprocess), `image`, resvg, PDFium (dynamically loaded), LibreOffice (headless subprocess), HEIC via libheif or `sips` on macOS. `default_registry()` registers whatever runs on this machine.
- `crates/scyconvert-cli`: the `scyconvert` binary. `scyconvert <files or folders> --to <fmt>` with options, `--preset`, `--json` progress, `-r` and `-j`; `formats`, `targets <file> [--menu]`, `engines`, `presets`, `pack`.
- `crates/scyconvert-app`: the desktop app (`scyconvert-app`), built on GPUI through `gpui-kit`. Excluded from `default-members`; build it with `-p scyconvert-app`.
- `crates/scyconvert-shell`: the Windows Explorer menu COM handler (`scyconvert_shell.dll`), registered per user by the installer (`integrations/windows`).
- `integrations/macos`: the Finder Sync extension, bundled by `package.sh`. Ad-hoc builds only get its "Open in scyconvert…" fallback.
- `packaging/windows`: `package.ps1` builds the Inno Setup installer and portable zip.
- `packaging/macos`: `package.sh <arch>` builds the ad-hoc signed `.app` and `.dmg`.
- `.github/workflows/release.yml`: builds both platforms on a `v*` tag and publishes a GitHub release.

## Commands

```sh
cargo build                                   # core, engines, CLI, shell
cargo build -p scyconvert-app                 # desktop app
cargo run -p scyconvert-cli -- photo.png --to webp
cargo test --workspace
cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings
cargo clippy -p scyconvert-app --all-targets -- -D warnings
```

`scripts/setup-windows.ps1` and `scripts/setup.sh` (macOS) install the toolchain and fetch PDFium into `vendor/pdfium`.

## Conventions

- Adding a conversion means declaring `steps()` on an engine; the registry finds chains. Give a native or hardware path a higher `priority()` than the fallback.
- Engines that shell out find their tool via `SCYCONVERT_<TOOL>`, then next to the executable, then `PATH`. LibreOffice is also found in Program Files (Windows) and `/Applications` (macOS).
- Multi-hop routes may not turn a still format into video or audio; only a direct step can.
- Tests that need FFmpeg, PDFium or LibreOffice belong in engine crates and must skip cleanly when the tool is missing.
- `scyconvert-core` stays free of native and platform dependencies.
- Every client gets conversions from `scyconvert_engines::default_registry()`. Do not add a conversion path that bypasses it.
- Release Windows builds of `scyconvert-app` use the GUI subsystem: no console window.

## Verification

- Prove behavior with a real run and inspected output, not only a passing build.
- A routing or format change affects every client. Check `scyconvert targets` for the formats around the change.
- macOS code cannot be compiled on Windows. Say so instead of claiming it works; the release workflow is the macOS build check.
- On some Windows machines the `windows_acl` and `packs` tests in `scyconvert-engines` fail because `%TEMP%` grants write access to other principals. That is the environment, not the change.
