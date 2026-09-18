# Phase 1: count unparsed

**Milestone:** F12 — Telemetry unparsed count
**Status:** done
**Depends on:** none
**Estimated diff:** ~150 lines, most of it tests
**Tags:** language=rust, kind=feature, size=s

## Goal

`read_all` counts telemetry lines it could not use although they claim the
current schema, and `rexymcp costs` shows that count. Today such lines are
skipped silently.

## Pre-flight

Measured 2026-09-18:

1. `cargo test -p rexymcp-executor read_all` → **7 passed**.
2. `cargo test -p rexymcp costs::` → **33 passed**.
3. Full `cargo test` → **727 / 2 / 1212 passed**.

## Spec

### 1. `StoreRecords.unparsed` — `executor/src/store/telemetry.rs`

Add as the last field of `StoreRecords` (it derives `Default`, so no literal
needs updating):

```rust
    /// Lines that claim the current `schema_version` and a known record type
    /// but fail to deserialize into it, plus lines that are not JSON at all.
    /// Old-schema lines and unknown record types are skipped, not counted.
    pub unparsed: usize,
```

### 2. Count in `read_all` — same file

Replace the whole loop body of `read_all`:

```rust
    for line in content.lines().filter(|l| !l.trim().is_empty()) {
        let head = match serde_json::from_str::<RecordHead>(line) {
            Ok(h) => h,
            Err(_) => continue,
        };

        match head.record.as_str() {
            ARCHITECT_LEDGER_RECORD_TAG => {
                if head.schema_version == TELEMETRY_SCHEMA_VERSION
                    && let Ok(l) = serde_json::from_str::<ArchitectLedger>(line)
                {
                    records.ledgers.push(l);
                }
            }
            ARCHITECT_ACTIVITY_RECORD_TAG => {
                if head.schema_version == TELEMETRY_SCHEMA_VERSION
                    && let Ok(a) = serde_json::from_str::<ArchitectActivity>(line)
                {
                    records.activities.push(a);
                }
            }
            "" => {
                if head.schema_version == TELEMETRY_SCHEMA_VERSION
                    && let Ok(r) = serde_json::from_str::<PhaseRun>(line)
                {
                    records.runs.push(r);
                }
            }
            _ => {}
        }
    }
```

with:

```rust
    for line in content.lines().filter(|l| !l.trim().is_empty()) {
        let head = match serde_json::from_str::<RecordHead>(line) {
            Ok(h) => h,
            Err(_) => {
                records.unparsed += 1;
                continue;
            }
        };
        if head.schema_version != TELEMETRY_SCHEMA_VERSION {
            continue;
        }

        match head.record.as_str() {
            ARCHITECT_LEDGER_RECORD_TAG => match serde_json::from_str::<ArchitectLedger>(line) {
                Ok(l) => records.ledgers.push(l),
                Err(_) => records.unparsed += 1,
            },
            ARCHITECT_ACTIVITY_RECORD_TAG => {
                match serde_json::from_str::<ArchitectActivity>(line) {
                    Ok(a) => records.activities.push(a),
                    Err(_) => records.unparsed += 1,
                }
            }
            REVIEW_RECORD_TAG => {
                if serde_json::from_str::<PhaseReview>(line).is_err() {
                    records.unparsed += 1;
                }
            }
            "" => match serde_json::from_str::<PhaseRun>(line) {
                Ok(r) => records.runs.push(r),
                Err(_) => records.unparsed += 1,
            },
            _ => {}
        }
    }
```

Also change the doc comment line above `read_all` from
`/// readers' `NotFound` behavior. Malformed lines are skipped silently.` to
`/// readers' `NotFound` behavior. Unusable lines are counted in `unparsed`.`

### 3. `rexymcp costs` reads once — `mcp/src/costs.rs`

Add as the last field of `CostReport`:

```rust
    /// `StoreRecords::unparsed` — telemetry lines that failed to parse.
    pub unparsed: usize,
```

