Title: Add real-hardware smoke tests and `make real-bench` target

Description:
Create reproducible smoke tests and benchmarks that run against a real NIC using AF_XDP. These tests should validate NIC setup (queues, drivers), hugepages, UMEM mapping, and a basic forwarding datapath.

Work items:
- Add `make real-bench` target that runs a small benchmark harness with configurable iface/queue/frames.
- Provide a Docker-friendly script or documentation to configure hugepages and udev rules.
- Add a smoke test that performs a short forward (send/receive) and verifies end-to-end semantics.
- Document expected environment variables and required hardware (e.g., Intel/AMD NIC models).

Labels: enhancement, test, hardware
Assignees: @maintainers
