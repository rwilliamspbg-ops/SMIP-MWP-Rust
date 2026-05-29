Title: Add Prometheus metrics export and structured tracing

Description:
Improve observability by exporting Prometheus metrics and adding structured tracing (e.g., `tracing` crate). This will aid diagnostics during chaos/stress runs.

Work items:
- Add a `metrics` module with Prometheus exporter and `/metrics` HTTP endpoint.
- Integrate `tracing` crate across `datapath`, `cli`, and `bridge` with configurable subscriber.
- Add examples to `cli` to run with metrics enabled.

Labels: enhancement, observability