In `load_cost_report`, replace:

```rust
    let runs: Vec<PhaseRun> =
        telemetry::read(&telemetry_file).map_err(|e| format!("failed to read telemetry: {e}"))?;
    let activities = telemetry::fold_activities(
        telemetry::read_architect_activities(&telemetry_file).unwrap_or_default(),
    );
    let ledgers = telemetry::fold_ledger(
        telemetry::read_architect_ledger(&telemetry_file).unwrap_or_default(),
    );
```

with:

```rust
    let store = telemetry::read_all(&telemetry_file)
        .map_err(|e| format!("failed to read telemetry: {e}"))?;
    let unparsed = store.unparsed;
    let runs: Vec<PhaseRun> = store.runs;
    let activities = telemetry::fold_activities(store.activities);
    let ledgers = telemetry::fold_ledger(store.ledgers);
```

Set `unparsed,` in the first `Ok(CostReport { … })` (inside
`if let Some(pid)`) and `unparsed,` in the second (the `else` branch).

In `format_costs`, replace:

```rust
    lines.push(format!("Assists: {}", report.assists));
```

with:

```rust
    lines.push(format!("Assists: {}", report.assists));
    if report.unparsed > 0 {
        lines.push(format!(
            "Unreadable telemetry records: {} (current schema, failed to parse)",
            report.unparsed
        ));
    }
```

### 4. Every `CostReport` literal gains the field

Exactly **9** sites in `mcp/src/costs.rs`: the 2 in `load_cost_report`
(§3) and **7 in tests** (near lines 501, 526, 985, 1011, 1061, 1076, 1096).
Add `unparsed: 0,` to each test literal. Confirm with
`grep -n "CostReport {" mcp/src/costs.rs` → 9 lines plus the struct itself.

**Gotcha:** the existing test helper `write_review_line` writes
`"verdict": "pass"`, which does **not** parse as `PhaseReview` (it needs
`architect_verdict`). After this phase, `read_all` counts that line as
unparsed. That is correct. Do **not** edit the helper or the existing tests
that use it.

**Must NOT:**

- Count lines whose `schema_version` is not current, or whose `record` is an
  unknown tag.
- Change `read`, `read_reviews`, `read_architect_activities` or
  `read_architect_ledger`.
- Touch the dashboard.
- Print anything from the executor library.

## Acceptance criteria

- [ ] A current-version ledger line missing `session_id` → `ledgers` 0, `unparsed` 1.
- [ ] The same line at `schema_version` 0 → `unparsed` 0.
- [ ] A non-JSON line → `unparsed` 1.
- [ ] A current-version review line missing `architect_verdict` → `unparsed` 1;
      a complete review line → `unparsed` 0.
- [ ] A current-version line with `record: "future_thing"` → `unparsed` 0.
- [ ] `format_costs` shows `Unreadable telemetry records: 3` when `unparsed` is 3,
      and no such line when it is 0.
- [ ] `cargo fmt --all --check`, `cargo build`,
      `cargo clippy --all-targets --all-features -- -D warnings`, `cargo test` pass.

## Test plan

In `executor/src/store/telemetry.rs` tests. Write each line with
`std::fs::write(dir.path().join("phase_runs.jsonl"), "<line>\n")` and call
`read_all` on it. Every name contains `read_all`:

1. `read_all_counts_current_version_line_that_fails_to_parse` —
   `{"record":"architect_ledger","schema_version":1,"model":"m","skill":"s","tokens":{"input":1,"cache_creation":0,"cache_read":0,"output":0},"messages":1,"last_ts":1}`
   (no `session_id`) → `ledgers.len() == 0`, `unparsed == 1`.
2. `read_all_does_not_count_old_schema_lines` — the same line with
   `"schema_version":0` → `unparsed == 0`.
