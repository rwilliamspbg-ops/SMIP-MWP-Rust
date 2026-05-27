#!/usr/bin/env python3
"""Read routing miss sweep CSV, report the best route count, and optionally write an SVG plot.

Usage: plot_routing_miss_sweep.py <input.csv> [output.svg]
"""
import csv
import sys
from pathlib import Path


def load_rows(path: str):
    rows = []
    with open(path, newline="", encoding="utf-8") as handle:
        reader = csv.DictReader(handle)
        for row in reader:
            try:
                rows.append(
                    {
                        "route_count": int(row["route_count"]),
                        "mean_time_ns": float(row["mean_time_ns"]),
                        "throughput_melem_s": float(row.get("throughput_melem_s") or 0.0),
                    }
                )
            except (KeyError, TypeError, ValueError):
                continue
    rows.sort(key=lambda row: row["route_count"])
    return rows


def svg_polyline(points, width, height, padding):
    xs = [p[0] for p in points]
    ys = [p[1] for p in points]
    min_x, max_x = min(xs), max(xs)
    min_y, max_y = min(ys), max(ys)
    x_span = max(max_x - min_x, 1)
    y_span = max(max_y - min_y, 1e-9)

    def map_point(x, y):
        px = padding + (x - min_x) / x_span * (width - 2 * padding)
        py = height - padding - (y - min_y) / y_span * (height - 2 * padding)
        return px, py

    mapped = [map_point(x, y) for x, y in points]
    polyline = " ".join(f"{x:.1f},{y:.1f}" for x, y in mapped)
    return polyline, mapped, (min_x, max_x, min_y, max_y)


def write_svg(rows, output_path):
    width, height, padding = 960, 540, 60
    points = [(row["route_count"], row["mean_time_ns"]) for row in rows]
    polyline, mapped, bounds = svg_polyline(points, width, height, padding)
    min_x, max_x, min_y, max_y = bounds
    best = min(rows, key=lambda row: row["mean_time_ns"])

    lines = [
        f'<svg xmlns="http://www.w3.org/2000/svg" width="{width}" height="{height}" viewBox="0 0 {width} {height}">',
        '<rect width="100%" height="100%" fill="#0f172a"/>',
        f'<text x="{padding}" y="28" fill="#e2e8f0" font-family="monospace" font-size="20">Routing miss sweep</text>',
        f'<text x="{padding}" y="48" fill="#94a3b8" font-family="monospace" font-size="12">best route_count={best["route_count"]} mean_time_ns={best["mean_time_ns"]:.3f}</text>',
        f'<line x1="{padding}" y1="{height-padding}" x2="{width-padding}" y2="{height-padding}" stroke="#334155"/>',
        f'<line x1="{padding}" y1="{padding}" x2="{padding}" y2="{height-padding}" stroke="#334155"/>',
        f'<polyline fill="none" stroke="#38bdf8" stroke-width="3" points="{polyline}"/>',
    ]

    for (row, (x, y)) in zip(rows, mapped):
        lines.append(f'<circle cx="{x:.1f}" cy="{y:.1f}" r="4" fill="#f59e0b"/>')
        lines.append(
            f'<text x="{x + 8:.1f}" y="{y - 8:.1f}" fill="#cbd5e1" font-family="monospace" font-size="11">{row["route_count"]}</text>'
        )

    lines.append('</svg>')
    Path(output_path).write_text("\n".join(lines), encoding="utf-8")


def main():
    if len(sys.argv) < 2:
        print("Usage: plot_routing_miss_sweep.py <input.csv> [output.svg]")
        sys.exit(2)

    input_path = sys.argv[1]
    output_path = sys.argv[2] if len(sys.argv) > 2 else None
    rows = load_rows(input_path)
    if not rows:
        raise SystemExit("no rows found")

    best = min(rows, key=lambda row: row["mean_time_ns"])
    print(
        f"best route_count={best['route_count']} mean_time_ns={best['mean_time_ns']:.3f} throughput_melem_s={best['throughput_melem_s']:.4f}"
    )

    if output_path:
        write_svg(rows, output_path)
        print(f"wrote SVG to {output_path}")


if __name__ == "__main__":
    main()