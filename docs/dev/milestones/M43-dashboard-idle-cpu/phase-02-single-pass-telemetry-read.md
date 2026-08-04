# Phase 02: single-pass telemetry read

**Milestone:** M43 — Dashboard Idle CPU
**Status:** in-progress
**Depends on:** phase-01 (done — the reload gate that makes this the *only*
remaining telemetry cost)
**Estimated diff:** ~200 lines
**Tags:** language=rust, kind=refactor, size=m

## Goal

Collapse the dashboard's three full reads and five parse passes over
`phase_runs.jsonl` into **one read and one dispatched parse**. Phase 01 made the
reload rare; this makes each reload cheap. Filtering semantics must come out
**bit-for-bit identical** to today — this is a performance change only.

## Architecture references

Read before starting:

- `docs/dev/milestones/M43-dashboard-idle-cpu/README.md` — the measurements, and
  why the store is 99.95 % `architect_ledger` records.

## Pre-flight

1. Read `docs/dev/STANDARDS.md` top to bottom.
2. Read the architecture reference above.
3. Read this entire phase doc before touching any code.
4. Confirm the repo is on a clean branch with no uncommitted changes.

## Current state

`mcp/src/dashboard/mod.rs:94` and `:98–111` read the same file three times per
`load_data`:

```rust
    let phase_runs: Vec<PhaseRun> = telemetry_dir.map(read_phase_runs).unwrap_or_default();

    match project_id {
        Some(pid) => {
            let folded_activities = match telemetry_dir {
                Some(dir) => telemetry::fold_activities(
                    telemetry::read_architect_activities(&dir.join("phase_runs.jsonl"))
                        .unwrap_or_default(),
                ),
                _ => Vec::new(),
            };
            let ledgers = match telemetry_dir {
                Some(dir) => telemetry::fold_ledger(
                    telemetry::read_architect_ledger(&dir.join("phase_runs.jsonl"))
                        .unwrap_or_default(),
                ),
                _ => Vec::new(),
            };
```

The two `telemetry::` readers parse **every line twice** — once into an owned
`serde_json::Value`, once back out of it. `read_architect_ledger`
(`executor/src/store/telemetry.rs:712`) is representative; `read_architect_activities`
(`:576`), `read_reviews` (`:412`) and `read` (`:214`) are the same shape:

```rust
    Ok(content
        .lines()
        .filter(|l| !l.trim().is_empty())
        .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
        .filter(|v| {
            v.get("schema_version").and_then(serde_json::Value::as_u64)
                == Some(TELEMETRY_SCHEMA_VERSION as u64)
        })
        .filter_map(|v| serde_json::from_value::<ArchitectLedger>(v).ok())
        .filter(|l| l.record == ARCHITECT_LEDGER_RECORD_TAG)
        .collect())
```

The dashboard's own run reader (`mcp/src/dashboard/mod.rs:216`) is the odd one —
**no `schema_version` gate**:

```rust
fn read_phase_runs(telemetry_dir: &Path) -> Vec<PhaseRun> {
    let path = telemetry_dir.join("phase_runs.jsonl");
    let Ok(content) = std::fs::read_to_string(&path) else {
        return Vec::new();
    };
    content
        .lines()
        .filter(|l| !l.trim().is_empty())
        .filter_map(|l| serde_json::from_str(l).ok())
        .collect()
}
```

Relevant constants, all already `pub` in `executor/src/store/telemetry.rs`:

| Constant                        | Line  | Value                  |
| ------------------------------- | ----- | ---------------------- |
| `TELEMETRY_SCHEMA_VERSION`      | `190` | `1` (a `u32`)          |
| `REVIEW_RECORD_TAG`             | `387` | `"review"`             |
| `ARCHITECT_ACTIVITY_RECORD_TAG` | `512` | `"architect_activity"` |
| `ARCHITECT_LEDGER_RECORD_TAG`   | `596` | `"architect_ledger"`   |

`PhaseRun` (`:121`) carries **no** `record` field and **no** `schema_version`
field — `schema_version` is injected into the JSON at the write boundary
(`:201`, `value["schema_version"] = TELEMETRY_SCHEMA_VERSION.into();`), not
deserialized back onto the struct.

## Spec

### 1. Add `StoreRecords` + `read_all` to `executor/src/store/telemetry.rs`