3. `read_all_counts_malformed_json_line` — `not json` → `unparsed == 1`.
4. `read_all_counts_unparseable_review` —
   `{"record":"review","schema_version":1,"ts":1,"phase_id":"p"}` → `unparsed == 1`.
5. `read_all_does_not_count_valid_review` —
   `{"record":"review","schema_version":1,"ts":1,"phase_id":"p","architect_verdict":"approved_first_try"}`
   → `unparsed == 0`.
6. `read_all_does_not_count_unknown_record_types` —
   `{"record":"future_thing","schema_version":1}` → `unparsed == 0`.

In `mcp/src/costs.rs` tests, reuse an existing `CostReport` literal as the base:

7. `format_costs_reports_unparsed_records` — `unparsed: 3` → output contains
   `Unreadable telemetry records: 3`.
8. `format_costs_omits_unparsed_line_when_zero` — `unparsed: 0` → output does
   not contain `Unreadable telemetry records`.

Write test 1 first, run it, and quote the compile error or failure in the
Update Log before implementing.

**Finish condition:**

- `cargo test -p rexymcp-executor read_all` → **13 passed**
- `cargo test -p rexymcp costs::` → **35 passed**
- Full `cargo test` → **729 / 2 / 1218 passed**

Paste the `test result:` lines.

## End-to-end verification

Run `cargo test -p rexymcp-executor read_all -- --nocapture` and
`cargo test -p rexymcp costs:: -- --nocapture`; paste both in a
`### Update — YYYY-MM-DD HH:MM (end-to-end verification)` entry. The architect
runs `rexymcp costs` against the live store at review.

## Authorizations

- New field `unparsed` on `StoreRecords` and on `CostReport`.

## Out of scope

- The per-type readers, the dashboard, the scorecard.

## Update Log

### Update — 2026-09-18 17:58 (start)

Starting F12 phase-01: add `StoreRecords.unparsed` and count current-schema
lines that fail to parse in `read_all` (executor); add `CostReport.unparsed`
and print `Unreadable telemetry records: N` when nonzero in `rexymcp costs`
(mcp). Test-first per spec: write test 1 first, run it, quote the failure,
then implement.

### Update — 2026-09-18 17:58 (progress)

Test-first per spec: wrote `read_all_counts_current_version_line_that_fails_to_parse` before implementing the field. Quoted failure:

```
error[E0609]: no field `unparsed` on type `StoreRecords`
    --> executor/src/store/telemetry.rs:1840:28
     |
1840 |         assert_eq!(records.unparsed, 1, "current-version line failing to parse must be counted");
     |                            ^^^^^^^^ unknown field
     |
     = note: available fields are: `runs`, `activities`, `ledgers`
```

Implemented `StoreRecords.unparsed`, the `read_all` counting loop, and the 6 new `read_all_*` tests, then the `CostReport.unparsed` field, the single `read_all` call in `load_cost_report`, the optional `Unreadable telemetry records: N` line in `format_costs`, and the 2 new `format_costs_*unparsed*` tests. All 9 `CostReport` literals now carry `unparsed`.

### Update — 2026-09-18 17:58 (end-to-end verification)

`cargo test -p rexymcp-executor read_all -- --nocapture`:

```
test result: ok. 13 passed; 0 failed; 0 ignored; 0 measured; 1215 filtered out; finished in 0.00s
```

`cargo test -p rexymcp costs:: -- --nocapture`:

```
test result: ok. 35 passed; 0 failed; 0 ignored; 0 measured; 694 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 2 filtered out; finished in 0.00s
```

Full `cargo test` summary lines:

```
test result: ok. 729 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 6.24s
test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 1218 passed; 0 failed; 10 ignored; 0 measured; 0 filtered out; finished in 6.21s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
```

`cargo fmt --all --check`, `cargo build`, and `cargo clippy --all-targets --all-features -- -D warnings` all pass clean.

<!-- entries appended below this line -->

### Update — 2026-09-18 18:21 (complete, server-authored)

