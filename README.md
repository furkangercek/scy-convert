# scyconvert

Local file conversion for Windows and macOS. Drop files on the app or use the CLI, pick a format, and the result lands next to the original. Nothing is uploaded and the app makes no network calls on its own.

- Around 40 formats through native engines: FFmpeg, PDFium, LibreOffice, image and resvg
- A desktop app built with [GPUI](https://www.gpui.rs) and the `scyconvert` CLI in the same install
- Multi-step routes of up to three hops when no engine converts directly

## Install

Download the latest build from [Releases](https://github.com/furkangercek/scy-convert/releases):

- **Windows**: `scyconvert-<version>-windows-x64-setup.exe` installs for the current user (no admin rights), adds **Convert with scyconvert** to the right-click menu for files (under **Show more options** on Windows 11) and can add the CLI to `PATH`. The `.zip` is the same files, portable, without the menu.
- **macOS**: `scyconvert-<version>-macos-arm64.dmg` (Apple silicon) or `-x86_64.dmg` (Intel). The app is not notarized, so the first launch needs right-click > **Open**, or `xattr -dr com.apple.quarantine /Applications/scyconvert.app`.

FFmpeg and PDFium are bundled. Word, Excel and PowerPoint files need [LibreOffice](https://www.libreoffice.org/download/) installed; scyconvert finds it in Program Files or `/Applications`.

## Usage

```bash
scyconvert clip.mov --to mp4
scyconvert photos/ --to webp -r
scyconvert lease.pdf --to png --pages 1-3
scyconvert engines              # backends available on this machine
scyconvert targets photo.png    # formats a file can reach
scyconvert formats
```

`scyconvert --help` lists the quality, size, page, DPI, video and job options. Output goes next to each input unless you pass `--out-dir`.

## Build from source

Windows (PowerShell):

```powershell
.\scripts\setup-windows.ps1        # VS Build Tools, Rust, FFmpeg, LibreOffice, PDFium
cargo run -p scyconvert-app
.\packaging\windows\package.ps1    # installer and zip in packaging\out (needs Inno Setup)
```

macOS (needs Xcode and Homebrew):

```bash
bash scripts/setup.sh
cargo run -p scyconvert-app
bash packaging/macos/package.sh    # dmg in packaging/out
```

Pushing a `v*` tag runs `.github/workflows/release.yml`, which builds both platforms and publishes a release.

## License

AGPL-3.0-only, see [LICENSE](LICENSE). Bundled FFmpeg is GPL and PDFium is BSD-3-Clause; their licenses ship in the install's `licenses` folder.