Additive only — **do not modify or delete** `read`, `read_reviews`,
`read_architect_activities`, or `read_architect_ledger`. Other callers
(`mcp/src/costs.rs`, `runs.rs`, `harvest.rs`, `server.rs`, `profile_cli.rs`,
`scorecard_cli.rs`) keep using them; see Out of scope.

```rust
/// Every record type in one pass over the store. Filtering is **identical** to
/// the per-type readers, including one deliberate inconsistency between them —
/// see the field docs.
#[derive(Debug, Default)]
pub struct StoreRecords {
    /// Every line that deserializes as a `PhaseRun`, with **no**
    /// `schema_version` gate. This matches the dashboard's `read_phase_runs`
    /// and deliberately does **not** match `read` (`:214`), which gates.
    /// The divergence is real and pre-existing (it makes the dashboard and
    /// `rexymcp costs` disagree); reconciling it is M43 phase-05, NOT this phase.
    pub runs: Vec<PhaseRun>,
    /// `schema_version == TELEMETRY_SCHEMA_VERSION` AND
    /// `record == ARCHITECT_ACTIVITY_RECORD_TAG` — identical to
    /// `read_architect_activities`.
    pub activities: Vec<ArchitectActivity>,
    /// `schema_version == TELEMETRY_SCHEMA_VERSION` AND
    /// `record == ARCHITECT_LEDGER_RECORD_TAG` — identical to
    /// `read_architect_ledger`.
    pub ledgers: Vec<ArchitectLedger>,
}

/// Read the store once, dispatching each line on its `record` discriminator.
/// A missing file yields `StoreRecords::default()`, matching the per-type
/// readers' `NotFound` behavior. Malformed lines are skipped silently.
pub fn read_all(path: &Path) -> std::io::Result<StoreRecords> {
    // ...
}
```

`reviews` are deliberately **not** collected — the dashboard has no consumer for
them and an unused field is dead weight. A later phase adds it when something
needs it.

**The dispatch, and the one thing that makes this fast.** Do *not* parse each
line into a `serde_json::Value`. Parse it into a tiny header struct first — serde
walks the line and discards unknown fields without allocating them — then parse
the line again directly into the concrete type:

```rust
#[derive(serde::Deserialize)]
struct RecordHead {
    /// Absent on `PhaseRun` lines, which carry no discriminator.
    #[serde(default)]
    record: String,
    /// Absent on pre-M35 lines.
    #[serde(default)]
    schema_version: u32,
}
```

Per non-empty line:

1. `serde_json::from_str::<RecordHead>(line)` — on `Err`, skip the line.
2. Match on `head.record.as_str()`:
   - `ARCHITECT_LEDGER_RECORD_TAG` → if `head.schema_version ==
     TELEMETRY_SCHEMA_VERSION`, `from_str::<ArchitectLedger>(line)` and push on
     `Ok`. Otherwise skip.
   - `ARCHITECT_ACTIVITY_RECORD_TAG` → same shape, into `activities`.
   - `""` (no discriminator) → `from_str::<PhaseRun>(line)`, push on `Ok`, **with
     no `schema_version` check**.
   - anything else (including `REVIEW_RECORD_TAG`) → skip.

**Do NOT sniff the record type with a substring search** (`line.contains(
"\"record\":\"architect_ledger\"")`). It looks faster and it is a trap: it binds
to serde's current key ordering and exact spacing, and it matches the same bytes
appearing inside a string value elsewhere in the line. Parse the header struct.

**Why restricting `PhaseRun` parsing to untagged lines is safe** — today's
`read_phase_runs` attempts `PhaseRun` on *every* line, including tagged ones.
That is equivalent to the dispatch above because no tagged record can
deserialize as a `PhaseRun`: `PhaseRun` requires `ts`, `model`,
`generation_params`, `phase_id`, `tags`, `status`, `escalated`, `gates`,
`parse_failure_rate`, `repairs_per_call`, `verifier_retries`,
`tool_success_rate`, `turns`, `wall_clock_s` and `tokens` with **no**
`#[serde(default)]`, and `PhaseReview` (`:357`), `ArchitectActivity` (`:483`) and
`ArchitectLedger` (`:607`) each lack most of them. This equivalence is load-bearing
and is pinned by a test below — if someone later adds `#[serde(default)]` to a
`PhaseRun` field, that test must fail rather than the dashboard's numbers moving
silently.

### 2. Rewire `load_data` in `mcp/src/dashboard/mod.rs`

Replace the three reads with one. At the top of `load_data`, before the
`match project_id`:

