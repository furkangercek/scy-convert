# macOS integration

A Finder Sync extension that adds **Convert with scyconvert** to Finder's right-click menu.

`packaging/macos/package.sh` bundles and ad-hoc signs the extension. Turn it on in System Settings > General > Login Items & Extensions (Finder extensions). An ad-hoc build has no team ID, so macOS doesn't share the App Group between the app and the extension: the menu then offers only **Open in scyconvert…**, which opens Quick convert. The per-format submenu needs both bundles signed by the same Developer ID team, with the group prefixed by the team ID.

## The menu

- `FinderSync/FinderSync.swift` builds the menu from the target list the app publishes. The extension is sandboxed and runs from its own executable, so it never probes tools: `crates/scyconvert-app/src/macos.rs` writes `targets.json` (extension to format, format to targets, with categories) into the App Group container `<TEAMID>.io.github.furkangercek.scyconvert` at launch and after every engine change. Both bundles name the group in the `ScyconvertAppGroup` Info.plist key. The menu shows the targets every selected file shares, grouped under a header per category ("Video", "Audio only" for a video's audio, and so on), with **More options…** at the bottom. With no readable list (the app has never run) it shows only **Open in scyconvert…**.
- Picking a format converts in place with no window; **More options…** opens Quick convert. AppKit drops `NSWorkspace.OpenConfiguration.arguments` for sandboxed callers, so the extension can't pass `open --to <format> -- <files>`. Instead it writes the request (`{version, to, files, created}`) to `requests/<uuid>.json` in the App Group container and opens `scyconvert://finder` with this bundle's app by path (another app may claim the `scyconvert` scheme). The app takes every fresh request (under two minutes old, absolute paths, deleted before use) on that link, or at launch in place of the main window. The link only wakes the app: a web page can open it, but only processes in the App Group can write requests, so links still never convert without a click.
- When it can't write the request (no shared container, as in an ad-hoc build), the extension opens the files with scyconvert instead, which shows Quick convert. Finder's "Open With" → scyconvert arrives the same way.
- The Services menu has **Convert with scyconvert** as a fallback when the extension is off (`NSServices` in `packaging/macos/Info.plist`, handled in `macos.rs`). It opens Quick convert.
- The extension is sandboxed and ships inside `scyconvert.app/Contents/PlugIns/`. The GPUI app registers the `scyconvert://` scheme for these requests.

## Building

`packaging/macos/package.sh` compiles `FinderSync/FinderSync.swift` with `swiftc -application-extension` (no Xcode project), fills `FinderSync/Info.plist`, places it at `scyconvert.app/Contents/PlugIns/FinderSync.appex` and signs it inside out with `FinderSync.entitlements` before the app.
