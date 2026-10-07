#!/usr/bin/env python3
"""Generates the 48x48 PNG icons of the bundled apps (wasm-apps/<app>/icon.png).

No dependencies: draws into a pixel array and writes a PNG with zlib. The files
are committed; rerun this only to change the artwork:

    python3 tools/gen-app-icons.py
"""
import math
import struct
import zlib
import os

S = 48


def blank():
    return [[(0, 0, 0, 0)] * S for _ in range(S)]


def rrect(img, x0, y0, x1, y1, r, col):
    for y in range(y0, y1):
        for x in range(x0, x1):
            dx = max(x0 + r - x, 0, x - (x1 - 1 - r))
            dy = max(y0 + r - y, 0, y - (y1 - 1 - r))
            if dx * dx + dy * dy <= r * r:
                img[y][x] = col


def disc(img, cx, cy, rad, col):
    for y in range(S):
        for x in range(S):
            if (x - cx) ** 2 + (y - cy) ** 2 <= rad * rad:
                img[y][x] = col


def ring(img, cx, cy, ro, ri, col):
    for y in range(S):
        for x in range(S):
            d = (x - cx) ** 2 + (y - cy) ** 2
            if ri * ri <= d <= ro * ro:
                img[y][x] = col


def line(img, x0, y0, x1, y1, col, w=1):
    n = max(abs(x1 - x0), abs(y1 - y0)) * 2 + 1
    for i in range(n + 1):
        t = i / n
        x = round(x0 + (x1 - x0) * t)
        y = round(y0 + (y1 - y0) * t)
        for dy in range(-(w // 2), w - w // 2):
            for dx in range(-(w // 2), w - w // 2):
                if 0 <= x + dx < S and 0 <= y + dy < S:
                    img[y + dy][x + dx] = col


def rect(img, x0, y0, x1, y1, col):
    for y in range(y0, y1):
        for x in range(x0, x1):
            img[y][x] = col


def write(name, img):
    raw = b"".join(b"\x00" + bytes(c for px in row for c in px) for row in img)

    def chunk(t, d):
        return struct.pack(">I", len(d)) + t + d + struct.pack(">I", zlib.crc32(t + d) & 0xFFFFFFFF)

    png = b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack(">IIBBBBB", S, S, 8, 6, 0, 0, 0))
    png += chunk(b"IDAT", zlib.compress(raw, 9)) + chunk(b"IEND", b"")
    root = os.path.join(os.path.dirname(__file__), "..", "wasm-apps", name)
    os.makedirs(root, exist_ok=True)
    with open(os.path.join(root, "icon.png"), "wb") as f:
        f.write(png)


W = (255, 255, 255, 255)

# hello: teal tile, smiley
img = blank()
rrect(img, 2, 2, 46, 46, 10, (0x1F, 0xB5, 0xA6, 255))
disc(img, 24, 24, 15, (0xFF, 0xD8, 0x4D, 255))
disc(img, 18, 20, 2, (0x22, 0x22, 0x22, 255))
disc(img, 30, 20, 2, (0x22, 0x22, 0x22, 255))
for a in range(20, 161, 4):
    x = round(24 + 9 * math.cos(math.radians(a)))
    y = round(26 + 8 * math.sin(math.radians(a)))
    img[y][x] = (0x22, 0x22, 0x22, 255)
write("hello", img)

# clock: dark tile, face, hands
img = blank()
rrect(img, 2, 2, 46, 46, 10, (0x1E, 0x27, 0x4A, 255))
ring(img, 24, 24, 17, 15, W)
for h in range(12):
    a = math.radians(h * 30)
    line(img, round(24 + 13 * math.sin(a)), round(24 - 13 * math.cos(a)),
         round(24 + 15 * math.sin(a)), round(24 - 15 * math.cos(a)), (0x9A, 0xA6, 0xBD, 255))
line(img, 24, 24, 24, 13, W, 2)
line(img, 24, 24, 32, 28, (0x39, 0xD3, 0x53, 255), 2)
disc(img, 24, 24, 2, (0xE5, 0x4B, 0x4B, 255))
write("clock", img)

# notes: yellow page with lines
img = blank()
rrect(img, 2, 2, 46, 46, 10, (0xF2, 0xC1, 0x4E, 255))
rrect(img, 11, 8, 37, 41, 3, (0xFF, 0xFB, 0xE8, 255))
for y in (15, 21, 27, 33):
    rect(img, 15, y, 33, y + 2, (0xB8, 0x95, 0x3A, 255))
rect(img, 15, 15, 24, 17, (0x6A, 0x4F, 0x12, 255))
write("notes", img)

# paint: purple tile with color dots and a brush stroke
img = blank()
rrect(img, 2, 2, 46, 46, 10, (0x65, 0x4F, 0xF0, 255))
for (cx, cy, col) in ((15, 16, (0xE5, 0x4B, 0x4B, 255)), (26, 13, (0xFF, 0xD8, 0x4D, 255)),
                      (35, 20, (0x39, 0xD3, 0x53, 255)), (13, 28, (0x39, 0xA4, 0xFF, 255))):
    disc(img, cx, cy, 5, col)
line(img, 18, 38, 36, 30, W, 4)
disc(img, 38, 29, 3, W)
write("paint", img)

# snake: green tile, body segments and an apple
img = blank()
rrect(img, 2, 2, 46, 46, 10, (0x16, 0x2B, 0x1E, 255))
for (x, y) in ((10, 32), (18, 32), (26, 32), (26, 24), (26, 16), (34, 16)):
    rrect(img, x, y, x + 8, y + 8, 2, (0x2E, 0xA0, 0x43, 255))
rrect(img, 34, 16, 42, 24, 2, (0x39, 0xD3, 0x53, 255))
disc(img, 12, 14, 4, (0xE5, 0x4B, 0x4B, 255))
write("snake", img)

# plasma: magenta/cyan gradient
img = blank()
rrect(img, 2, 2, 46, 46, 10, (0, 0, 0, 255))
for y in range(S):
    for x in range(S):
        if img[y][x][3]:
            v = math.sin(x / 5.0) + math.sin(y / 4.0) + math.sin((x + y) / 6.0)
            r = int(128 + 127 * math.sin(v * 1.3))
            g = int(128 + 127 * math.sin(v * 1.3 + 2.1))
            b = int(128 + 127 * math.sin(v * 1.3 + 4.2))
            img[y][x] = (r, g, b, 255)
write("plasma", img)
print("icons written")
