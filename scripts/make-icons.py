#!/usr/bin/env python3
"""Draw the newtype mark and a macOS tray template. No third-party packages."""

import struct
import zlib
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
OUT = ROOT / "src-tauri" / "icons"
SOURCE = ROOT / "app-icon.png"


def png(width: int, height: int, rgba: bytes) -> bytes:
    if len(rgba) != width * height * 4:
        raise SystemExit("pixel buffer has the wrong size")

    def chunk(tag: bytes, data: bytes) -> bytes:
        return (
            struct.pack(">I", len(data))
            + tag
            + data
            + struct.pack(">I", zlib.crc32(tag + data) & 0xFFFFFFFF)
        )

    raw = b"".join(b"\x00" + rgba[y * width * 4 : (y + 1) * width * 4] for y in range(height))
    return (
        b"\x89PNG\r\n\x1a\n"
        + chunk(b"IHDR", struct.pack(">IIBBBBB", width, height, 8, 6, 0, 0, 0))
        + chunk(b"IDAT", zlib.compress(raw, 9))
        + chunk(b"IEND", b"")
    )


def inside_round_rect(x: int, y: int, width: int, height: int, radius: int) -> bool:
    if x < 0 or y < 0 or x >= width or y >= height:
        return False
    left = radius
    right = width - radius - 1
    top = radius
    bottom = height - radius - 1
    if left <= x <= right or top <= y <= bottom:
        return True
    corners = ((left, top), (right, top), (left, bottom), (right, bottom))
    for cx, cy in corners:
        if (x - cx) ** 2 + (y - cy) ** 2 <= radius**2:
            return True
    return False


def put(buf: bytearray, width: int, x: int, y: int, color: tuple[int, int, int, int]) -> None:
    if x < 0 or y < 0:
        return
    index = (y * width + x) * 4
    if index < 0 or index + 4 > len(buf):
        return
    buf[index : index + 4] = bytes(color)


def bar(buf: bytearray, width: int, height: int, x: int, y: int, w: int, h: int, color, radius: int) -> None:
    for py in range(y, y + h):
        for px in range(x, x + w):
            if inside_round_rect(px - x, py - y, w, h, radius):
                put(buf, width, px, py, color)


def app_icon(size: int) -> bytes:
    ink = (23, 32, 42, 255)
    mint = (216, 245, 232, 255)
    buf = bytearray(size * size * 4)
    radius = int(size * 0.22)
    for y in range(size):
        for x in range(size):
            if inside_round_rect(x, y, size, size, radius):
                put(buf, size, x, y, ink)
    heights = (0.28, 0.52, 0.40, 0.22)
    bar_w = int(size * 0.075)
    gap = int(size * 0.045)
    total = 4 * bar_w + 3 * gap
    start = (size - total) // 2
    for index, fraction in enumerate(heights):
        h = int(size * fraction)
        x = start + index * (bar_w + gap)
        y = (size - h) // 2
        bar(buf, size, size, x, y, bar_w, h, mint, max(2, bar_w // 3))
    return png(size, size, bytes(buf))


def tray_template(size: int) -> bytes:
    black = (0, 0, 0, 255)
    buf = bytearray(size * size * 4)
    heights = (10, 20, 14, 8)
    bar_w = 4
    gap = 3
    total = 4 * bar_w + 3 * gap
    start = (size - total) // 2
    for index, h in enumerate(heights):
        x = start + index * (bar_w + gap)
        y = (size - h) // 2
        bar(buf, size, size, x, y, bar_w, h, black, 1)
    return png(size, size, bytes(buf))


def main() -> None:
    OUT.mkdir(parents=True, exist_ok=True)
    SOURCE.write_bytes(app_icon(1024))
    (OUT / "tray-template.png").write_bytes(tray_template(32))
    print(SOURCE)
    print(OUT / "tray-template.png")


if __name__ == "__main__":
    main()