**Summary:** Committed. Working tree is clean.

**Summary + Notes for review**

Built F12 phase-01 as spec'd: added `StoreRecords.unparsed` to `executor/src/store/telemetry.rs` and rewrote the `read_all` loop to count non-JSON lines plus current-schema lines that fail to deserialize into their known record type (old-schema lines and unknown tags skipped, not counted; per-type readers untouched). Added `CostReport.unparsed` to `mcp/src/costs.rs`, switched `load_cost_report` to a single `read_all` call, and `format_costs` now prints `Unreadable telemetry records: N (current schema, failed to parse)` only when `N > 0`. Wrote test 1 first per spec and captured the `E0609` compile failure in the Update Log.

Verification: `cargo test -p rexymcp-executor read_all -- --nocapture` → `test result: ok. 13 passed; 0 failed`; `cargo test -p rexymcp costs:: -- --nocapture` → `test result: ok. 35 passed; 0 failed`; full `cargo test` → `test result: ok. 729 passed; 0 failed; 0 ignored`, `ok. 2 passed`, `ok. 1218 passed; 0 failed; 10 ignored`, `ok. 0 passed` — matching the pinned finish condition exactly. `cargo fmt --all --check`, `cargo build`, and `cargo clippy --all-targets --all-features -- -D warnings` all pass clean. All 9 `CostReport` literals (2 in `load_cost_report`, 7 in tests) carry `unparsed: 0`.

Notes: the spec's §3 sketch shows the new loop adding a `REVIEW_RECORD_TAG` arm and the doc-comment rewording exactly as shipped — no divergence found. The existing `write_review_line`-style test helper issue mentioned in the spec's gotcha does not manifest in the current `costs.rs` test suite (no `write_review_line` helper exists there), so no action was needed and no existing tests were edited. One formatting detail: `cargo fmt --all` reformatted several new `assert_eq!` lines to multi-line form before the final check; the committed tree is `fmt --check` clean.

**Executor:** RedHatAI/Qwen3.8-27B-INT4

**Gates:** format=run, build=run, lint=run, test=run

**Command output tails:**

