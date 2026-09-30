# Protocol examples

Shared example messages of the host protocol (PLAN-HOST.md, PROTOCOL). Both
sides test against them: the Rust types in `crates/ipc` parse every file here
(`cargo test -p savescummer-ipc`), and the UI's tests should do the
same. Change them together with the protocol.

- `request-*.json` — client → host requests.
- `response-*.json` — host → client answers (`re` is the request id).
- `event-*.json` — host → watcher events.

Protocol version **2** ships with the host, CLI and Tauri UI together. Version 1
clients are rejected. The checkpoint format is unchanged.

- Game summaries contain availability for Save, Load, Restore, Delete, Flush,
  Configure and Retry, plus separate stable `guidance` (kind, coverage and remedy).
  Failures may carry structured target causes or access details.
- History actions describe checkpoint eligibility. Combine them with the live
  game's Restore/Delete gate; do not refetch history for busy or running changes.
- `checking` is a provisional reservation visible in game activity. It does not
  mean durable acceptance and is not an outcome clients can recover after restart.
- Retry uses the ordinary accepted operation response and outcome query. Its
  request ID is replayable; failure retains the original interrupted journal.
- `open` with `access_settings` asks the host to open its selected permissions URL.
