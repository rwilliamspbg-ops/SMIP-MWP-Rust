#!/usr/bin/env python3
import argparse
import json
import os
import re
import sys


def parse_metric(pattern: str, text: str, name: str) -> float:
    m = re.search(pattern, text)
    if not m:
        raise ValueError(f"missing metric: {name}")
    return float(m.group(1))


def load_baselines() -> dict:
    here = os.path.dirname(__file__)
    path = os.path.join(here, "sla_baselines.json")
    if not os.path.exists(path):
        return {}
    with open(path, "r", encoding="utf-8") as f:
        return json.load(f)


def main() -> int:
    baselines = load_baselines()

    parser = argparse.ArgumentParser(description="Validate chaos benchmark output against thresholds")
    parser.add_argument("--input", required=True, help="Path to benchmark stdout capture")
    parser.add_argument("--min-throughput", type=float, default=None)
    parser.add_argument("--max-p99-ns", type=float, default=None)
    parser.add_argument("--max-p999-ns", type=float, default=None)
    parser.add_argument("--enforce-sla", action="store_true", help="Also enforce service-level p99 ceiling from baselines file")
    args = parser.parse_args()

    # Fill defaults from baselines file when CLI args unspecified
    smoke = baselines.get("smoke", {})
    service = baselines.get("service_level", {})

    min_throughput = args.min_throughput if args.min_throughput is not None else smoke.get("min_throughput", 100000.0)
    max_p99_ns = args.max_p99_ns if args.max_p99_ns is not None else smoke.get("max_p99_ns", 5000.0)
    max_p999_ns = args.max_p999_ns if args.max_p999_ns is not None else smoke.get("max_p999_ns", 10000.0)
    svc_p99_ns = service.get("p99_ns")

    text = open(args.input, "r", encoding="utf-8").read()

    throughput = parse_metric(r"throughput_pkt_s=([0-9.]+)", text, "throughput_pkt_s")
    p99 = parse_metric(r"latency_ns\s+p50=\d+\s+p99=(\d+)", text, "p99")
    p999 = parse_metric(r"latency_ns\s+p50=\d+\s+p99=\d+\s+p99_9=(\d+)", text, "p99_9")

    failures = []
    if throughput < float(min_throughput):
        failures.append(f"throughput_pkt_s {throughput:.2f} < min {min_throughput:.2f}")
    if p99 > float(max_p99_ns):
        failures.append(f"p99 {p99:.0f} > max {max_p99_ns:.0f}")
    if p999 > float(max_p999_ns):
        failures.append(f"p99_9 {p999:.0f} > max {max_p999_ns:.0f}")

    print(f"chaos-thresholds throughput_pkt_s={throughput:.2f} p99_ns={p99:.0f} p99_9_ns={p999:.0f}")

    if failures:
        for failure in failures:
            print(f"FAIL: {failure}")
        return 1

    # Optionally enforce the authoritative service-level p99 ceiling from baselines
    if args.enforce_sla and svc_p99_ns is not None:
        if p99 > float(svc_p99_ns):
            print(f"FAIL: SLA p99 {p99:.0f} > service_level p99 {svc_p99_ns:.0f}")
            return 2

    print("PASS: chaos benchmark thresholds satisfied")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
