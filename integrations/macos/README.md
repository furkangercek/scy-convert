# macOS integration

A Finder Sync extension that adds **Convert with scyconvert** to Finder's right-click menu.

**Status: not shipped.** `packaging/macos/package.sh` does not bundle the extension. It only works with a Developer ID signature that grants the App Group; an ad-hoc build can't. The Services menu entry below works without it.

## The menu

- `FinderSync/FinderSync.swift` builds the menu from the target list the app publishes. The extension is sandboxed and runs from its own executable, so it never probes tools: `crates/scyconvert-app/src/macos.rs` writes `targets.json` (extension to format, format to targets, with categories) into the App Group container `<TEAMID>.io.github.furkangercek.scyconvert` at launch and after every engine change. Both bundles name the group in the `ScyconvertAppGroup` Info.plist key. The menu shows the targets every selected file shares, grouped under a header per category ("Video", "Audio only" for a video's audio, and so on), with **More options…** at the bottom. With no readable list (the app has never run) it shows only **Open in scyconvert…**.
- Picking a format converts in place with no window; **More options…** opens Quick convert. AppKit drops `NSWorkspace.OpenConfiguration.arguments` for sandboxed callers, so the extension can't pass `open --to <format> -- <files>`. Instead it writes the request (`{version, to, files, created}`) to `requests/<uuid>.json` in the App Group container and opens `scyconvert://finder` with this bundle's app by path (another app may claim the `scyconvert` scheme). The app takes every fresh request (under two minutes old, absolute paths, deleted before use) on that link, or at launch in place of the main window. The link only wakes the app: a web page can open it, but only processes in the App Group can write requests, so links still never convert without a click.
- When it can't write the request (no shared container, as in an ad-hoc build), the extension opens the files with scyconvert instead, which shows Quick convert. Finder's "Open With" → scyconvert arrives the same way.
- The Services menu has **Convert with scyconvert** as a fallback when the extension is off (`NSServices` in `packaging/macos/Info.plist`, handled in `macos.rs`). It opens Quick convert.
- The extension is sandboxed and ships inside `scyconvert.app/Contents/PlugIns/`. The GPUI app registers the `scyconvert://` scheme for these requests.

## Building

The old release tooling compiled `FinderSync/FinderSync.swift` with `swiftc -application-extension -framework FinderSync`, filled `FinderSync/Info.plist`, placed it in `scyconvert.app/Contents/PlugIns/FinderSync.appex` and signed it inside out with matching App Group entitlements. Adding that to `package.sh` needs a signing identity and team ID.