```
FORMAT


BUILD
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.11s


LINT
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.14s


TEST
ts_path_outside_root ... ok
test tools::symbols::tests::references_finds_call_sites ... ok
test tools::symbols::tests::references_no_matches_advisory ... ok
test tools::symbols::tests::single_file_unsupported_extension_advisory_error ... ok
test tools::symbols::tests::references_respects_max_results ... ok
test tools::symbols::tests::type_mismatch_returns_recovery_hint ... ok
test tools::update_task::tests::flips_active_task_to_done ... ok
test tools::update_task::tests::flips_pending_task_to_active ... ok
test tools::update_task::tests::invalid_args_hint_lists_incomplete_ids ... ok
test tools::update_task::tests::invalid_args_hint_reports_all_complete ... ok
test tools::update_task::tests::invalid_state_returns_advisory_error ... ok
test tools::update_task::tests::malformed_args_returns_advisory_error ... ok
test tools::update_task::tests::metadata_shape_is_unchanged ... ok
test tools::update_task::tests::null_args_returns_recovery_hint ... ok
test tools::symbols::tests::metadata_carries_definitions_and_files_count ... ok
test tools::update_task::tests::result_flags_redundant_remark ... ok
test tools::update_task::tests::result_lists_remaining_incomplete_ids ... ok
test tools::update_task::tests::result_reports_all_complete_when_last_done ... ok
test tools::update_task::tests::success_output_names_task ... ok
test tools::update_task::tests::unknown_id_returns_advisory_error ... ok
test tools::write_file::tests::append_creates_file_if_missing ... ok
test tools::write_file::tests::append_false_overwrites ... ok
test tools::write_file::tests::appends_to_existing_file ... ok
test tools::write_file::tests::creates_new_file ... ok
test tools::write_file::tests::missing_path_returns_recovery_hint ... ok
test tools::write_file::tests::non_object_args_do_not_panic ... ok
test tools::write_file::tests::overwrites_existing_file ... ok
test tools::write_file::tests::rejects_malformed_args ... ok
test tools::symbols::tests::references_single_file_path ... ok
test tools::write_file::tests::reports_missing_parent_dir ... ok
test tools::write_file::tests::scope_escape_returns_advisory_error_and_writes_nothing ... ok
test tools::write_file::tests::success_output_includes_line_count ... ok
test tools::symbols::tests::references_snippet_shows_source_line ... ok
test tools::symbols::tests::references_across_multiple_files ... ok
test tools::symbols::tests::references_truncation_note_omits_kind_filter ... ok
test tools::symbols::tests::reports_line_and_column ... ok
test tools::symbols::tests::unsupported_extension_skipped_in_dir_walk ... ok
test tools::symbols::tests::finds_rust_struct_and_trait ... ok
test tools::symbols::tests::respects_gitignore ... ok
test tools::bash::tests::cargo_command_output_is_filtered_through_cargo_filter ... ok
test store::telemetry::tests::append_is_atomic_under_concurrent_appenders ... ok
test ai::backends::openai::tests::first_token_stall_retries_then_succeeds ... ok
test ai::backends::openai::tests::midstream_stall_is_not_retried ... ok
test ai::tests::stream_next_uses_supplied_timeout ... ok
test tools::bash::tests::arg_timeout_overrides_constructor_default ... ok
test tools::bash::tests::default_timeout_used_when_arg_absent ... ok
test tools::bash::tests::times_out_advisory_failure ... ok
test ai::backends::openai::tests::first_token_stall_exhausts_retries_then_errors ... ok
test health::tests::check_returns_unreachable_on_connection_error ... ok

test result: ok. 1218 passed; 0 failed; 10 ignored; 0 measured; 0 filtered out; finished in 6.20s


running 0 tests

test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

    Finished `test` profile [unoptimized + debuginfo] target(s) in 0.14s
     Running unittests src/main.rs (target/debug/deps/rexymcp-3de3446ef9b6a3ce)
     Running tests/readme_config_reference.rs (target/debug/deps/readme_config_reference-22f887757c17cb2b)
     Running unittests src/lib.rs (target/debug/deps/executor-0c52bf72536f9e8e)
   Doc-tests executor

```

**Files changed:**

- `docs/dev/milestones/F12-telemetry-unparsed-count/README.md` — +1 -1
- `docs/dev/milestones/F12-telemetry-unparsed-count/phase-01-count-unparsed.md` — +51 -1
- `executor/src/store/telemetry.rs` — +110 -18
- `mcp/src/costs.rs` — +57 -8

**Commit:** a73e8c252b01271be2318a0e1bd19b993bacaaf3

**Notes:** server-authored completion entry (executor no longer owns the bookkeeping tail; see M27 phase-03).

### Review verdict — 2026-09-18

- **Verdict:** approved_first_try
- **Bounces:** none
- **Executor:** RedHatAI/Qwen3.8-27B-INT4 (local), 78 turns
- **Scope deviations:** none (`a73e8c2`). No test removed.
- **Verification:** gates 729 / 2 / 1218. `rexymcp costs` on the live store prints no unreadable line; on a copy with one field-less ledger line and one non-JSON line it prints `Unreadable telemetry records: 2 (current schema, failed to parse)`. Mutation: dropping the ledger arm's count fails `read_all_counts_current_version_line_that_fails_to_parse`.
- **Calibration:** first dispatch on the F11 contract — started, progress and `(end-to-end verification)` entries all present, no model named, pasted `test result:` lines. The F11 fold held on its first live run. Two nits: the entries were placed above the `<!-- entries appended below this line -->` marker, and the Summary misplaced the `write_review_line` gotcha in `costs.rs` (it is in `telemetry.rs`; nothing touched it).
