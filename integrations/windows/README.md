# Windows Explorer menu

The installer (`packaging/windows/scyconvert.iss`) adds the scyconvert entries to Explorer's right-click menu for files when its Explorer menu task is checked. It registers `scyconvert_shell.dll` (the Rust `scyconvert-shell` COM handler) as a per-user COM server and points classic `HKCU\Software\Classes\*\shell\scyconvert*` verbs at it through `ExplorerCommandHandler`. No signature or administrator rights are needed. On Windows 11 that entry is under **Show more options**.

## Windows 11 compact menu

The compact menu only lists handlers from a signed package. `packaging/windows/menu.ps1` builds the sparse package in `packaging/windows/sparse` (no files of its own; the installed folder is its external location), signs it with a self-signed `CN=scyconvert` certificate and registers it for the current user. The first run creates the certificate in `CurrentUser\My` and asks for administrator rights once to add it to `LocalMachine\TrustedPeople`. It needs the Windows SDK (`makeappx.exe`, `signtool.exe`). Restart Explorer afterwards.

```powershell
powershell -ExecutionPolicy Bypass -File packaging\windows\menu.ps1          # register
powershell -ExecutionPolicy Bypass -File packaging\windows\menu.ps1 -Remove  # unregister
```

The package's verbs also appear under **Show more options**, so the script sets `LegacyDisable` on the installer's classic verbs while the package is registered, and `-Remove` clears it. Run the script again after reinstalling to a different folder. The certificate is for this machine only; a public release needs a real code-signing certificate whose subject matches the manifest's publisher.

`GetState` answers immediately, even when Explorer says it must not be slow: the classic menu treats `E_PENDING` as hidden. The probe takes about 50 ms once per extension and is then cached.

Explorer shows three entries, each its own COM class and verb, because the Windows 11 menu shows only one level of submenu: **Convert with scyconvert** (target formats, with greyed headings such as Video and Audio only), **Compress with scyconvert** (video to about 50%, 33% or 15% of its size, audio to 192, 128 or 64 kbit/s; a result that isn't smaller is dropped) and **Adjust audio with scyconvert** (mono, stereo, even loudness, extract or remove audio). An entry with nothing to offer is hidden. The DLL asks the installed `scyconvert.exe targets <file> --menu` what to offer: tab-separated `menu <id> <title>` lines, each followed by `heading`, `format`, `action` and `separator` lines. For several files it keeps the first file's order and offers only what every file can do; output it can't read hides the menu. Probes run without a console, time out after two seconds, and cache each extension for 30 seconds, so installing or removing document support refreshes the menu without restarting Explorer.

Choosing an entry leaves a request for the running app, with `to` or `action`, and starts `scyconvert-app.exe --minimized` if it isn't running, so it starts in the tray. No window opens: the output goes next to the input (an action that keeps the format adds a suffix, `clip-compressed.mp4`), and a notification says when it's done; clicking it shows the file. A conversion that can't run opens Quick convert instead. The app and its conversion subprocesses do not open console windows.

## Verify without touching the desktop

`context-menu` prints the classic menu exactly as the shell builds it from the registry, submenus included:

```powershell
cargo build --release -p scyconvert-shell --example context-menu
.\target\release\examples\context-menu.exe C:\test\sample.png
```

`explorer-probe --registered` activates the packaged COM server, as the compact menu does. Its DLL path argument is then ignored.

Build the public COM consumer and run it against the installed DLL and real test files:

```powershell
cargo build --release --target x86_64-pc-windows-msvc -p scyconvert-shell --example explorer-probe
.\target\x86_64-pc-windows-msvc\release\examples\explorer-probe.exe `
  "$env:LOCALAPPDATA\Programs\scyconvert\scyconvert_shell.dll" C:\test\sample.png
Get-AppxPackage -Name Scyconvert.Desktop
Get-ItemProperty 'HKCU:\Software\Classes\*\shell\scyconvert'
```

The consumer loads `DllGetClassObject`, constructs `IExplorerCommand`, enumerates before and after `GetState`, and compares the submenu with the installed CLI. Give it multiple files to check target intersection. After uninstalling, the package and both registry keys must be absent. A real Explorer right-click remains a separate visual check; never drive a desktop while its owner is active or a game is focused.