```rust
    let store = telemetry_dir
        .map(|dir| telemetry::read_all(&dir.join("phase_runs.jsonl")).unwrap_or_default())
        .unwrap_or_default();
    let phase_runs: Vec<PhaseRun> = store.runs;
```

Then inside the `Some(pid)` arm, `folded_activities` and `ledgers` fold the
already-read vectors instead of re-reading:

```rust
            let folded_activities = telemetry::fold_activities(store.activities);
            let ledgers = telemetry::fold_ledger(store.ledgers);
```

Note the `match telemetry_dir { Some(dir) => ..., _ => Vec::new() }` wrappers
disappear — `read_all` on an absent dir already yields empty vectors, so the
`None` case is handled by the `unwrap_or_default()` above. Everything downstream
(`costs::scope_costs`, `project_escalation_count`, `skill_costs`) keeps its
current shape and arguments.

Delete `read_phase_runs` (`mcp/src/dashboard/mod.rs:216`) and its doc comment —
it has no callers once this lands, and a dead private fn fails the lint gate.

### 3. Tests

Unit tests for `read_all` go in the existing `#[cfg(test)] mod tests` at the
bottom of `executor/src/store/telemetry.rs`, alongside
`read_architect_activities_version_gates` (`:791`) — reuse that block's
`TempDir` + `std::fs::write` fixture style.

The dashboard rewire needs no new test of its own: the existing `load_data_*`
tests in `mcp/src/dashboard/mod.rs` (`load_data_reads_project_savings_from_phase_runs`,
`load_data_reads_project_architect_costs_from_ledger`,
`load_data_counts_assist_journal_records_as_escalations`, and the
`load_data_project_savings_*` pair) already pin the numbers `load_data` produces
from a fixture store. **They must pass unchanged.** If one needs editing, stop
and file a blocker — an edit there means the rewire changed behavior, which is
exactly what this phase forbids.

## Acceptance criteria

- [ ] `cargo build` succeeds.
- [ ] `cargo clippy --all-targets --all-features -- -D warnings` is clean.
- [ ] `cargo fmt --all --check` reports no diff. (Fix with `rustfmt <file>` on
      touched files only — never `cargo fmt --all`.)
- [ ] `cargo test` passes, with **every existing `load_data_*` test unmodified**.
- [ ] `phase_runs.jsonl` is opened exactly once per `load_data`: `grep -c
      'phase_runs.jsonl' mcp/src/dashboard/mod.rs` shows the path constructed in
      exactly one place.
- [ ] `read_phase_runs` no longer exists in `mcp/src/dashboard/mod.rs`.
- [ ] No `serde_json::Value` appears in `read_all`.
- [ ] The reload-path measurement below shows **≤ 70 ticks**, down from 124.

## Test plan

In `executor/src/store/telemetry.rs`'s `mod tests`:

- `read_all_collects_each_record_type_in_one_pass` — a fixture with one
  `PhaseRun` line, one activity, one ledger and one review; assert
  `runs.len() == 1`, `activities.len() == 1`, `ledgers.len() == 1`.
- `read_all_runs_are_not_schema_version_gated` — a `PhaseRun` line **without**
  `schema_version` is present in `runs`. Pins the dashboard's semantics and the
  deliberate divergence from `read`.
- `read_all_activities_and_ledgers_are_schema_version_gated` — an activity line
  and a ledger line each carrying `schema_version: 0` are **absent** from
  `activities` / `ledgers`.
- `read_all_does_not_parse_tagged_records_as_runs` — a file containing **only**
  a ledger line, an activity line and a review line yields `runs.is_empty()`.
  This is the equivalence guard described in Spec §1; assert it explicitly rather
  than relying on the counts in the first test.
- `read_all_matches_per_type_readers_on_the_same_file` — build one mixed fixture,
  then assert `read_all(&p).activities.len() == read_architect_activities(&p).len()`
  and the same for `ledgers` / `read_architect_ledger`. The strongest guard that
  this refactor preserved semantics; compare lengths and the identifying field of
  each element (e.g. ledger `session_id`, activity `activity`), not full structs —
  neither type implements `PartialEq`.
- `read_all_missing_file_returns_empty` — a path that does not exist yields all
  three vectors empty and `Ok`, not `Err`.
- `read_all_skips_malformed_lines` — a fixture with a blank line, a
  `not json at all` line, and one valid ledger line yields exactly one ledger.

## End-to-end verification

Phase 01 made the *idle* path free, so this phase's win is only visible on the
**reload** path — with the watched file actively changing. Measure that, using a
scratch copy of the store so the real one is never mutated.

