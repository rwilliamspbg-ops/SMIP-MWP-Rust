#!/usr/bin/env python3
import argparse
import re
import sys


def parse_metric(pattern: str, text: str, name: str) -> float:
    m = re.search(pattern, text)
    if not m:
        raise ValueError(f"missing metric: {name}")
    return float(m.group(1))


def main() -> int:
    parser = argparse.ArgumentParser(description="Validate chaos benchmark output against thresholds")
    parser.add_argument("--input", required=True, help="Path to benchmark stdout capture")
    parser.add_argument("--min-throughput", type=float, default=100000.0)
    parser.add_argument("--max-p99-ns", type=float, default=5000.0)
    parser.add_argument("--max-p999-ns", type=float, default=10000.0)
    args = parser.parse_args()

    text = open(args.input, "r", encoding="utf-8").read()

    throughput = parse_metric(r"throughput_pkt_s=([0-9.]+)", text, "throughput_pkt_s")
    p99 = parse_metric(r"latency_ns\s+p50=\d+\s+p99=(\d+)", text, "p99")
    p999 = parse_metric(r"latency_ns\s+p50=\d+\s+p99=\d+\s+p99_9=(\d+)", text, "p99_9")

    failures = []
    if throughput < args.min_throughput:
        failures.append(
            f"throughput_pkt_s {throughput:.2f} < min {args.min_throughput:.2f}"
        )
    if p99 > args.max_p99_ns:
        failures.append(f"p99 {p99:.0f} > max {args.max_p99_ns:.0f}")
    if p999 > args.max_p999_ns:
        failures.append(f"p99_9 {p999:.0f} > max {args.max_p999_ns:.0f}")

    print(
        f"chaos-thresholds throughput_pkt_s={throughput:.2f} p99_ns={p99:.0f} p99_9_ns={p999:.0f}"
    )

    if failures:
        for failure in failures:
            print(f"FAIL: {failure}")
        return 1

    print("PASS: chaos benchmark thresholds satisfied")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
