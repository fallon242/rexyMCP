# Phase 04: `calibrate-governor` — deterministic row order + k/M byte columns

**Milestone:** M37 — Governor Read-Only Calibration
**Status:** review
**Depends on:** phase-03 (reuses `metrics::fmt_tokens`) — sequencing only; 03 is done
**Estimated diff:** ~90 lines
**Tags:** language=rust, kind=feature, size=s

## Goal

Two fixes to `calibrate-governor`'s text report, both in the pure
`format_report` function:

1. **Deterministic row order.** Rows are pushed in `HashMap` iteration order, so
   the report reorders run-to-run. This actively obstructed the M37 phase-01
   review: the before/after `calibrate-governor` diff was 100+ lines of pure
   reordering noise around ~24 real changes, readable only after sorting both
   sides by hand. M37's own README asks reviewers to diff this output before and
   after a terminator change, so the instability breaks the workflow it serves.

2. **k/M-compact the byte columns.** The `output_flood_windowed_bytes` signal's
   P50/P90/P99 are **byte** sums that reach five and six digits (`22035`,
   `61128`, `71740` in the live report), rendered as raw integers. Compact them
   through the shared `metrics::fmt_tokens` — the formatter phase-03 just
   consolidated — so they read `22.0k`, `61.1k`. Every other signal's
   percentiles are small counts (turns, distinct calls) and stay raw.

## Architecture references

Read before starting:

- `docs/architecture.md` § Status #37 — this milestone; the row-order finding was
  recorded at the phase-01 review as folding into this phase.
- `docs/dev/milestones/M37-governor-read-only-calibration/phase-03-token-formatter-consolidation.md`
  — `metrics::fmt_tokens`, the shared decimal-SI-with-M formatter this phase
  reuses. Its `0 → "—"` sentinel is intentional here (see § Spec Task 2).

## Pre-flight

1. Read `docs/dev/STANDARDS.md` top to bottom.
2. Read the architecture references above.
3. Read this entire phase doc before touching any code.
4. Confirm the repo is on a clean branch with no uncommitted changes.

## Current state

**`mcp/src/calibrate_governor.rs:218` `format_report(rows: &[ReportRow]) -> String`**
— the pure function both fixes live in. It already imports from the shared
module (`use rexymcp_executor::store::metrics::percentile;` at line 10), so
adding `fmt_tokens` is a one-token change to that `use`.

**`ReportRow`** (`:204-215`) carries `signal: String`, `model: String`,
`outcome: String`, and `p_mid`/`p_near`/`p_far: usize`.

**Defect 1 — the row emission** (`:233-259`). For each signal, rows are
filtered out of `rows` and pushed in whatever order they were built, which is
`HashMap` order (built at `:337` and `:374`, both `for … in <HashMap>`):

```rust
    for signal in signals {
        let signal_rows: Vec<_> = rows.iter().filter(|r| r.signal == *signal).collect();
        ...
        for row in signal_rows {
            lines.push(format!(
                "{:<8} {:<10} {:>4}  {:>4}  {:>4}  {:>4}  {:>4}",
                row.model, row.outcome, row.runs, row.n, row.p_mid, row.p_near, row.p_far
            ));
        }
    }
```

Signal *grouping* is already deterministic (the outer `signals` list is fixed
order). Only the within-signal `model`/`outcome` order is unstable — so the fix
is localized to sorting `signal_rows`.

**Defect 2 — the byte columns.** The same `format!` renders `p_mid/p_near/p_far`
with `{:>4}` for **every** signal. Only `output_flood_windowed_bytes` needs
compaction; the label to match on is exactly that string (`:68`, `:231`).

## Spec

### 1. Sort each signal's rows into a stable order

In `format_report`, make `signal_rows` mutable and sort it before rendering.
Canonical order: **the `(all)` summary row first, then by `model` ascending, then
by `outcome` ascending** — so the global summary leads each signal block and the
per-model rows are alphabetical.

