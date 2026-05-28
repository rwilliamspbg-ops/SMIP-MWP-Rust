# Crypto Overhead Analysis

Status: **DRAFT / NOT FINAL**

## Target

Worst-case crypto mode should stay within:

- <15% increase over baseline p99 latency

## Experimental Design

- Baseline run: crypto disabled/minimized hot path
- Worst-case run: Hybrid KEX encrypt/decrypt on every packet header
- Keep all non-crypto settings fixed (payload, pinning, packet count, core isolation)

## Delta Formula

Given baseline p99 $L_b$ and worst-case p99 $L_w$:

$$
\Delta_{crypto}\% = \frac{L_w - L_b}{L_b} \times 100
$$

Pass condition:

$$
\Delta_{crypto}\% < 15
$$

## Current State

- Automated crypto-overhead pair run is not yet wired as a dedicated benchmark target.
- Do not claim final crypto-overhead compliance until this file is updated with measured pairs and artifacts.
