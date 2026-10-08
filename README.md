# scyconvert

My file converter for Windows and macOS. Right-click a file, pick a format, done: the result lands next to the original. Everything runs on your own machine. No uploads, no accounts, no trial, no telemetry, no update pings.

## What it converts

| Kind | Formats | Engine |
| --- | --- | --- |
| Images | JPEG, PNG, WebP, AVIF, GIF, TIFF, BMP, ICO, TGA, PPM, QOI, OpenEXR, SVG, HEIC (macOS) | image, resvg, sips |
| Video | MP4, MOV, WebM, MKV, AVI, and video to GIF | FFmpeg |
| Audio | MP3, WAV, FLAC, AAC, M4A, OGG, Opus, or the audio track of a video | FFmpeg |
| PDF | Pages to PNG or JPEG | PDFium |
| Documents | DOCX, DOC, ODT, RTF, TXT, HTML, PPTX, PPT, ODP, XLSX, XLS, ODS, CSV, and any of them to PDF | LibreOffice |

When there's no direct path, scyconvert chains up to three conversions to get there.

## Get it

Grab the latest build from [Releases](https://github.com/furkangercek/scy-convert/releases).

### Windows

Run `scyconvert-<version>-windows-x64-setup.exe`. It installs for your user only, so there's no admin prompt. The installer can:

- add **Convert with scyconvert** to the right-click menu for files (on Windows 11 it's under **Show more options**)
- put the `scyconvert` command on your `PATH`
- create a desktop shortcut

Prefer no installer? The `.zip` has the same files and runs from any folder, just without the right-click menu.

### macOS

Open `scyconvert-<version>-macos-arm64.dmg` (Apple silicon) or `-x86_64.dmg` (Intel) and drag the app to Applications.

The app isn't notarized, so macOS blocks the first launch. Right-click the app and choose **Open**, or run:

```bash
xattr -dr com.apple.quarantine /Applications/scyconvert.app
```

For the Finder right-click entry, turn on scyconvert under **System Settings > General > Login Items & Extensions**. Right-click > **Services** > **Convert with scyconvert** works too.

### Office documents

FFmpeg and PDFium come bundled. Word, Excel and PowerPoint need [LibreOffice](https://www.libreoffice.org/download/), which is too big to bundle. Install it normally and scyconvert finds it on its own.

## Command line

```bash
scyconvert clip.mov --to mp4
scyconvert photos/ --to webp -r          # a whole folder, recursively
scyconvert scan.pdf --to png --pages 1-3
scyconvert song.wav --to mp3 --out-dir ~/Music

scyconvert targets photo.png             # what can this file become?
scyconvert engines                       # which engines work on this machine
scyconvert formats                       # every supported format
```

Run `scyconvert --help` for quality, size, DPI, video and parallelism options.

## Building it yourself

Windows, in PowerShell:

```powershell
.\scripts\setup-windows.ps1          # VS Build Tools, Rust, FFmpeg, LibreOffice, PDFium
cargo run -p scyconvert-app          # run the app
.\packaging\windows\package.ps1      # installer + zip in packaging\out (needs Inno Setup)
```

macOS, with Xcode and Homebrew installed:

```bash
bash scripts/setup.sh
cargo run -p scyconvert-app
bash packaging/macos/package.sh      # dmg in packaging/out
```

Pushing a `v*` tag builds the Windows and macOS downloads on GitHub Actions and publishes them as a release.

## Credits and license

scyconvert is a modified version of [convt](https://github.com/opencoredev/convt) by opencoredev, with the website, accounts, licensing, update checks and cloud service removed and its own installers added. It is licensed under the [GNU AGPL v3](LICENSE). The bundled FFmpeg is GPL and PDFium is BSD-3-Clause; their licenses are in the `licenses` folder of every install.
