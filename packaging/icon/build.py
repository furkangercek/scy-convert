"""Renders the app icons from the SVG masters in this folder.

    python packaging/icon/build.py

Needs Pillow and a built CLI (cargo build --release -p scyconvert-cli), which
renders the SVGs. Writes icon.ico (Windows), icon.icns (macOS) and the
Finder menu template images; commit them with the masters.

- icon-win.svg / icon-small-win.svg: edge to edge, as Windows draws icons
- icon.svg / icon-small.svg: on Apple's 824 px tile with padding
- the -small masters (arrows only) are used at 32 px and below
- menu.svg: a black glyph that macOS tints for light and dark menus
"""

import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

from PIL import Image

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[1]
SMALL = 32


def cli() -> Path:
    exe = ROOT / "target" / "release" / ("scyconvert.exe" if sys.platform == "win32" else "scyconvert")
    if not exe.exists():
        sys.exit(f"build the CLI first: cargo build --release -p scyconvert-cli ({exe} missing)")
    return exe


def render(svg: str, tmp: Path) -> Image.Image:
    shutil.copy(HERE / svg, tmp / svg)
    # The masters are 1024 px at 96 DPI.
    subprocess.run([cli(), tmp / svg, "--to", "png", "--out-dir", tmp], check=True, capture_output=True)
    return Image.open(tmp / svg.replace(".svg", ".png")).convert("RGBA")


def sized(full: Image.Image, small: Image.Image, size: int) -> Image.Image:
    return (small if size <= SMALL else full).resize((size, size), Image.LANCZOS)


def main() -> None:
    with tempfile.TemporaryDirectory() as t:
        tmp = Path(t)
        win, win_small = render("icon-win.svg", tmp), render("icon-small-win.svg", tmp)
        mac, mac_small = render("icon.svg", tmp), render("icon-small.svg", tmp)
        menu = render("menu.svg", tmp)

    ico_sizes = [256, 128, 64, 48, 32, 24, 16]
    ico = [sized(win, win_small, s) for s in ico_sizes]
    ico[0].save(HERE / "icon.ico", sizes=[(s, s) for s in ico_sizes], append_images=ico[1:])

    icns_sizes = [1024, 512, 256, 128, 64, 32, 16]
    icns = [sized(mac, mac_small, s) for s in icns_sizes]
    icns[0].save(HERE / "icon.icns", append_images=icns[1:])

    menu.resize((16, 16), Image.LANCZOS).save(HERE / "MenuIconTemplate.png")
    menu.resize((32, 32), Image.LANCZOS).save(HERE / "MenuIconTemplate@2x.png")
    print("wrote icon.ico, icon.icns, MenuIconTemplate.png, MenuIconTemplate@2x.png")


if __name__ == "__main__":
    main()
