#!/usr/bin/env python3
import argparse
import csv
import struct
import zlib
from pathlib import Path


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Generate benchmark/report_latency.png")
    parser.add_argument("--input", default="tools/bench_results/chaos_epyc_profile.csv")
    parser.add_argument("--output", default="benchmark/report_latency.png")
    return parser.parse_args()


def read_rows(path: Path) -> list[dict[str, str]]:
    with path.open("r", encoding="utf-8", newline="") as f:
        return list(csv.DictReader(f))


def make_grayscale_plot(rows: list[dict[str, str]], width: int = 800, height: int = 240) -> bytes:
    p99_values = [float(r.get("p99_ns", "0") or 0) for r in rows]
    if not p99_values:
        p99_values = [0.0]

    lo = min(p99_values)
    hi = max(p99_values)
    span = max(1.0, hi - lo)

    pixels = [[255 for _ in range(width)] for _ in range(height)]

    for x in range(width):
        src_idx = int(round((x / max(1, width - 1)) * (len(p99_values) - 1)))
        value = p99_values[src_idx]
        norm = (value - lo) / span
        y = height - 1 - int(norm * (height - 1))
        for dy in (-1, 0, 1):
            yy = y + dy
            if 0 <= yy < height:
                pixels[yy][x] = 40

    raw = b"".join(b"\x00" + bytes(row) for row in pixels)

    def chunk(tag: bytes, data: bytes) -> bytes:
        return struct.pack(">I", len(data)) + tag + data + struct.pack(">I", zlib.crc32(tag + data) & 0xFFFFFFFF)

    ihdr = struct.pack(">IIBBBBB", width, height, 8, 0, 0, 0, 0)
    idat = zlib.compress(raw, level=9)
    png = b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", ihdr) + chunk(b"IDAT", idat) + chunk(b"IEND", b"")
    return png


def main() -> int:
    args = parse_args()
    input_path = Path(args.input)
    output_path = Path(args.output)

    if not input_path.exists():
        raise SystemExit(f"missing input CSV: {input_path}")

    rows = read_rows(input_path)
    png = make_grayscale_plot(rows)

    output_path.parent.mkdir(parents=True, exist_ok=True)
    output_path.write_bytes(png)
    print(f"wrote {output_path}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
