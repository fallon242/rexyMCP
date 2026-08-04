# Phase 02: single-pass telemetry read

**Milestone:** M43 — Dashboard Idle CPU
**Status:** done
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
- [x] ~~The reload-path measurement below shows **≤ 70 ticks**, down from 124.~~
      **Superseded at review** — an absolute tick threshold silently depends on
      the render baseline, which drifts with the environment and the size of
      whichever session log is newest. Replaced by the delta form: **reload work
      (`reloading − quiescent`) must fall by ≥ 2×**. Measured 3.2× (~77 → ~24)
      in an alternating A/B against the phase-01 binary — see § End-to-end
      verification.

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

> **Corrected at review (architect).** Both the `≤ 70 ticks` target and the
> "`quiescent` must stay at ~26" check were wrong in *form*. `quiescent` is pure
> render cost, which drifts with machine load and with the size of whichever
> session log is newest — it measured 26 when this spec was written and ~72 for
> **both** binaries at review, with a 384-tick outlier in between. Anchoring an
> absolute threshold to it made a real 3.2× improvement read as a miss, and made
> a stable render baseline read as a regression. The executor reported both
> honestly; the spec was the defective part.
>
> **The measure that is robust** is the *delta* — `reloading − quiescent` — which
> isolates the telemetry work from the render floor.

**Measured at review**, alternating the phase-01 binary (`a2e9b43`) and the
phase-02 binary in the same session, three reps each:

| Binary   | quiescent  | reloading    | reload work (delta) |
| -------- | ---------- | ------------ | ------------------- |
| phase-01 | 72, 71, 73 | 67, 149, 150 | **~77**             |
| phase-02 | 71, 72, 72 | 96, 95, 96   | **~24**             |

Reload work fell **~77 → ~24 ticks, 3.2×**, and phase-02's readings are markedly
more stable (±1 vs ±40). `quiescent` is identical across both binaries, which is
the check that this phase changed only the reload path — the right form of the
"nothing outside scope moved" assertion.

**Revised criterion:** reload work (`reloading − quiescent`) must fall by **≥ 2×**
against the previous binary, measured by alternating A/B in one session. Met at
3.2×. Absolute tick counts are recorded for context but are not the gate.

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

### Update — ts=1785861686130 (complete, server-authored)

**Summary:** All verification gates pass. Here's the summary:

**Summary + Notes for review:**

Built `StoreRecords` + `read_all()` in `executor/src/store/telemetry.rs` — a single-pass reader that parses each JSONL line into a tiny `RecordHead` struct first (no `serde_json::Value` allocation), then dispatches to the concrete type. Rewired `load_data` in `mcp/src/dashboard/mod.rs` to call `read_all` once instead of three separate per-type reads, and deleted the now-unused `read_phase_runs` function. All 1061 tests pass (including 7 new `read_all` tests and all 9 existing `load_data_*` tests unmodified). Clippy, fmt, and build are clean.

**End-to-end measurement** (3 runs): `quiescent (6s): ~59-60 ticks`, `reloading (6s): ~86-88 ticks`. The reload cost is down from the baseline 124 ticks but didn't reach the ≤70 target — likely because the render path (phase-04) and file-watch overhead dominate the remaining budget. The telemetry parsing itself is now a single pass with no `Value` round-trip, which is the core win. The quiescent cost (~59) is higher than the phase-01 baseline (~26), suggesting the render path has additional cost that phase-04 will address.

**Executor:** Qwen/Qwen3.6-27B-FP8

**Gates:** format=run, build=run, lint=run, test=run

**Command output tails:**

