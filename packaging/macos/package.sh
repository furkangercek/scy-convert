#!/usr/bin/env bash
# Builds scyconvert.app and scyconvert-<version>-macos-<arch>.dmg.
#
#   packaging/macos/package.sh [arm64|x86_64]   (default: this Mac's architecture)
#
# Needs Xcode (GPUI compiles Metal shaders) and rustup. The app bundles the
# CLI, a static FFmpeg/ffprobe and PDFium. Office documents use a separately
# installed LibreOffice from /Applications.
#
# The app is ad-hoc signed, not notarized: on first launch macOS asks to
# confirm it (right-click > Open, or System Settings > Privacy & Security).
set -euo pipefail

arch=${1:-$(uname -m)}
root=$(cd "$(dirname "$0")/../.." && pwd)
here="$root/packaging/macos"
out="$root/packaging/out/macos-$arch"
cache="$root/packaging/.cache"
version=$(sed -n 's/^version = "\(.*\)"/\1/p' "$root/Cargo.toml" | head -1)
min_macos=13.0

# FFmpeg: Martin Riedl's static release builds (https://ffmpeg.martin-riedl.de).
case "$arch" in
  arm64) triple=aarch64-apple-darwin ffmpeg_build=arm64/1789931890_9.0.2 pdfium=mac-arm64 ;;
  x86_64) triple=x86_64-apple-darwin ffmpeg_build=amd64/1789931006_9.0.2 pdfium=mac-x64 ;;
  *) echo "unknown architecture: $arch (use arm64 or x86_64)" >&2; exit 2 ;;
esac

fetch() { # url dest
  [ -f "$2" ] && return
  curl -fsSL --retry 3 "$1" -o "$2.part"
  mv "$2.part" "$2"
}

mkdir -p "$out" "$cache"
export MACOSX_DEPLOYMENT_TARGET=$min_macos
rustup target add "$triple" >/dev/null
(cd "$root" && cargo build --release --locked --target "$triple" -p scyconvert-cli -p scyconvert-app)
bin="$root/target/$triple/release"

app="$out/scyconvert.app"
contents="$app/Contents"
rm -rf "$app"
mkdir -p "$contents/MacOS" "$contents/Frameworks" "$contents/Resources/licenses"

cp "$bin/scyconvert-app" "$bin/scyconvert" "$contents/MacOS/"

for tool in ffmpeg ffprobe; do
  zip="$cache/$tool-${ffmpeg_build//\//-}.zip"
  url="https://ffmpeg.martin-riedl.de/download/macos/$ffmpeg_build/$tool.zip"
  fetch "$url" "$zip"
  want=$(curl -fsSL --retry 3 "$url.sha256" | awk '{print $1}')
  got=$(shasum -a 256 "$zip" | awk '{print $1}')
  if [ "$want" != "$got" ]; then
    rm -f "$zip"
    echo "$tool.zip checksum mismatch (want $want, got $got)" >&2
    exit 1
  fi
  unpacked="$cache/$tool-unpacked"
  rm -rf "$unpacked" && unzip -q "$zip" -d "$unpacked"
  find "$unpacked" -type f -name "$tool" -exec cp {} "$contents/MacOS/$tool" \;
  chmod +x "$contents/MacOS/$tool"
done

tgz="$cache/pdfium-$pdfium.tgz"
fetch "https://github.com/bblanchon/pdfium-binaries/releases/latest/download/pdfium-$pdfium.tgz" "$tgz"
rm -rf "$cache/pdfium-$pdfium" && mkdir -p "$cache/pdfium-$pdfium"
tar -xzf "$tgz" -C "$cache/pdfium-$pdfium"
cp "$cache/pdfium-$pdfium/lib/libpdfium.dylib" "$contents/Frameworks/"

cp "$root/LICENSE" "$contents/Resources/licenses/scyconvert.txt"
cp "$cache/pdfium-$pdfium/LICENSE" "$contents/Resources/licenses/PDFium.txt"
echo "FFmpeg 9.0.2 static build by Martin Riedl, GPL. Source: https://ffmpeg.org/releases/ffmpeg-9.0.2.tar.xz" \
  > "$contents/Resources/licenses/FFmpeg.txt"

sed -e "s/@VERSION@/$version/g" -e "s/@MIN_MACOS@/$min_macos/g" "$here/Info.plist" > "$contents/Info.plist"
printf 'APPL????' > "$contents/PkgInfo"
if [ -f "$root/packaging/icon.icns" ]; then
  cp "$root/packaging/icon.icns" "$contents/Resources/scyconvert.icns"
fi

# Ad-hoc signature, inside out. Apple Silicon refuses unsigned code.
codesign --force --sign - --timestamp=none "$contents/Frameworks/libpdfium.dylib"
for name in ffmpeg ffprobe scyconvert; do
  codesign --force --sign - --timestamp=none "$contents/MacOS/$name"
done
codesign --force --sign - --timestamp=none "$app"
codesign --verify --strict "$app"

dmg="$root/packaging/out/scyconvert-$version-macos-$arch.dmg"
stage="$out/dmg"
rm -rf "$stage" "$dmg" && mkdir -p "$stage"
cp -R "$app" "$stage/"
ln -s /Applications "$stage/Applications"
hdiutil create -quiet -volname "scyconvert" -srcfolder "$stage" -format UDZO -fs HFS+ "$dmg"
rm -rf "$stage"
echo "built $dmg"
