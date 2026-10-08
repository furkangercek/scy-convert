#!/usr/bin/env bash
# Repository libraries are explicit test inputs, never release discovery paths.
set -euo pipefail
root=$(cd "$(dirname "$0")/.." && pwd)
export SCYCONVERT_LICENSE_STORE=file
export SCYCONVERT_PDFIUM_DIR=${SCYCONVERT_PDFIUM_DIR:-$root/vendor/pdfium/lib}

# scyconvert only accepts the patched libheif the Linux bundle ships, so HEIC and
# AVIF cases need it. Use the built bundle unless the caller chose a libheif.
bundle=$root/packaging/out/scyconvert
if [ -z "${SCYCONVERT_LIBHEIF_DIR:-}" ] && [ -e "$bundle/lib/libheif.so.1" ]; then
  export SCYCONVERT_LIBHEIF_DIR=$bundle/lib
  export SCYCONVERT_LIBHEIF_PLUGIN_DIR=$bundle/lib/libheif/plugins
  export LIBHEIF_PLUGIN_PATH=$bundle/lib/libheif/plugins
  export LD_LIBRARY_PATH=$bundle/lib${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}
  export SCYCONVERT_FFMPEG=${SCYCONVERT_FFMPEG:-$bundle/ffmpeg}
  export SCYCONVERT_FFPROBE=${SCYCONVERT_FFPROBE:-$bundle/ffprobe}
fi
if [ -z "${SCYCONVERT_LIBHEIF_DIR:-}" ] && [ "${SCYCONVERT_MATRIX_WITHOUT_HEIF:-}" != 1 ]; then
  echo "No patched libheif: HEIC and AVIF cases would be skipped." >&2
  echo "Run 'bun run bundle:linux' first, set SCYCONVERT_LIBHEIF_DIR, or set SCYCONVERT_MATRIX_WITHOUT_HEIF=1 to skip them on purpose." >&2
  exit 1
fi

cd "$root"
exec cargo test -p scyconvert-engines "$@"
