# M43 — Dashboard Idle CPU

**Goal:** Make `rexymcp dashboard` cost approximately nothing when nothing is
happening, by (a) not re-reading unchanged files, (b) reading the telemetry store
once per refresh instead of three times, and (c) stopping `phase_runs.jsonl` from
growing without bound.

**Status:** planning

**Depends on:** M35 (the architect ledger and `scope_costs` core this reads), M40
(the sweep that appends the ledger snapshots), M8 (the dashboard itself)

**Exit criteria:**

- [x] Idle cost is **independent of telemetry-store size** — the 103 MB store and
      an empty one measure identically (both 4 %, from a 62 % baseline). Met by
      phase 01. *(Supersedes the original "≤ 2 % of one core", which was set from
      a mis-measurement — see § Measured, not inferred.)*
- [ ] An idle `rexymcp dashboard --repo .` consumes **≤ 2 % of one core**
      sustained, measured by pid identity with a liveness assertion. Requires
      phase 04.
- [x] A refresh that *does* have new data costs **one** read + parse of
      `phase_runs.jsonl`, not three. Met by phase 02 — reload work fell 3.2×
      (~77 → ~24 ticks) in an alternating A/B against the phase-01 binary.
- [x] `phase_runs.jsonl` stops growing monotonically while `rexymcp serve` idles.
      Met by phase 03 — verified against the real 48-session corpus: 145 records
      appended into an empty store, then **0 appended / 145 unchanged** on a
      re-harvest, and exactly **1 appended / 144 unchanged** after one message was
      added to one transcript. *(Requires restarting a running `serve` to pick up
      the new binary.)*
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

Against this repo, release build, dashboard left completely idle with no
keystrokes. **These figures were re-measured at phase-01 review** and supersede
the ones this milestone opened with — the original selector,
`pgrep -f "rexymcp dashboard" | head -1`, returns the `script` wrapper rather than
the dashboard (the wrapper's cmdline matches the same pattern and it holds the
lower pid), so every "0 %" it produced was the idle wrapper. Corrected figures
select by `/proc/<pid>/comm` and assert liveness across the window:

| Binary                | Telemetry store | Session log   | Idle CPU |
| --------------------- | --------------- | ------------- | -------- |
| pre-change (`73817b3`)| 103 MB          | real (1.5 MB) | **62 %** |
| phase-01 (`a2e9b43`)  | 103 MB          | real (1.5 MB) | **4 %**  |
| phase-01              | empty dir       | real (1.5 MB) | **4 %**  |
| phase-01              | 103 MB          | trivial       | **0 %**  |

Rows 2 and 3 are the load-bearing pair: after phase 01, the size of the telemetry
store no longer affects idle cost **at all**. Row 4 attributes the entire residual
4 % to the per-tick render of the session log.

**One conclusion this milestone opened with was wrong.** The original table's "0 %
with telemetry removed" was read off the wrapper, and it was used to justify
declaring the render path free and refusing it a phase. It is not free — it is
100 % of what remains. That refusal is withdrawn below and the work is now
phase 04.

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
| 01  | mtime-gated reload ([phase-01-mtime-gated-reload.md](phase-01-mtime-gated-reload.md)) | done   |
| 02  | single-pass telemetry read ([phase-02-single-pass-telemetry-read.md](phase-02-single-pass-telemetry-read.md)) | done |
| 03  | skip unchanged ledger appends ([phase-03-skip-unchanged-ledger-appends.md](phase-03-skip-unchanged-ledger-appends.md)) | done |
| 04  | memoize transcript render ([phase-04-memoize-transcript-render.md](phase-04-memoize-transcript-render.md)) | review      |
| 05  | reconcile the `schema_version` gate divergence                                       | todo   |
| 06  | compact the existing store (data-migration surface)                                  | todo   |

**01** removes the idle cost outright — the dashboard stops doing the work when
there is no new work to do. It is deliberately first because it is the smallest
change that resolves the reported symptom, and it is independent of the other two.

**02** attacks the per-refresh cost that remains when data *has* changed: one
read, one parse pass, dispatched on the `record` discriminator, replacing three
reads and five parse passes. This is a change in
`executor/src/store/telemetry.rs`, so it is scoped and sequenced separately from
the dashboard-local phase 01.

**03** attacks the root enabler — the write amplification — and was **split in two
when drafted**. The options sketched at milestone open (fold-before-append,
compaction, a separate last-write-wins file) turned out to have very different risk
profiles, and bundling them would have put a one-way rewrite of the user's
telemetry store inside the same review as a small, safe write-side guard. So:

- **03** is the write-side fix alone: harvest reads the folded ledger state once
  (via phase 02's `read_all`) and appends only buckets that actually differ.
  Stops the ~53 KB/minute of pure amplification. No migration surface — it only
  ever writes *fewer* records than today.
- **06** is compaction: reclaiming the existing 103 MB. It rewrites the store, so
  it gets its own phase, its own backup story, and its own review. It also has a
  trap worth naming now — compaction must **not** silently drop the 566
  unversioned legacy `PhaseRun` lines, because that would decide phase 05's open
  question by deletion rather than by argument.

03 deliberately accepts one new cost: a full store read per harvest (~150 ms once
per 60 s sweep tick against today's file), in exchange for stopping the appends.
That trade improves once 06 shrinks the file.

**05** was added while drafting phase 02, which had to pick a filtering semantics
and so surfaced a pre-existing defect: the dashboard's private `read_phase_runs`
(`mcp/src/dashboard/mod.rs:216`) has **no `schema_version` gate**, while
`telemetry::read` (`executor/src/store/telemetry.rs:214`) does. For this project's
own runs the two disagree by 2.4×:

| Reader                                | Runs | Executor input tokens |
| ------------------------------------- | ---- | --------------------- |
| dashboard (`read_phase_runs`, ungated)| 279  | 675,472,883           |
| `rexymcp costs` (`read`, gated)       | 55   | 287,266,673           |

566 of the 745 `PhaseRun` lines in the store predate M35 and carry no
`schema_version`. Phase 02 **preserves both behaviors exactly** — per an explicit
decision that a visible numbers change should be reviewed on its own merits rather
than folded into a performance refactor. Phase 05 picks the winner. Note
`architecture.md` §35 already states pre-M35 records go dark, which argues the
dashboard is the outlier; that is phase 05's argument to make, not phase 02's.

**04** was added at phase-01 review, when correcting the measurement showed the
render path is not free after all: it is the entire residual 4 %, re-wrapping and
re-highlighting a 1.5 MB session log on every 500 ms tick regardless of whether
anything changed. The likely shape mirrors phase 01 — the render inputs change far
less often than the tick — but the phase must **measure before choosing**, since
that is exactly the step this milestone got wrong the first time.

**Exit criterion revision.** The milestone's ≤ 2 % target was set from the bad
measurement. Phases 01–03 cannot reach it; the honest split is: 01–03 drive
*telemetry* cost to zero (done at 01, confirmed by rows 2–3 of the evidence table),
and 04 owns whatever idle cost remains.

## Notes

**Scope rejected on evidence — one rejection withdrawn.** Two things were
originally measured out of scope on the strength of the "0 %" row:

- The session log (up to 1.5 MB) is re-read and re-highlighted every frame.
  **Rejection withdrawn.** It is the whole of the residual 4 % (evidence table row
  4: the same binary against a trivial session log costs 0 %). This is now
  **phase 04**.
- `resolve_milestone` walks `docs/dev/milestones` (335 phase docs) and is
  effectively invoked twice per refresh (`mod.rs:80`, `mod.rs:81`). **Rejection
  stands** — it now runs only on the reload path, which the phase-01 gate makes
  rare, and row 4 shows the per-tick floor is 0 % with it still present.

**Calibration — the measurement lesson, filed at phase-01 review.** The
architect's own end-to-end command was wrong, and the phase doc handed it to the
executor verbatim, so the executor faithfully produced and reported a false green
(`idle CPU: 0%`) while the true figure was 4 %. Two folds follow, both about
*process measurement* specifically:

1. A measurement command that selects a process must select it by identity
   (`/proc/<pid>/comm`, a pidfile, `$!`), never by a substring of a command line
   that a wrapper (`script`, `sh -c`, `timeout`, `env`) also matches.
2. A measurement whose failure mode is indistinguishable from its success value —
   here, a dead or wrong process reading 0 % on a "lower is better" metric — must
   carry a liveness assertion, or it is not a measurement.

**Second occurrence, filed at phase-02 review — the pattern is now a trend.**
Phase 02's criterion was an **absolute** tick count (`≤ 70`) anchored to a render
baseline that drifts with machine load and session-log size. That baseline read 26
when the spec was written and ~72 for *both* binaries at review (with a 384-tick
outlier between), so a genuine 3.2× win presented as a miss and a stable render
floor presented as a regression. The robust form was the **delta**
(`reloading − quiescent`), measured by alternating A/B in one session.

**Third occurrence, filed at phase-03 review — the fold threshold is reached.**
Phase 03's end-to-end command counted `$SP/store.jsonl`, but `--telemetry-path`
ignores the filename it is given: `harvest()` takes the *parent* as the telemetry
dir (`mcp/src/harvest.rs:226`) and always writes `<parent>/phase_runs.jsonl`
(`:244`). So every count read 0, `after == mid` held trivially, and the check
reported success while measuring a file nothing ever wrote.

All three are the same architect error: **an end-to-end criterion stated in terms
the phase does not control**, and in each case the failure mode was
indistinguishable from success.

| Phase | What was measured                       | Why a "pass" was meaningless      |
| ----- | --------------------------------------- | --------------------------------- |
| 01    | a pid never verified to be the target   | the `script` wrapper reads 0 %     |
| 02    | a delta against a floor it did not own  | the floor drifted 26 → 72 → 384    |
| 03    | a file the binary never writes          | an untouched file never grows      |

Per WORKFLOW § Calibration, three is a fix. **Folded 2026-08-04 with the user's
sign-off** into `docs/dev/STANDARDS.md` § 1.1, "An end-to-end verification must
prove it is live", plus a cross-reference from the §1 end-to-end DoD box. The rule:

> An end-to-end check has to be able to fail. Every one carries a **positive
> control** — an observation in the same session that would come out differently
> if the measurement were not live. Seed a known-good starting state so the first
> observation must be non-zero; prefer a difference measured in one session over
> an absolute carried in from another environment; assert the subject exists,
> survived, and is the thing under test; check exit status.

Not propagated to `plugin/templates/STANDARDS.md` — the templates already lag the
resolved copies (248 vs 265 lines before this fold), and pushing a rule to every
future project is a product decision separate from adopting it here.

**Calibration note (one occurrence — hold, do not fold).** The defect class here
is "a reader whose cost scales with total history, on a store designed to be
append-only forever." M35 built the append-only store; M40 added the sweep that
writes to it every 60 s; M8's dashboard reads it at 2 Hz. Each was locally
reasonable. If a second such interaction shows up, the fold is a standing rule
about append-only stores needing a bounded-read contract at design time.
