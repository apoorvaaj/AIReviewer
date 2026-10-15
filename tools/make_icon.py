"""Generate the app icon (a question mark in a speech bubble) with the standard
library only. Output: src-tauri/icons/{32x32.png,128x128.png,icon.png,icon.ico}
Usage: python tools/make_icon.py
"""

import pathlib
import struct
import zlib

OUT = pathlib.Path(__file__).resolve().parent.parent / "src-tauri" / "icons"


def render(size):
    bg = (35, 82, 124)
    fg = (255, 255, 255)
    px = []
    s = size / 64.0
    for y in range(size):
        row = []
        for x in range(size):
            u, v = x / s, y / s
            # rounded square background
            r = 12
            dx = max(r - u, 0, u - (63 - r))
            dy = max(r - v, 0, v - (63 - r))
            inside = dx * dx + dy * dy <= r * r
            if not inside:
                row.append((0, 0, 0, 0))
                continue
            colour = bg
            # question mark: ring arc (top), stem, dot
            cx, cy = 32, 24
            d = ((u - cx) ** 2 + (v - cy) ** 2) ** 0.5
            arc = 8 <= d <= 14 and not (v > cy and u < cx)
            stem = 28 <= u <= 36 and 30 <= v <= 42
            dot = ((u - 32) ** 2 + (v - 50) ** 2) ** 0.5 <= 4.5
            if arc or stem or dot:
                colour = fg
            row.append(colour + (255,))
        px.append(row)
    return px


def png(size):
    rows = render(size)
    raw = b"".join(b"\x00" + b"".join(struct.pack("BBBB", *p) for p in row) for row in rows)

    def chunk(tag, data):
        return struct.pack(">I", len(data)) + tag + data + struct.pack(">I", zlib.crc32(tag + data) & 0xFFFFFFFF)

    return (b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack(">IIBBBBB", size, size, 8, 6, 0, 0, 0))
            + chunk(b"IDAT", zlib.compress(raw, 9)) + chunk(b"IEND", b""))


def ico(sizes):
    images = [png(s) for s in sizes]
    header = struct.pack("<HHH", 0, 1, len(images))
    offset = 6 + 16 * len(images)
    entries = b""
    for s, data in zip(sizes, images):
        entries += struct.pack("<BBBBHHII", s % 256, s % 256, 0, 0, 1, 32, len(data), offset)
        offset += len(data)
    return header + entries + b"".join(images)


def main():
    OUT.mkdir(parents=True, exist_ok=True)
    (OUT / "32x32.png").write_bytes(png(32))
    (OUT / "128x128.png").write_bytes(png(128))
    (OUT / "icon.png").write_bytes(png(256))
    (OUT / "icon.ico").write_bytes(ico([16, 32, 48, 256]))
    print("icons written to", OUT)


if __name__ == "__main__":
    main()
