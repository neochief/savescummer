# Checks for coding tasks

- After changing Rust source, run `cargo fmt --all`, then `cargo clippy -q --workspace --all-targets --locked -- -D warnings` before finishing. Fix diagnostics caused by the task and rerun the failed check. Review formatter changes so unrelated edits are not overwritten.
- Run tests relevant to changed behavior. Use `cargo xtask check` when preparing a release or changing workspace-wide build or release behavior; it also runs tests, a build, and the catalog check, so do not run it for every small edit.
- Keep check output concise: use quiet commands when available, and report the relevant failure diagnostics rather than full build logs. Do not run `cargo clippy --fix` across the workspace without reviewing its edits.