> **Measurement discipline (M43 phase-01 lesson).** `script`'s own cmdline
> contains `rexymcp dashboard` and it holds the lower pid, so
> `pgrep -f … | head -1` selects the **wrapper**, which is idle by construction
> and reads 0 %. Select by `/proc/<pid>/comm` and assert the process is alive at
> the end of the window, exactly as below. A measurement whose failure mode reads
> as success is not a measurement.

```bash
cargo build --release
SP=$(mktemp -d)
mkdir -p "$SP/tel"
head -60000 ~/.rexymcp/telemetry/phase_runs.jsonl > "$SP/tel/phase_runs.jsonl"
sed 's#^dir = "/home/matt/.rexymcp/telemetry"#dir = "'"$SP"'/tel"#' rexymcp.toml > "$SP/live.toml"

script -qec "target/release/rexymcp dashboard --repo . --config $SP/live.toml" /dev/null >/dev/null 2>&1 &
sleep 4
PID=""
for p in $(pgrep -f "rexymcp dashboard"); do
  [ -r "/proc/$p/comm" ] || continue
  if [ "$(cat /proc/$p/comm)" = "rexymcp" ]; then PID=$p; break; fi
done
[ -n "$PID" ] || { echo "FAIL: dashboard process not found"; exit 1; }

read -r _ _ _ _ _ _ _ _ _ _ _ _ _ U1 S1 _ < /proc/$PID/stat
sleep 6
read -r _ _ _ _ _ _ _ _ _ _ _ _ _ U2 S2 _ < /proc/$PID/stat
echo "quiescent (6s): $((U2-U1+S2-S1)) ticks"

for i in 1 2 3 4 5 6; do
  tail -1 "$SP/tel/phase_runs.jsonl" >> "$SP/tel/phase_runs.jsonl"
  sleep 1
done
[ -d "/proc/$PID" ] || { echo "FAIL: process died during sample"; exit 1; }
read -r _ _ _ _ _ _ _ _ _ _ _ _ _ U3 S3 _ < /proc/$PID/stat
echo "reloading (6s): $((U3-U2+S3-S2)) ticks"
kill "$PID"
```

**Measured on the phase-01 tree (the baseline to beat):**

```
quiescent (6s): 26 ticks
reloading (6s): 124 ticks
```

The ~26 quiescent ticks are the per-tick render, which this phase does not touch
(that is phase 04) — so ~98 of the 124 are telemetry parsing. Removing two of
three reads and the `Value` round-trip must bring `reloading` to **≤ 70 ticks**.
`quiescent` must stay at ~26 — if it moves, something outside this phase's scope
changed. Quote both literal output lines in the completion Update Log.

## Authorizations

None. No new dependency, no `Cargo.toml` edit. Touches only
`executor/src/store/telemetry.rs` and `mcp/src/dashboard/mod.rs`.

## Out of scope

- **The `schema_version` divergence between `read_phase_runs` and `read`.** It is
  real — it makes the dashboard report 279 runs / 675M input tokens for this
  project where `rexymcp costs` reports 55 / 287M — and it is **deliberately
  preserved** by this phase. Reconciling it is **phase 05**, kept separate so a
  visible numbers change is reviewed on its own merits instead of riding inside a
  performance refactor. Do not "fix" it here.
- **The other callers** of the per-type readers (`mcp/src/costs.rs:268–275`,
  `runs.rs:309`, `harvest.rs:346`, `server.rs:495,543`, `profile_cli.rs:36,65`,
  `scorecard_cli.rs:35`). They are one-shot CLI/tool invocations, not a 2 Hz
  loop, so they are not the problem this milestone is solving. Leave them, and
  leave the four per-type readers themselves unchanged.
- **Deleting or rewriting `read` / `read_reviews` /
  `read_architect_activities` / `read_architect_ledger`.** `read_all` is
  additive. Their tests must keep passing untouched.
- **The store's growth** (phase 03) and **the render path** (phase 04).
- **Changing what the dashboard displays.** Any diff in a `load_data_*` test is a
  blocker, not a fix.

## Update Log

(Filled in by the executor. See WORKFLOW.md § "Update Log entries".)

<!-- entries appended below this line -->

### Update — 2026-08-04 16:23 (started)

**Executor:** Claude (Sonnet)

**Work:** Implementing `StoreRecords` + `read_all` single-pass telemetry reader and rewiring `load_data` to use it.
