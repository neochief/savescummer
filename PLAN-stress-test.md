# Stress tests for game monitoring and storage

**Status:** test-plan stub; no stress harness or results yet.

## Goal

Measure how the real host behaves with many save files, large save files, and a long checkpoint history on Windows, macOS, and Linux. Run the same workloads before and after a performance change so the result shows both the improvement and any new cost. Keep correctness checks alongside the timings: a fast host that misses a game or blocks requests is a failure.

## Test shape

- Extend the existing `tests/e2e/tests/scale.rs` approach: use a disposable `World`, a real host and CLI, and one or more custom games backed by the fake-game executable. Generate all save files, checkpoint folders, and database rows under the test world's temporary directories. Never use an installed game, the user's data folder, or a real checkpoint store.
- Seed history as the current scale test does: make a real template Save, then create valid checkpoint folders, signatures, and history rows. Database-only rows do not exercise the full scan's checkpoint walk.
- Build two independent tests: one stresses many large files and the other stresses very many small files. Each includes custom-game history, but they never share a fixture or run concurrently. Give each a small smoke mode for normal test runs and an opt-in full mode.
- Cap the **peak disk space used by either test at 50 GB decimal**, including live saves, checkpoints, temporary copies, database files, and filesystem overhead. Estimate before seeding, check actual allocation during the run, and stop before the cap. Record file count, logical bytes, allocated bytes, and filesystem type. Clean up the temporary world and fake-game processes even on failure.

## The two full workloads

Both have 100 custom games. The small-file and large-file fixtures are separate so their costs can be measured independently. The values below are starting points, not performance targets; the 50 GB peak cap takes precedence.

| Test | Library shape | Nominal live + checkpoint payload | Main pressure |
| --- | --- | ---: | --- |
| **Many large files** | 95 light games with 100 checkpoints of one 64 KiB file each; 5 heavy games with 16 × 64 MiB files each and 6 checkpoints per game | About **35.6 GiB** (38.2 GB) | 80 large live files, 480 large checkpoint files, real Save/Load throughput and staging space. |
| **Very many small files** | 90 light games with 1,000 checkpoints of one 64 KiB file each; 10 heavy games with 10,000 × 4 KiB files each and 10 checkpoints per game | About **9.7 GiB** (10.4 GB), before substantial file metadata | 100,000 small live files, one million small checkpoint files, about 90,100 checkpoint folders, repeated save-folder walks and full-scan metadata work. |

Run the tests **sequentially** and delete the first fixture before creating the second. Leave headroom under 50 GB for metadata and for the peak of one Save or Load; if the next step could cross the cap, report that the fixture did not fit rather than filling the disk. Check available file/inode capacity as well as bytes where the filesystem exposes it. Generate independent, non-sparse file contents and report both logical and allocated size so filesystem cloning or compression cannot silently turn a byte-throughput result into a metadata-only result.

For each test, measure three phases: host idle with the library loaded, a heavy custom game running through multiple two-second refreshes, and a forced full scan. Close the fake game before timing one real Save and Load of a heavy game, check their contents, and repeat with the same fixture before and after a proposed fix.

## Measurements and comparison

- On each OS, run the same fixture and host build in a quiet period, repeat it several times, and report the median plus the slowest run. Save machine, OS, filesystem, build profile, workload parameters, and revision alongside raw results. Compare before and after **on the same OS and machine**; do not treat cross-OS timing differences as a regression.
- Capture host CPU time and peak memory; disk bytes and operations where the OS exposes them; wall time for startup, game detection, Save, Load, and a full scan; and CLI response latency while background work runs. Separate fixture-seeding time from host measurements. Sample idle, running-game, and full-scan phases independently.
- Assert that starts/exits, running state, checkpoint counts, Save/Load contents, and history remain correct. Check that requests and monitoring continue during a slow scan. Keep performance numbers as reports rather than brittle CI thresholds until each platform has a reliable baseline.
- Record a before/after table for each proposed fix with the workload, raw runs, median, worst case, correctness result, and observed trade-off. Profile any case that shows a large regression before changing code.

## Platform runs

Run the real OS adapter on Windows, macOS, and Linux, not the scripted demo process source. Use local temporary storage first; add an optional slow or removable drive run only where available. On Linux, include X11/XWayland and a native Wayland session for focus behavior when those environments are available. Keep platform-specific measurement code small and report metrics unavailable on a platform as unavailable, not zero.

## Next implementation step

Add the portable fixture generator and opt-in stress cases beside `tests/e2e/tests/scale.rs`, then collect a baseline on all three OSes before optimizing the monitor or file walks.
