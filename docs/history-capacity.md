# History capacity measurements

Recorded on 2026-09-21 with Windows 11 Pro 10.0.26200, Intel Core i5-14600KF,
using the Rust debug test build. These are individual observations with warm
filesystem caches, not latency budgets or a guarantee for every save directory.

The fixture has two configured games and two actual small backup directories.
It adds the indicated number of visible history rows, an equal number of terminal
operation journals, and an equal number of retired checkpoint records. "Split"
distributes these between both games; "one game" concentrates them in the first.
History rows deliberately reference the surviving checkpoints. It measures audit
growth independently of filesystem discovery and copying thousands of backups.

| Added rows per record type | Distribution | Startup | First 50 rows | Next page | Small Save | Summary bytes |
| ---: | --- | ---: | ---: | ---: | ---: | ---: |
| 100 | One game | 1.52 ms | 1.39 ms | 1.38 ms | 12.44 ms | 4,338 |
| 100 | Split | 1.59 ms | 1.40 ms | 0.11 ms | 15.78 ms | 4,338 |
| 10,000 | One game | 1.78 ms | 1.46 ms | 1.41 ms | 11.97 ms | 4,339 |
| 10,000 | Split | 1.75 ms | 1.41 ms | 1.39 ms | 11.49 ms | 4,340 |
| 100,000 | One game | 1.60 ms | 1.67 ms | 1.45 ms | 11.69 ms | 4,340 |
| 100,000 | Split | 1.81 ms | 1.50 ms | 1.43 ms | 10.95 ms | 4,342 |

The final page in the 100-row split case contains fewer than 50 rows. Startup
includes core startup recovery and backup refresh, but excludes opening SQLite
and schema migration. Save includes admission, metadata transactions and real
copying of the small fixture. Summary construction ranged from 0.29 to 0.44 ms.
No process peak-memory or idle CPU measurement was taken in this run. Correctness
tests assert that boot loads no historical rows or checkpoints and only current
operations; summary payloads exclude history and terminal journals. File scanning,
Flush path collection, and session-index maintenance require separate workload
measurements before defining a general supported capacity.

Run the capacity test explicitly, separately from correctness checks:

```powershell
cargo test -p savescummer-integration-tests --test history_scaling capacity_measurements -- --ignored --nocapture
```

The normal suite checks actual SQLite row writes, atomic rollback of a failed
batch, game/host cursor invalidation, pagination without duplicate/missing rows,
exact lookup of old operations, and boot/page/summary behavior with over 8 MiB of
audit metadata. External-backup scenarios compare indexed pages against the core's
complete visible-history projection, including sessions spanning page boundaries.