```rust
        let mut signal_rows: Vec<_> = rows.iter().filter(|r| r.signal == *signal).collect();
        signal_rows.sort_by(|a, b| {
            // "(all)" summary first, then model asc, then outcome asc.
            let a_all = a.model != "(all)"; // false (0) sorts before true (1)
            let b_all = b.model != "(all)";
            a_all
                .cmp(&b_all)
                .then_with(|| a.model.cmp(&b.model))
                .then_with(|| a.outcome.cmp(&b.outcome))
        });
```

Do **not** change how rows are *built* (the `HashMap` accumulation at `:317`,
`:337`, `:374` is fine — order only matters at render). Sorting at render keeps
the fix in one place.

### 2. Compact the byte columns via `metrics::fmt_tokens`

Add `fmt_tokens` to the existing `metrics` import, and in the row `format!`,
render the three percentile cells through it **only for the byte signal**:

```rust
        let is_bytes = *signal == "output_flood_windowed_bytes";
        let cell = |v: usize| -> String {
            if is_bytes {
                metrics::fmt_tokens(v as u64)
            } else {
                v.to_string()
            }
        };
        for row in signal_rows {
            lines.push(format!(
                "{:<8} {:<10} {:>4}  {:>4}  {:>6}  {:>6}  {:>6}",
                row.model, row.outcome, row.runs, row.n,
                cell(row.p_mid), cell(row.p_near), cell(row.p_far)
            ));
        }
```

Two details, both intentional — pin them so they are not "fixed" later:

- **`0 → "—"`.** `fmt_tokens(0)` renders the `—` sentinel. A flood percentile of
  0 is common (the live report shows `P50 = 0` for `complete` runs) and `—`
  reads correctly as "negligible flood at this percentile." This is the intended
  rendering; do **not** special-case bytes to render `0`.
- **Column width `{:>6}`** on the three percentile cells (was `{:>4}`) so a
  compacted `22.0k`/`61.1k` fits without pushing the next column. Small raw
  counts right-align in the wider field unchanged. Widen the three data columns
  only; the loose `col_header` string (`:247-249`) does not need to match widths
  exactly — it never did.

## Acceptance criteria

- [ ] `cargo build` is green.
- [ ] `cargo clippy --all-targets --all-features -- -D warnings` is clean.
- [ ] `cargo fmt --all --check` reports no diff in the files this phase touched.
- [ ] `cargo test` passes.
- [ ] `format_report` produces the same output for the same input **regardless of
      the input `rows` order** — pinned by the shuffle test below.
- [ ] The `output_flood_windowed_bytes` block renders percentile cells via
      `metrics::fmt_tokens` (e.g. `22035 → "22.0k"`); every other signal's
      percentiles render raw.
- [ ] Two `calibrate-governor` runs over the same corpus produce **byte-identical**
      text output.

## Test plan

`format_report` is pure over `&[ReportRow]` — test it directly. The module
already has `format_report`-based tests (e.g.
`format_report_labels_oscillation_tail_low` near `:843`); reuse that fixture
shape.

- `format_report_row_order_is_deterministic` — build a `Vec<ReportRow>` for one
  signal with several `(model, outcome)` rows **in scrambled order** (e.g.
  `zeta`, `(all)`, `alpha`), call `format_report`, and assert the rendered rows
  appear in canonical order: `(all)` first, then `alpha`, then `zeta`. This is
  the load-bearing test — it fails if the sort is dropped or the key is wrong.
- `format_report_byte_signal_is_k_compacted` — a `ReportRow` with
  `signal = "output_flood_windowed_bytes"` and `p_near = 22035`; assert the
  output contains `22.0k` and does **not** contain the raw `22035`.
- `format_report_non_byte_signal_stays_raw` — a `ReportRow` with a non-byte
  signal (e.g. `identical_run`) and `p_near = 22035`; assert the output contains
  `22035` and not `22.0k`. (Negative case — pins that compaction is scoped to the
  byte signal, not applied globally.)
