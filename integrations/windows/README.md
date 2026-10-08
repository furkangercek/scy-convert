# Windows Explorer menu

The installer (`packaging/windows/scyconvert.iss`) adds **Convert with scyconvert** to Explorer's right-click menu for files when its Explorer menu task is checked. It registers `scyconvert_shell.dll` (the Rust `scyconvert-shell` COM handler) as a per-user COM server and points a classic `HKCU\Software\Classes\*\shell\scyconvert` verb at it through `ExplorerCommandHandler`. No signature or administrator rights are needed. On Windows 11 that entry is under **Show more options**.

## Windows 11 compact menu

The compact menu only lists handlers from a signed package. `packaging/windows/menu.ps1` builds the sparse package in `packaging/windows/sparse` (no files of its own; the installed folder is its external location), signs it with a self-signed `CN=scyconvert` certificate and registers it for the current user. The first run creates the certificate in `CurrentUser\My` and asks for administrator rights once to add it to `LocalMachine\TrustedPeople`. It needs the Windows SDK (`makeappx.exe`, `signtool.exe`). Restart Explorer afterwards.

```powershell
powershell -ExecutionPolicy Bypass -File packaging\windows\menu.ps1          # register
powershell -ExecutionPolicy Bypass -File packaging\windows\menu.ps1 -Remove  # unregister
```

The package's verb also appears under **Show more options**, so the script sets `LegacyDisable` on the installer's classic verb while the package is registered, and `-Remove` clears it. Run the script again after reinstalling to a different folder. The certificate is for this machine only; a public release needs a real code-signing certificate whose subject matches the manifest's publisher.

`GetState` answers immediately, even when Explorer says it must not be slow: the classic menu treats `E_PENDING` as hidden. The probe takes about 50 ms once per extension and is then cached.

The DLL asks the installed `scyconvert.exe targets <file> --menu` for targets. It keeps the first file's order and offers only targets shared by every selected file. Unsupported selections and folders have no menu. Probes run without a console, time out after two seconds, and cache each extension for 30 seconds. Installing or removing document support therefore refreshes the menu without restarting Explorer.

Choosing a target launches `scyconvert-app.exe open --show-progress --to <format> -- <files...>`. The existing Activity window displays progress and the output goes next to the input. The Windows-only flag leaves Linux and Finder launches unchanged. The app and its conversion subprocesses do not open console windows.

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
