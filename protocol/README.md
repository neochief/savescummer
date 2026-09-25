# Protocol examples

Shared example messages of the host protocol (PLAN-HOST.md, PROTOCOL). Both
sides test against them: the Rust types in `crates/ipc` parse every file here
(`cargo test -p savescummer-ipc`), and the UI's tests should do the
same. Change them together with the protocol.

- `request-*.json` — client → host requests.
- `response-*.json` — host → client answers (`re` is the request id).
- `event-*.json` — host → watcher events.