- `format_report_byte_zero_renders_dash` — a byte-signal row with `p_mid = 0`;
  assert the cell renders `—`, not `0`. Pins the intentional sentinel so a later
  edit does not "fix" it.

Pin **cell content** (`contains("22.0k")`), not column widths or byte-exact
lines — per WORKFLOW § "Specs pin behavior, not rendering".

## End-to-end verification

The payoff is a stable diff. Run the real binary twice and diff:

```bash
cargo run -p rexymcp -- calibrate-governor --repo . > /tmp/cg-a.txt 2>&1
cargo run -p rexymcp -- calibrate-governor --repo . > /tmp/cg-b.txt 2>&1
diff /tmp/cg-a.txt /tmp/cg-b.txt && echo "STABLE ✓"
```

Paste the result in the completion Update Log — `diff` must be **empty**
(`STABLE ✓`). Before this phase, two runs differ by 100+ reordered lines; after,
they are identical. Also paste the `signal: output_flood_windowed_bytes` block
showing the compacted `k`/`M` percentile cells.

## Authorizations

None. No new dependencies. No edits to `docs/architecture.md`.

## Out of scope

- Changing which signals `calibrate-governor` reports, how percentiles are
  computed, or the replay path. **The distributions must not move** — this phase
  changes only row *order* and byte *rendering*, not any value.
- The `--json` output (`:411`) — it serializes `rows` structurally; order there
  is the consumer's to sort, and byte values stay numeric in JSON. Leave it.
- Adding a second count formatter. Reuse `metrics::fmt_tokens`; re-fragmenting
  what phase-03 consolidated is the opposite of this milestone's intent.
- The other M37 phases (all now done except this one).

## Update Log

(Filled in by the executor. See WORKFLOW.md § "Update Log entries".)

<!-- entries appended below this line -->

### Update — 2026-07-24 16:39 (started)

**Executor:** Claude (Sonnet 4.5)

Implementing deterministic row sort and k/M byte compaction in `format_report`.
### Update — ts=1784911648765 (complete, server-authored)

**Summary:** All verification gates passed. Summary and notes for review:

**What was built:** Two fixes in `format_report` — (1) each signal's rows are now sorted deterministically: `(all)` summary first, then by model ascending, then outcome ascending; (2) `output_flood_windowed_bytes` percentile cells are compacted through `metrics::fmt_tokens` (e.g. `22035 → "22.0k"`), with `0` rendering as `—`. All other signals render raw integers.

**Tests:** 4 new unit tests cover deterministic ordering, k-compaction for the byte signal, raw rendering for non-byte signals, and the dash sentinel for zero. All 1045 tests pass.

**E2E verification:** Two runs of `calibrate-governor` produce byte-identical output (`diff` is empty, `STABLE ✓`). The live `output_flood_windowed_bytes` block shows compacted values like `29.6k`, `71.7k`, `24.2k`, `61.1k`.

**No deviations from spec.** The implementation matches the spec exactly — sort key, `fmt_tokens` reuse, column widths widened to `{:>6}`, and the `0 → "—"` sentinel are all as specified.

**Executor:** Qwen/Qwen3.6-27B-FP8

**Gates:** format=run, build=run, lint=run, test=run

**Command output tails:**

