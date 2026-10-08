# Windows Explorer menu

**Status: not shipped.** `crates/scyconvert-shell` builds a COM handler for **Convert with scyconvert**, but the installer does not register it yet. Windows 11's compact menu needs a signed sparse MSIX identity; Windows 10 and **Show more options** can use the same handler through classic HKCU registry verbs, which need no signature or administrator rights.

The DLL asks the installed `scyconvert.exe targets <file> --menu` for targets. It keeps the first file's order and offers only targets shared by every selected file. Unsupported selections and folders have no menu. Probes run without a console, time out after two seconds, and cache each extension for 30 seconds. Installing or removing document support therefore refreshes the menu without restarting Explorer.

Choosing a target launches `scyconvert-app.exe open --show-progress --to <format> -- <files...>`. The existing Activity window displays progress and the output goes next to the input. The Windows-only flag leaves Linux and Finder launches unchanged. The app and its conversion subprocesses do not open console windows.

## Verify without touching the desktop

Build the public COM consumer and run it against the installed DLL and real test files:

```powershell
cargo build --release --target x86_64-pc-windows-msvc -p scyconvert-shell --example explorer-probe
.\target\x86_64-pc-windows-msvc\release\examples\explorer-probe.exe `
  "$env:LOCALAPPDATA\Programs\scyconvert\scyconvert_shell.dll" C:\test\sample.png
Get-AppxPackage -Name Scyconvert.Desktop
Get-ItemProperty 'HKCU:\Software\Classes\*\shell\scyconvert'
```

The consumer loads `DllGetClassObject`, constructs `IExplorerCommand`, enumerates before and after `GetState`, and compares the submenu with the installed CLI. Give it multiple files to check target intersection. After uninstalling, the package and both registry keys must be absent. A real Explorer right-click remains a separate visual check; never drive a desktop while its owner is active or a game is focused.
