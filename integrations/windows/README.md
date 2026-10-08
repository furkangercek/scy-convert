# Windows Explorer menu

The installer (`packaging/windows/scyconvert.iss`) adds **Convert with scyconvert** to Explorer's right-click menu for files when its Explorer menu task is checked. It registers `scyconvert_shell.dll` (the Rust `scyconvert-shell` COM handler) as a per-user COM server and points a classic `HKCU\Software\Classes\*\shell\scyconvert` verb at it through `ExplorerCommandHandler`. No signature or administrator rights are needed. On Windows 11 the entry is under **Show more options**: the compact menu needs a signed sparse MSIX identity, which this build does not have.

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