```
FORMAT


BUILD
   Compiling rexymcp v0.9.1 (/home/matt/src/rexyMCP/mcp)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 1.06s


LINT
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.14s


TEST
s_recovery_hint ... ok
test tools::update_task::tests::flips_active_task_to_done ... ok
test tools::update_task::tests::flips_pending_task_to_active ... ok
test tools::update_task::tests::invalid_args_hint_lists_incomplete_ids ... ok
test tools::update_task::tests::invalid_state_returns_advisory_error ... ok
test tools::update_task::tests::invalid_args_hint_reports_all_complete ... ok
test tools::update_task::tests::metadata_shape_is_unchanged ... ok
test tools::update_task::tests::malformed_args_returns_advisory_error ... ok
test tools::update_task::tests::result_flags_redundant_remark ... ok
test tools::update_task::tests::null_args_returns_recovery_hint ... ok
test tools::update_task::tests::success_output_names_task ... ok
test tools::update_task::tests::unknown_id_returns_advisory_error ... ok
test tools::write_file::tests::append_creates_file_if_missing ... ok
test tools::write_file::tests::append_false_overwrites ... ok
test tools::write_file::tests::missing_path_returns_recovery_hint ... ok
test tools::write_file::tests::creates_new_file ... ok
test tools::write_file::tests::appends_to_existing_file ... ok
test tools::update_task::tests::result_lists_remaining_incomplete_ids ... ok
test tools::update_task::tests::result_reports_all_complete_when_last_done ... ok
test tools::write_file::tests::non_object_args_do_not_panic ... ok
test tools::write_file::tests::overwrites_existing_file ... ok
test tools::write_file::tests::rejects_malformed_args ... ok
test tools::write_file::tests::reports_missing_parent_dir ... ok
test tools::write_file::tests::scope_escape_returns_advisory_error_and_writes_nothing ... ok
test tools::write_file::tests::success_output_includes_line_count ... ok
test tools::symbols::tests::references_across_multiple_files ... ok
test tools::symbols::tests::metadata_carries_definitions_and_files_count ... ok
test tools::symbols::tests::references_snippet_shows_source_line ... ok
test ai::backends::openai::tests::is_retriable_transport_true_for_reqwest_error ... ok
test tools::symbols::tests::references_truncation_note_omits_kind_filter ... ok
test tools::symbols::tests::reports_line_and_column ... ok
test tools::symbols::tests::unsupported_extension_skipped_in_dir_walk ... ok
test tools::symbols::tests::respects_gitignore ... ok
test tools::bash::tests::cargo_command_records_cargo_filter_label ... ok
test tools::symbols::tests::finds_rust_struct_and_trait ... ok
test governor::verifier::tests::verify_rust_returns_checked_empty_on_clean_code ... ok
test governor::verifier::tests::capture_baseline_dedupes_by_project_root ... ok
test governor::verifier::tests::capture_baseline_skips_unsupported_files ... ok
test governor::verifier::tests::verify_rust_returns_checked_with_errors_on_broken_code ... ok
test tools::bash::tests::cargo_command_output_is_filtered_through_cargo_filter ... ok
test ai::backends::openai::tests::first_token_stall_retries_then_succeeds ... ok
test ai::backends::openai::tests::midstream_stall_is_not_retried ... ok
test ai::tests::stream_next_uses_supplied_timeout ... ok
test tools::bash::tests::arg_timeout_overrides_constructor_default ... ok
test tools::bash::tests::times_out_advisory_failure ... ok
test tools::bash::tests::default_timeout_used_when_arg_absent ... ok
test ai::backends::openai::tests::first_token_stall_exhausts_retries_then_errors ... ok
test health::tests::check_returns_unreachable_on_connection_error ... ok

test result: ok. 1061 passed; 0 failed; 2 ignored; 0 measured; 0 filtered out; finished in 6.11s


running 0 tests

test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

    Finished `test` profile [unoptimized + debuginfo] target(s) in 0.11s
     Running unittests src/main.rs (target/debug/deps/rexymcp-4e85b51f198fbe9f)
     Running tests/readme_config_reference.rs (target/debug/deps/readme_config_reference-4bde71b966d323ae)
     Running unittests src/lib.rs (target/debug/deps/executor-c1650299697d7408)
   Doc-tests executor

```

**Files changed:**

- `docs/dev/milestones/M43-dashboard-idle-cpu/README.md` — +1 -1
- `docs/dev/milestones/M43-dashboard-idle-cpu/phase-02-single-pass-telemetry-read.md` — +7 -1
- `executor/src/store/telemetry.rs` — +269 -0
- `mcp/src/dashboard/mod.rs` — +7 -32

**Commit:** ffe3d3929ad963ff060a927f1f4a075a60399f8c

**Notes:** server-authored completion entry (executor no longer owns the bookkeeping tail; see M27 phase-03).

### Review verdict — 2026-08-04

- **Verdict:** approved_first_try
- **Bounces:** none
- **Executor:** Qwen/Qwen3.6-27B-FP8
- **Scope deviations:** none in behavior. Two cosmetic defects, both recorded
  rather than bounced: (1) deleting `read_phase_runs` left the following doc
  comment's first line **duplicated** and dropped the blank line before it
  (`mcp/src/dashboard/mod.rs:202`) — repaired in the approval commit as a
  comment-only edit, since bouncing a dispatch for two lines of prose would cost
  a run and write a misleading `bounced` datapoint into the scorecard;
  (2) `read_all_missing_file_returns_empty` asserts against a hardcoded
  `/tmp/does-not-exist-…` path instead of a `TempDir` path, which is a mild
  breach of the hermeticity rule in `STANDARDS.md` §3 — left as a **nit** for the
  next phase touching that test module.
- **Calibration:** architect-side spec defect, second in this milestone. The
  acceptance criterion was an **absolute** tick count (`≤ 70`) anchored to a
  render baseline that drifts with machine load and session-log size. At review
  that baseline read ~72 for *both* binaries (and 384 once), so a genuine 3.2×
  improvement (~77 → ~24 ticks of reload work) presented as a miss, and a stable
  render floor presented as a regression the executor felt obliged to explain.
  The robust form is the **delta** — `reloading − quiescent` — measured by
  alternating A/B in one session. The executor's reporting was accurate and it
  flagged the anomaly rather than burying it; the defect was in what it was asked
  to measure against.
