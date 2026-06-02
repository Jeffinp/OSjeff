#!/usr/bin/env python3
"""Convert assets/osjeff-icon.png into raw RGBA byte blobs the kernel embeds
via include_bytes! (no runtime PNG decoder). Re-run after changing the icon."""
from pathlib import Path
from PIL import Image

ROOT = Path(__file__).resolve().parent.parent
SRC = ROOT / "assets" / "osjeff-icon.png"
SIZES = (128, 40)  # boot mark, dock icon

src = Image.open(SRC).convert("RGBA")
for size in SIZES:
    im = src.resize((size, size), Image.LANCZOS)
    out = ROOT / "assets" / f"osjeff-icon-{size}.raw"
    out.write_bytes(im.tobytes())  # tightly packed RGBA, row-major
    print(f"wrote {out.relative_to(ROOT)} ({size}x{size}, {out.stat().st_size} bytes)")