```
FORMAT


BUILD
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.07s


LINT
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.07s


TEST
 ok
test tools::update_task::tests::invalid_args_hint_reports_all_complete ... ok
test tools::update_task::tests::invalid_state_returns_advisory_error ... ok
test tools::update_task::tests::malformed_args_returns_advisory_error ... ok
test tools::update_task::tests::metadata_shape_is_unchanged ... ok
test tools::symbols::tests::exact_match_no_substring ... ok
test tools::update_task::tests::null_args_returns_recovery_hint ... ok
test tools::symbols::tests::no_symbols_returns_advisory_error ... ok
test tools::update_task::tests::result_flags_redundant_remark ... ok
test tools::update_task::tests::result_lists_remaining_incomplete_ids ... ok
test tools::update_task::tests::success_output_names_task ... ok
test tools::update_task::tests::result_reports_all_complete_when_last_done ... ok
test tools::symbols::tests::references_respects_max_results ... ok
test tools::update_task::tests::unknown_id_returns_advisory_error ... ok
test tools::write_file::tests::append_creates_file_if_missing ... ok
test tools::write_file::tests::append_false_overwrites ... ok
test tools::write_file::tests::creates_new_file ... ok
test tools::write_file::tests::missing_path_returns_recovery_hint ... ok
test tools::write_file::tests::appends_to_existing_file ... ok
test tools::write_file::tests::non_object_args_do_not_panic ... ok
test tools::write_file::tests::overwrites_existing_file ... ok
test tools::write_file::tests::rejects_malformed_args ... ok
test tools::write_file::tests::reports_missing_parent_dir ... ok
test tools::write_file::tests::scope_escape_returns_advisory_error_and_writes_nothing ... ok
test tools::write_file::tests::success_output_includes_line_count ... ok
test tools::symbols::tests::kind_filter_returns_only_matching_kind ... ok
test tools::symbols::tests::finds_python_function_and_class ... ok
test tools::symbols::tests::references_snippet_shows_source_line ... ok
test tools::symbols::tests::references_across_multiple_files ... ok
test tools::symbols::tests::references_truncation_note_omits_kind_filter ... ok
test tools::symbols::tests::metadata_carries_definitions_and_files_count ... ok
test tools::symbols::tests::unsupported_extension_skipped_in_dir_walk ... ok
test tools::symbols::tests::reports_line_and_column ... ok
test ai::backends::openai::tests::is_retriable_transport_true_for_reqwest_error ... ok
test tools::symbols::tests::respects_gitignore ... ok
test tools::bash::tests::cargo_command_records_cargo_filter_label ... ok
test tools::symbols::tests::finds_rust_struct_and_trait ... ok
test governor::verifier::tests::verify_rust_returns_checked_empty_on_clean_code ... ok
test governor::verifier::tests::capture_baseline_dedupes_by_project_root ... ok
test governor::verifier::tests::verify_rust_returns_checked_with_errors_on_broken_code ... ok
test governor::verifier::tests::capture_baseline_skips_unsupported_files ... ok
test tools::bash::tests::cargo_command_output_is_filtered_through_cargo_filter ... ok
test ai::backends::openai::tests::midstream_stall_is_not_retried ... ok
test ai::backends::openai::tests::first_token_stall_retries_then_succeeds ... ok
test ai::tests::stream_next_uses_supplied_timeout ... ok
test tools::bash::tests::arg_timeout_overrides_constructor_default ... ok
test tools::bash::tests::default_timeout_used_when_arg_absent ... ok
test tools::bash::tests::times_out_advisory_failure ... ok
test ai::backends::openai::tests::first_token_stall_exhausts_retries_then_errors ... ok
test health::tests::check_returns_unreachable_on_connection_error ... ok

test result: ok. 1045 passed; 0 failed; 2 ignored; 0 measured; 0 filtered out; finished in 6.09s


running 0 tests

test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

    Finished `test` profile [unoptimized + debuginfo] target(s) in 0.07s
     Running unittests src/main.rs (target/debug/deps/rexymcp-4e85b51f198fbe9f)
     Running unittests src/lib.rs (target/debug/deps/executor-c1650299697d7408)
   Doc-tests executor

```

**Files changed:**
- `docs/dev/milestones/M37-governor-read-only-calibration/README.md` — +1 -1
- `docs/dev/milestones/M37-governor-read-only-calibration/phase-04-calibrate-governor-render.md` — +7 -1
- `mcp/src/calibrate_governor.rs` — +145 -4

**Commit:** b28d350336eca97bba4090ed209162b645b3e480

**Notes:** server-authored completion entry (executor no longer owns the bookkeeping tail; see M27 phase-03).

