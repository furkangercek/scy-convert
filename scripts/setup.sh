#!/usr/bin/env bash
# One-shot dev setup: system packages, Rust toolchain, PDFium.
set -euo pipefail
cd "$(dirname "$0")/.."

case "$(uname -s)" in
  Darwin) bash scripts/setup-macos.sh ;;
  *) echo "On Windows run scripts/setup-windows.ps1 in PowerShell." >&2; exit 1 ;;
esac

command -v rustup >/dev/null || curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --default-toolchain none
export PATH="$HOME/.cargo/bin:$PATH"
rustup show active-toolchain >/dev/null  # installs the version in rust-toolchain.toml

bash scripts/fetch-pdfium.sh
echo "Done. Try: cargo run -p scyconvert-cli -- formats"
