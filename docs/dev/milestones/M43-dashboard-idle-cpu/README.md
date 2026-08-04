# M43 — Dashboard Idle CPU

**Goal:** Make `rexymcp dashboard` cost approximately nothing when nothing is
happening, by (a) not re-reading unchanged files, (b) reading the telemetry store
once per refresh instead of three times, and (c) stopping `phase_runs.jsonl` from
growing without bound.

**Status:** planning

**Depends on:** M35 (the architect ledger and `scope_costs` core this reads), M40
(the sweep that appends the ledger snapshots), M8 (the dashboard itself)

**Exit criteria:**

- [ ] An idle `rexymcp dashboard --repo .` against this repo's 103 MB telemetry
      store consumes **≤ 2 % of one core** sustained (measured from
      `/proc/<pid>/stat`; baseline today is 59 %).
- [ ] A refresh that *does* have new data costs **one** read + parse of
      `phase_runs.jsonl`, not three.
- [ ] `phase_runs.jsonl` stops growing monotonically while `rexymcp serve` idles.
- [ ] No behavior change visible in the TUI: the same panels, the same numbers,
      the same follow/scroll semantics.

## Architecture references

- `docs/architecture.md#status` §8 (M8 — dashboard), §35 (metrics & cost
  accounting), §40 (token-ledger dash alignment)

## Why this milestone exists

Reported symptom: `rexymcp dashboard --repo .` pins a core even when `rexymcp
serve` is idle, and only on long-lived projects (`~/src/rexyMCP`,
`~/src/daemoneye`).

### Measured, not inferred

Against this repo at `659d321`, release build, dashboard left completely idle
with no keystrokes:

| Configuration                              | Idle CPU (10 s sample) |
| ------------------------------------------ | ---------------------- |
| `--repo .` (telemetry dir = 103 MB store)  | **59 %** of one core   |
| `--repo .` with `[telemetry] dir` empty    | **0 %**                |

The second row is the load-bearing one: with the telemetry store removed from the
picture the dashboard is free. **All** of the cost is `phase_runs.jsonl`. Nothing
else in the refresh path — rendering, wrapping, syntax highlighting, the session
log re-read, the milestone directory walk — is worth a phase of work, and this
milestone deliberately does not spend one on them.

### The three multiplied factors

**1. The refresh is unconditional.** `event_loop::run_loop`
(`mcp/src/dashboard/event_loop.rs:29`) calls `load_data` at the top of every
iteration and then blocks for at most 500 ms on `event::poll`
(`event_loop.rs:64`). Nothing asks whether any input file changed. Idle and busy
cost exactly the same.

**2. Each refresh reads the file three times.** `load_data`
(`mcp/src/dashboard/mod.rs:46`) makes three independent full passes:

| Call                                  | Site                                    | Parse shape                                     |
| ------------------------------------- | --------------------------------------- | ----------------------------------------------- |
| `read_phase_runs`                     | `mcp/src/dashboard/mod.rs:175`          | `from_str::<PhaseRun>` per line                  |
| `telemetry::read_architect_activities`| `executor/src/store/telemetry.rs:576`   | `from_str::<Value>` **then** `from_value::<T>`   |
| `telemetry::read_architect_ledger`    | `executor/src/store/telemetry.rs:712`   | `from_str::<Value>` **then** `from_value::<T>`   |

The latter two parse every line twice — once into an owned `serde_json::Value`,
once out of it — to read a `schema_version` field they could have matched on the
raw line. Measured on the real file: `read_to_string` 20 ms, one `Value` pass
178 ms. Three passes plus the `from_value` half is ≥ 600 ms of CPU per refresh,
scheduled every 500 ms. The loop can never keep up; the 59 % is that arithmetic.

**3. The file is 99.95 % redundant.** `phase_runs.jsonl` is 103 MB / 278,836
lines, composed as:

| Record type           | Lines       |
| --------------------- | ----------- |
| `architect_ledger`    | 278,226     |
| `PhaseRun` (untagged) | 743         |
| `review`              | 306         |
| `architect_activity`  | 323         |

The sweep inside `rexymcp serve` (`mcp/src/sweep.rs`) re-harvests every 60 s and
appends the **entire** ledger — currently 143 records per tick, per its own
liveness marker `{"outcome":"143 records / 7176 msgs"}` — whenever any Claude Code
transcript has changed, which is continuously while the architect is working.
`fold_ledger` (`executor/src/store/telemetry.rs:666`) then collapses all 278,226
lines back down to ~143 by last-write-wins on
`(project_id, session_id, model, skill)`. Every byte beyond that ~143 is written,
stored, re-read, and re-parsed forever, only to be discarded.

This is why the bug is invisible on new projects and fatal on old ones: at 1 MB
the same code costs ~2 ms per refresh.

## Phases

| #   | Phase                                                                              | Status |
| --- | ---------------------------------------------------------------------------------- | ------ |
| 01  | mtime-gated reload ([phase-01-mtime-gated-reload.md](phase-01-mtime-gated-reload.md)) | review        |
| 02  | single-pass telemetry read                                                          | todo   |
| 03  | bound `phase_runs.jsonl` growth                                                     | todo   |

**01** removes the idle cost outright — the dashboard stops doing the work when
there is no new work to do. It is deliberately first because it is the smallest
change that resolves the reported symptom, and it is independent of the other two.

**02** attacks the per-refresh cost that remains when data *has* changed: one
read, one parse pass, dispatched on the `record` discriminator, replacing three
reads and five parse passes. This is a change in
`executor/src/store/telemetry.rs`, so it is scoped and sequenced separately from
the dashboard-local phase 01.

**03** attacks the root enabler — the write amplification. Options to be settled
when the phase is drafted: fold-before-append in the sweep, a compaction pass over
the store, or splitting the ledger into its own last-write-wins file. Sequenced
last because it is the only one with a data-migration surface, and because 01 + 02
already make the store's size a non-problem for readers.

## Notes

**Scope explicitly rejected on evidence.** Two things looked like contributors and
were measured out:

- `resolve_milestone` walks `docs/dev/milestones` (335 phase docs) and is
  effectively invoked twice per refresh (`mod.rs:80` and `mod.rs:81`). Real, but
  inside the 0 % row above.
- The session log (up to 1.5 MB) is re-read and re-highlighted every frame. Also
  inside the 0 % row.

Neither gets a phase. If a future measurement promotes them, they come back as
their own milestone with their own numbers.

**Calibration note (one occurrence — hold, do not fold).** The defect class here
is "a reader whose cost scales with total history, on a store designed to be
append-only forever." M35 built the append-only store; M40 added the sweep that
writes to it every 60 s; M8's dashboard reads it at 2 Hz. Each was locally
reasonable. If a second such interaction shows up, the fold is a standing rule
about append-only stores needing a bounded-read contract at design time.
