# Builds the Windows installer and portable zip:
#   packaging\out\scyconvert-<version>-windows-x64-setup.exe
#   packaging\out\scyconvert-<version>-windows-x64.zip
#
# Run from any directory in PowerShell. Needs the Rust toolchain and the MSVC
# build tools (scripts\setup-windows.ps1) and Inno Setup 6
# (winget install JRSoftware.InnoSetup). The payload bundles FFmpeg and
# PDFium; Office documents use a separately installed LibreOffice.
param([switch]$SkipBuild)

$ErrorActionPreference = "Stop"
$ProgressPreference = "SilentlyContinue"
$Root = Resolve-Path (Join-Path $PSScriptRoot "..\..")
$Out = Join-Path $Root "packaging\out"
$Cache = Join-Path $Root "packaging\.cache"
$Payload = Join-Path $Out "windows\scyconvert"
$Version = (Select-String -Path (Join-Path $Root "Cargo.toml") -Pattern '^version = "(.+)"').Matches[0].Groups[1].Value

# BtbN's GPL build of the FFmpeg 9.0 release branch. The shared build keeps the
# codecs in DLLs that ffmpeg.exe and ffprobe.exe share, instead of twice.
$FfmpegUrl = "https://github.com/BtbN/FFmpeg-Builds/releases/download/latest/ffmpeg-n9.0-latest-win64-gpl-shared-9.0.zip"
$PdfiumUrl = "https://github.com/bblanchon/pdfium-binaries/releases/latest/download/pdfium-win-x64.tgz"

function Check($what) {
    if ($LASTEXITCODE -ne 0) { throw "$what failed (exit $LASTEXITCODE)" }
}
function Fetch($url, $dest) {
    if (-not (Test-Path $dest)) { Invoke-WebRequest $url -OutFile $dest }
}

New-Item -ItemType Directory -Force $Out, $Cache | Out-Null

if (-not $SkipBuild) {
    Push-Location $Root
    try {
        cargo build --release --locked -p scyconvert-cli -p scyconvert-app -p scyconvert-shell; Check "cargo build"
    } finally { Pop-Location }
}

if (Test-Path $Payload) { Remove-Item -Recurse -Force $Payload }
New-Item -ItemType Directory -Force (Join-Path $Payload "licenses") | Out-Null
$Release = Join-Path $Root "target\release"
$Binaries = "scyconvert.exe", "scyconvert-app.exe", "scyconvert_shell.dll"
Copy-Item ($Binaries | ForEach-Object { Join-Path $Release $_ }) $Payload

$FfmpegZip = Join-Path $Cache "ffmpeg-win64-gpl-shared-9.0.zip"
Fetch $FfmpegUrl $FfmpegZip
$FfmpegDir = Join-Path $Cache "ffmpeg-win64"
if (Test-Path $FfmpegDir) { Remove-Item -Recurse -Force $FfmpegDir }
Expand-Archive $FfmpegZip $FfmpegDir
$FfmpegBin = Get-ChildItem $FfmpegDir -Recurse -Filter ffmpeg.exe | Select-Object -First 1
Copy-Item (Join-Path $FfmpegBin.DirectoryName "ffmpeg.exe"), (Join-Path $FfmpegBin.DirectoryName "ffprobe.exe") $Payload
Copy-Item (Join-Path $FfmpegBin.DirectoryName "*.dll") $Payload
Copy-Item (Join-Path $FfmpegBin.Directory.Parent.FullName "LICENSE.txt") (Join-Path $Payload "licenses\FFmpeg.txt")

$PdfiumTgz = Join-Path $Cache "pdfium-win-x64.tgz"
Fetch $PdfiumUrl $PdfiumTgz
$PdfiumDir = Join-Path $Cache "pdfium-win-x64"
if (Test-Path $PdfiumDir) { Remove-Item -Recurse -Force $PdfiumDir }
New-Item -ItemType Directory $PdfiumDir | Out-Null
# System32 tar: a GNU tar earlier on PATH (Git Bash) reads "C:" as a remote host.
& "$env:SystemRoot\System32\tar.exe" -xzf $PdfiumTgz -C $PdfiumDir; Check "tar"
Copy-Item (Join-Path $PdfiumDir "bin\pdfium.dll") $Payload
Copy-Item (Join-Path $PdfiumDir "LICENSE") (Join-Path $Payload "licenses\PDFium.txt")

Copy-Item (Join-Path $Root "LICENSE") (Join-Path $Payload "LICENSE.txt")
$Icon = Join-Path $Root "packaging\icon.ico"
if (Test-Path $Icon) { Copy-Item $Icon (Join-Path $Payload "scyconvert.ico") }

$Zip = Join-Path $Out "scyconvert-$Version-windows-x64.zip"
if (Test-Path $Zip) { Remove-Item $Zip }
Compress-Archive (Join-Path $Payload "*") $Zip

$Iscc = Get-Command iscc -ErrorAction SilentlyContinue
if (-not $Iscc) {
    $Iscc = @(
        "${env:ProgramFiles(x86)}\Inno Setup 6\ISCC.exe",
        "$env:ProgramFiles\Inno Setup 6\ISCC.exe",
        "$env:LOCALAPPDATA\Programs\Inno Setup 6\ISCC.exe"
    ) | Where-Object { Test-Path $_ } | Select-Object -First 1
}
if (-not $Iscc) { throw "Inno Setup 6 not found. Install it with: winget install JRSoftware.InnoSetup" }
& $Iscc /Q "/DAppVersion=$Version" "/DPayload=$Payload" "/DOutputDir=$Out" (Join-Path $PSScriptRoot "scyconvert.iss"); Check "ISCC"

Write-Host "built $Zip"
Write-Host "built $(Join-Path $Out "scyconvert-$Version-windows-x64-setup.exe")"
