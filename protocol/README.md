# Local service protocol v2

The checked-in JSON schemas and fixtures in this directory are the shared contract
for Rust and C++/Qt clients. Changes must update the schemas, fixtures and
compatibility tests together. Generate the schemas from the Rust wire types with:

```text
cargo run -p savescummer-ipc --example export_schema
```

Each frame is a four-byte unsigned little-endian payload length followed by that
many UTF-8 JSON bytes. Frames must contain 1 through 8,388,608 bytes. There is no
newline terminator. Requests have `version`, `request_id` and `command`; responses
have `version`, the same `request_id`, a `host_id` and `result`. An unknown version
returns a structured `invalid_request` error. Malformed or oversized frames close
the connection. A disconnect never means the operation succeeded or was cancelled.

Use one request per connection. Ordinary queries and commands return one response.
`watch` returns the current complete state, followed by complete states when its
`revision` or `artwork_revision` changes. Intermediate progress updates may be coalesced; every delivered
state is self-contained. Revisions increase within one `host_id`; a different
`host_id` means the host restarted and clients must discard their old revision.
After reconnecting, obtain a new snapshot and query any accepted operation IDs.
The desktop also sends `check_artwork` on attachment. This returns `ok` immediately
after queueing a host background cache check, without waiting for disk or network.
`state.artwork` maps game IDs to `{steam_app_id, icon_path}`; `icon_path` is null
until a validated local image is available. Artwork is an ephemeral host projection,
not catalog or database data. Its independent revision does not invalidate Flush
confirmations. Watch clients must observe both revisions and reset both on host restart.
During graceful shutdown an existing watch receives `{"type":"shutting_down"}`
as its final result. Desktop clients close and stop reconnecting on that message;
the host independently waits for accepted work to reach a safe stopping point.

`execute` replies with `accepted` only after the pending operation is durably stored.
Repeating the same `request_id`, game and action returns the same operation ID,
including after restart. Reusing the ID for a different action is rejected. IDs
are opaque strings, and timestamps are UTC Unix milliseconds, never identifiers.

Explicit `load.target` is a saved checkpoint ID (`snapshot_id` on a Saved/Existing
backup history row). `revert.target` is a recovery checkpoint ID (`recovery_id` on
a Loaded/Reverted row). Version 1 history-ID targets are no longer accepted.
Omitting Load's target selects the eligible saved checkpoint with the greatest
`(selection_time, registration_order)`, independently of history rows. Checkpoints
carry their `original_data_dir`; the game carries the current `data_dir`. Service
state reports `available` only when the checkpoint is unchanged, accessible and
belongs to that game's current directory. Reconfiguring back to the original
directory permits restoring its unchanged checkpoints again. Directory comparison
uses filesystem resolution, preserving distinct case-sensitive paths.

History and operation `target_id` remain optional audit links to the original
history row; `snapshot_id`, `recovery_id` and operation `source_id` identify the
actual checkpoints. A missing audit link never determines restore eligibility.
The host migrates database schema 1 to 2 atomically, preserving IDs, history links
and recovery journals. Legacy manual checkpoints with no retained original-path
mapping have null `original_data_dir` and remain ineligible until a scan verifies
the same generation at the configured directory. New checkpoints always have an
original directory. Existing explicit operation targets migrate to checkpoint IDs
so accepted requests remain idempotent when retried through protocol v2.

The client must render `state.visible_history` and consult snapshot availability
for actions. `state.history` and removed snapshot generations are audit records,
not rows to render. Removed generations never regain availability. A new folder
generation receives new IDs even when its path was previously used. For Existing
backup rows, `selection_time` is a folder-modification estimate, `discovered_at` is
the observation time, and `saved_at` remains null. These meanings must not be mixed.

`state.availability[game_id]` supplies `data_available` and the core-selected
`default_snapshot_id` (null when none is eligible). This additive v2 projection
lets the desktop render Save/Load availability and timestamps without probing
save directories or recreating selection policy. Live-directory availability
changes advance the host revision even without a history event. Clients connected
to older hosts without the field must leave ordinary Save/Load controls disabled.

Operation `status` is authoritative. `pending` is busy; `completed` is committed;
`failed` is an unsuccessful operation whose live data is safe; `recovery_needed`
blocks ordinary operations; `resolved` records a recovery choice, not a successful
Load/Revert. `phase` and `bytes_copied` provide indeterminate progress information.
Errors have a stable `code` and diagnostic `message`. UI wording can use the code.

`state.settings.play_sounds` is the persisted app-wide audio preference and defaults
to true for existing databases. Send `{"type":"set_play_sounds","enabled":false}`
to mute it (or true to enable); `ok` means the setting committed. The state revision
and watch stream update after a change. This is an additive protocol v2 command.
The Windows host owns playback for Save/Load commands regardless of client lifetime;
clients must not play duplicate cues. Replayed accepted requests are silent.

Additional Windows-host entry points (additive v2):

- `execute_active` accepts `action: "save"|"load"`. The core chooses the first
  active-stack game. Replaying an accepted ID uses the original game and action.
- `explorer_targets` accepts an absolute `path`; its reply has `targets`, each
  containing `game_id` and an ordinary `action` for `execute`. An empty list means
  no menu item. Clients retain the returned checkpoint ID, never invent one from a
  path, and never substitute default Load. Execution revalidates the generation.
- `explore` accepts `game_id` and opens the validated parent directory through the
  platform adapter. It remains available during file operations and recovery.
- `set_launch_on_startup` accepts `enabled`. OS registration must succeed before
  the core commits `settings.launch_on_startup` (default false). A commit failure
  attempts registration rollback and returns an error, including rollback failure.
- `select_detected_location` accepts `game_id` and `location` matching a current
  `game.detected_locations` entry (`data_dir`, `executables`). Null chooses the sole
  detected location. Ambiguity or a stale choice is rejected; core path/busy/recovery
  validation still applies. `configure` remains available for manual overrides.

Game records publish `detected_locations` and `user_configured`, with backwards
compatible defaults for existing databases. Invalid or ambiguous discoveries have
`configuration_error` and cannot start file operations. `state.discovery_errors`
publishes source/integration diagnostics independently of a UI client. These errors
do not erase valid configurations, checkpoints or history.

Flush is two-step: request `flush_preview`, show all counts/paths, then execute
`flush` with its `confirmed_revision`. The host refreshes backup discovery before
checking this revision, so external backup changes also invalidate the confirmation
and require a new preview. Any intervening revision invalidates that confirmation.
Recovery commands use an explicit interrupted operation ID and one
of `keep_current`, `restore_before` or `retry`.

Windows endpoints include the signed-in user SID and data-directory hash. Their
DACL grants access only to that SID, and remote pipe clients are rejected. Unix
uses a mode-0600 socket inside a mode-0700 data directory. The host holds the
data-directory lock for its entire lifetime. Alternate data directories are for
isolated development/test instances; normal use has one default host per user.
