# Adds "Convert with scyconvert" to the Windows 11 compact right-click menu.
#
#   powershell -ExecutionPolicy Bypass -File packaging\windows\menu.ps1
#   powershell -ExecutionPolicy Bypass -File packaging\windows\menu.ps1 -Remove
#
# The compact menu only lists handlers from a signed package. This builds the
# sparse package in packaging\windows\sparse, signs it with a self-signed
# certificate (CN=scyconvert, created once in CurrentUser\My), and registers
# it for the current user over the installed files. Windows only installs
# packages from a trusted publisher, so the first run asks for administrator
# rights once to add the certificate to LocalMachine\TrustedPeople.
# Restart Explorer afterwards to see the entry. The classic menu
# ("Show more options") comes from the installer and needs none of this.
param(
    [string]$InstallDir = (Join-Path $env:LOCALAPPDATA "Programs\scyconvert"),
    [switch]$Remove
)

$ErrorActionPreference = "Stop"
$Root = Resolve-Path (Join-Path $PSScriptRoot "..\..")
$PackageName = "Scyconvert.Desktop"
$Subject = "CN=scyconvert"
# The installer's classic verbs. The package's verbs also show in the
# classic menu, so LegacyDisable hides these while the package is registered.
$ClassicVerbs = "scyconvert", "scyconvert.compress", "scyconvert.audio" |
    ForEach-Object { "HKCU:\Software\Classes\*\shell\$_" }

function Check($what) {
    if ($LASTEXITCODE -ne 0) { throw "$what failed (exit $LASTEXITCODE)" }
}

if ($Remove) {
    Get-AppxPackage -Name $PackageName | Remove-AppxPackage
    foreach ($verb in $ClassicVerbs | Where-Object { Test-Path -LiteralPath $_ }) {
        Remove-ItemProperty -LiteralPath $verb -Name LegacyDisable -ErrorAction SilentlyContinue
    }
    Write-Host "Removed the compact menu package. The certificate stays in CurrentUser\My and LocalMachine\TrustedPeople."
    exit 0
}

foreach ($file in "scyconvert-app.exe", "scyconvert.exe", "scyconvert_shell.dll") {
    if (-not (Test-Path (Join-Path $InstallDir $file))) { throw "$file is missing from $InstallDir; install scyconvert first" }
}

$Kit = Get-ChildItem "${env:ProgramFiles(x86)}\Windows Kits\10\bin\*\x64\makeappx.exe" -ErrorAction SilentlyContinue |
    Sort-Object FullName -Descending | Select-Object -First 1
if (-not $Kit) { throw "makeappx.exe not found; install the Windows SDK (winget install Microsoft.WindowsSDK.10.0.26100)" }
$MakeAppx = $Kit.FullName
$SignTool = Join-Path $Kit.DirectoryName "signtool.exe"

$Cert = Get-ChildItem Cert:\CurrentUser\My |
    Where-Object { $_.Subject -eq $Subject -and $_.HasPrivateKey -and $_.NotAfter -gt (Get-Date) } |
    Sort-Object NotAfter -Descending | Select-Object -First 1
if (-not $Cert) {
    # Code signing EKU, not a CA.
    $Cert = New-SelfSignedCertificate -Type Custom -Subject $Subject -KeyUsage DigitalSignature `
        -FriendlyName "scyconvert package signing" -CertStoreLocation Cert:\CurrentUser\My `
        -NotAfter (Get-Date).AddYears(10) `
        -TextExtension @("2.5.29.37={text}1.3.6.1.5.5.7.3.3", "2.5.29.19={text}")
    Write-Host "Created signing certificate $($Cert.Thumbprint)"
}

$Work = Join-Path $Root "packaging\out\sparse"
if (Test-Path $Work) { Remove-Item -Recurse -Force $Work }
$Stage = Join-Path $Work "package"
New-Item -ItemType Directory -Force $Stage | Out-Null

$Trusted = Get-ChildItem Cert:\LocalMachine\TrustedPeople | Where-Object { $_.Thumbprint -eq $Cert.Thumbprint }
if (-not $Trusted) {
    $Cer = Join-Path $Work "scyconvert.cer"
    Export-Certificate -Cert $Cert -FilePath $Cer | Out-Null
    Write-Host "Asking for administrator rights to trust the signing certificate..."
    $Import = "Import-Certificate -FilePath '$Cer' -CertStoreLocation Cert:\LocalMachine\TrustedPeople | Out-Null"
    $p = Start-Process powershell -Verb RunAs -Wait -PassThru -WindowStyle Hidden `
        -ArgumentList "-NoProfile", "-Command", $Import
    if ($p.ExitCode -ne 0) { throw "trusting the certificate failed (exit $($p.ExitCode))" }
}

# Four-part version from the workspace, e.g. 0.1.1 -> 0.1.1.0.
$Version = (Select-String -Path (Join-Path $Root "Cargo.toml") -Pattern '^version = "(.+)"').Matches[0].Groups[1].Value
$Version = ($Version -replace '[-+].*$', '') + ".0"
$Manifest = Get-Content -Raw (Join-Path $PSScriptRoot "sparse\AppxManifest.xml")
$Manifest = $Manifest.Replace('$PUBLISHER$', $Subject).Replace('$VERSION$', $Version)
[IO.File]::WriteAllText((Join-Path $Stage "AppxManifest.xml"), $Manifest)

Copy-Item (Join-Path $PSScriptRoot "sparse\logo.png") $Stage

$Msix = Join-Path $Work "scyconvert-menu.msix"
& $MakeAppx pack /o /nv /d $Stage /p $Msix | Out-Null; Check "makeappx"
& $SignTool sign /q /fd SHA256 /sha1 $Cert.Thumbprint /s My $Msix; Check "signtool"

# Only now that the new package is built and signed does the old one go.
Get-AppxPackage -Name $PackageName | Remove-AppxPackage
Add-AppxPackage -Path $Msix -ExternalLocation $InstallDir
$Package = Get-AppxPackage -Name $PackageName
if (-not $Package) { throw "the package did not register" }
foreach ($verb in $ClassicVerbs | Where-Object { Test-Path -LiteralPath $_ }) {
    New-ItemProperty -LiteralPath $verb -Name LegacyDisable -Value "" -Force | Out-Null
}
Write-Host "Registered $($Package.PackageFullName). Restart Explorer to see the menu:"
Write-Host "  Stop-Process -Name explorer"
