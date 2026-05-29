Title: Wire `bridge/` control plane into datapath (dynamic route/session updates)

Description:
The bridge crate provides contract schemas and example control requests, but the runtime wiring to apply dynamic updates to the datapath is incomplete. We need a non-blocking control plane path to update routes and session state without stalling the hot path.

Work items:
- Implement a control command queue consumed by worker threads that applies route/session updates atomically.
- Ensure updates are applied safely (lockless or with short critical sections) to avoid stalls.
- Add a CLI flag to run `mohawk-node` as a control-plane daemon that accepts bridge requests.
- Expand `tools/validation/verify_bridge.sh` to exercise dynamic updates under load.

Labels: enhancement, bridge, control-plane
Assignees: @maintainers
