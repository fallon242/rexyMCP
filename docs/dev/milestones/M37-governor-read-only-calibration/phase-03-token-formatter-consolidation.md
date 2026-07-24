# Phase 03: Consolidate the token formatters into `metrics::fmt_tokens`

**Milestone:** M37 — Governor Read-Only Calibration
**Status:** review
**Depends on:** none (independent of 01/02)
**Estimated diff:** ~130 lines
**Tags:** language=rust, kind=refactor, size=m

## Goal — and the decision this phase makes

There are **four** hand-rolled token/count k-formatters in `mcp/`, and they
**disagree** with each other. Consolidating them is not a mechanical merge: it
forces a choice of one canonical format, and that choice **changes the rendered
output of the `runs` and `scorecard` tables**. This phase makes that call
explicitly.

**Decision (architect, pin this — do not re-litigate in code):** the canonical
format is **decimal SI with a thousands *and* millions tier**, one decimal
place — i.e. exactly what `costs::format_tokens` already does:

```
0            → "—"
1..=999      → "123"        (raw)
1_000..      → "12.3k"      (decimal thousands, 1 dp)
1_000_000..  → "2.1M"       (decimal millions, 1 dp)
```

Why decimal, not binary (1024): tokens are a decimal quantity, not bytes;
`12288` tokens is `12.3k`, not `12k`. Why a millions tier: without it, a 2.1M-
token run renders `2100.0k`. `costs` (the newest surface, M38) already uses this
shape; the older `runs`/`scorecard` tables use a binary-1024 format with no
millions tier, which is the thing being corrected.

**This is a deliberate, user-visible output change to `runs` and `scorecard`**,
not an incidental one. The pinned tests that assert the *old* binary output must
be updated to the new decimal output — **updating those assertions is the fix;
reverting the formatter to satisfy them is the bug.** (WORKFLOW § "Green bounces"
/ § "Derive every spec fact" — a test asserting old behavior after a deliberate
behavior change is updated, not obeyed.)

## Architecture references

Read before starting:

- `docs/architecture.md` § Status #37 — this milestone; the token-formatter
  consolidation is one of its carried-debt exit criteria.
- `docs/architecture.md` § Status #35 — where `store::metrics` was established as
  the shared home for derived-number helpers (`reclaimed_total`,
  `tokens_per_sec`, `settings_label`, `token_cost`, `percentile`).

## Pre-flight

1. Read `docs/dev/STANDARDS.md` top to bottom.
2. Read the architecture references above.
3. Read this entire phase doc before touching any code.
4. Confirm the repo is on a clean branch with no uncommitted changes.

## Current state

**The shared home already exists.** `executor/src/store/metrics.rs` holds the
derived-number helpers, several returning `String` (`settings_label:30`,
`run_id:54`). `mcp/` imports it as `use rexymcp_executor::store::metrics;`. The
new formatter goes here.

**The four divergent formatters:**

1. **`mcp/src/costs.rs:369` `format_tokens(count: u64)`** — the canonical
   behavior already. `0→"—"`, `>=1_000_000→"{:.1}M"`, `>=1_000→"{:.1}k"`, else
   raw. Callers: `costs.rs:354,454-456,461-463` (7 sites, all `u64`).

```rust
pub(crate) fn format_tokens(count: u64) -> String {
    if count == 0 { "—".to_string() }
    else if count >= 1_000_000 { format!("{:.1}M", count as f64 / 1_000_000.0) }
    else if count >= 1_000 { format!("{:.1}k", count as f64 / 1_000.0) }
    else { count.to_string() }
}
```

2. **`mcp/src/runs.rs:58` `fmt_tokens(total: u32)`** — binary, no M tier.
   `0→"—"`, `>=1024→"{}k"`, else raw. Callers: `runs.rs:269` (TOKENS column),
   `profile_cli.rs:148` (imports it via `use crate::runs::{fmt_cost, fmt_tokens}`).

3. **`mcp/src/scorecard_cli.rs:86-90` inline** (on an `Option<f64>` mean) — binary.
   `None→"—"`, `Some(v) if v>=1024.0 → "{:.0}k"`, else `"{:.0}"`. Renders the
   RECLAIMED column.

4. **`mcp/src/runs.rs:261-266` inline** (on a `usize` reclaimed total) — binary.
   `0→"—"`, `>=1024→"{}k"`, else raw. Renders the RECLAIMED column of `runs`.

**Not in scope — a raw counter, not a k-formatter:** `runs.rs:151`
(`reclaimed.to_string()` in the single-run detail view) renders the precise
value with no k/M compaction. A detail view showing the exact number while the
table shows `12.3k` is correct; leave it.

**The pinned test that changes:** `runs.rs` test around line 690 builds a run
with reclaimed `8000+3000+1000+288 = 12288` and asserts
`qwen_line.contains("12k")`. Under the canonical format `12288 → "12.3k"`. This
assertion becomes `contains("12.3k")`. Grep the test modules for other `"…k"` /
`"…M"` string assertions on token/reclaimed cells and update each to the decimal
rendering — do not assume this is the only one.

## Spec

### 1. Add `fmt_tokens` to `executor/src/store/metrics.rs`

```rust
/// Compact human token/count rendering: decimal SI with thousands and millions
/// tiers, one decimal place. Zero renders as the `—` sentinel.
///
/// `0 → "—"`, `1..=999 → "123"`, `1_000.. → "12.3k"`, `1_000_000.. → "2.1M"`.
/// Decimal (1000), not binary (1024): tokens are a decimal quantity. This is the
/// single formatter for token/reclaimed cells across `costs`, `runs`,
/// `scorecard`, and `profile`.
pub fn fmt_tokens(count: u64) -> String {
    if count == 0 {
        "—".to_string()
    } else if count >= 1_000_000 {
        format!("{:.1}M", count as f64 / 1_000_000.0)
    } else if count >= 1_000 {
        format!("{:.1}k", count as f64 / 1_000.0)
    } else {
        count.to_string()
    }
}
```

### 2. Migrate `costs.rs` and delete its `format_tokens`

Replace the 7 `format_tokens(...)` calls with `metrics::fmt_tokens(...)` (add
the import if not present) and delete the private `costs::format_tokens`. **No
output change** — the behavior is identical; this is the reference the others
converge on. Verify `costs`' existing tests still pass **unchanged**.

### 3. Migrate `runs.rs` — the named fn and the inline reclaimed block

- Replace `runs.rs:269` `fmt_tokens(run.tokens.total())` with
  `metrics::fmt_tokens(run.tokens.total() as u64)` (`total()` returns `u32` —
  `ai/types.rs:56` — so cast up).
- Replace the inline reclaimed block (`runs.rs:261-266`) with
  `metrics::fmt_tokens(reclaimed_total as u64)` (`reclaimed_total` is `usize`
  from `metrics::reclaimed_total`).
- Delete the private `runs::fmt_tokens`.

### 4. Migrate `profile_cli.rs`

`profile_cli.rs:9` imports `fmt_tokens` from `crate::runs`; `:148` calls it.
Point both at `metrics::fmt_tokens` (cast the arg to `u64`). Leave `fmt_cost` —
it is a different helper and out of scope.

### 5. Migrate `scorecard_cli.rs` — the inline `Option<f64>` mean

Replace the inline block (`scorecard_cli.rs:86-90`) with:

```rust
        let reclaimed = match row.tokens_reclaimed_mean {
            None => "—".to_string(),
            Some(v) => metrics::fmt_tokens(v.round() as u64),
        };
```

Note the semantic detail: the old inline rendered `Some(0.0)` as `"0"` (its else
branch), whereas `fmt_tokens(0)` renders `"—"`. This unifies `Some(0.0)` and
`None` to `—`, which is the correct behavior — a zero mean *is* "nothing
reclaimed". If a scorecard test asserts `"0"` for a zero reclaimed mean, update
it to `"—"` and note it in the Update Log.

### 6. Update the pinned tests

Per § Current state: `runs.rs` `contains("12k")` → `contains("12.3k")`, plus any
other `"…k"`/`"…M"` token-cell assertions the grep in § Test plan surfaces.

## Acceptance criteria

- [ ] `cargo build` is green.
- [ ] `cargo clippy --all-targets --all-features -- -D warnings` is clean.
- [ ] `cargo fmt --all --check` reports no diff in the files this phase touched.
- [ ] `cargo test` passes.
- [ ] `grep -rn "fn format_tokens\|fn fmt_tokens" mcp/src` returns **no**
      matches — both private fns are gone.
- [ ] `metrics::fmt_tokens` is the only token k/M formatter: `grep -rn
      '/ 1024\|/ 1_000\|1_000_000\|>= 1024' mcp/src` shows no k/M formatting
      logic left in `runs.rs`/`scorecard_cli.rs`/`costs.rs` (matches inside
      `store/metrics.rs` and unrelated arithmetic are fine — inspect, don't just
      count).
- [ ] `rexymcp costs` output is **unchanged** from before this phase.
- [ ] `rexymcp runs` and `rexymcp scorecard` render reclaimed/token cells in the
      new decimal format (`12.3k`, `2.1M`), columns still aligned.

## Test plan

- **Unit tests for `metrics::fmt_tokens`** in `store/metrics.rs`'s test module —
  pin the tier boundaries and the sentinel, as exact-equality assertions:
  - `fmt_tokens_zero_is_dash` — `fmt_tokens(0) == "—"`.
  - `fmt_tokens_raw_below_thousand` — `fmt_tokens(999) == "999"`.
  - `fmt_tokens_thousands_one_decimal` — `fmt_tokens(12_288) == "12.3k"` (the
    exact value the `runs` test exercises, so the two agree by construction).
  - `fmt_tokens_millions_tier` — `fmt_tokens(2_100_000) == "2.1M"`.
  - `fmt_tokens_boundary_at_thousand` — `fmt_tokens(1_000) == "1.0k"` and
    `fmt_tokens(999_999)` is a `k` value, `fmt_tokens(1_000_000) == "1.0M"`.
    (Negative-ish: pins that the tier switches at the right edge.)
- **Update existing assertions**, do not add parallel ones: run `grep -rn
  'contains("[0-9].*[kM]")' mcp/src` (and the reclaimed/token test bodies) and
  change each old-format literal to its decimal rendering. Every migrated caller
  already has table-render tests; those are the coverage — do not write new
  near-duplicates.

Do not pin column *width* or byte-exact table lines; pin the **cell content**
(`contains("12.3k")`), per WORKFLOW § "Specs pin behavior, not rendering".

## End-to-end verification

All three surfaces, against the real binary — the point is that the rendered
cells changed for `runs`/`scorecard` but not for `costs`:

```bash
cargo run -p rexymcp -- costs --config rexymcp.toml --repo . | head -8
cargo run -p rexymcp -- runs --config rexymcp.toml | head -6
cargo run -p rexymcp -- scorecard --config rexymcp.toml | head -6
```

Paste all three in the completion Update Log. Expected: `costs` **byte-identical**
to before; `runs` and `scorecard` show `k`/`M` cells in the new decimal format
with columns still aligned (no wrapping from the extra decimal digit). If any
column wraps, that is a real regression — report it, do not hand-pad.

## Authorizations

- [x] May edit `executor/src/store/metrics.rs` — adding the shared helper is the
      point of the phase. This is the executor crate, but the file is the
      established shared-metrics home and the change is additive (one `pub fn` +
      its tests).

No new dependencies. No edits to `docs/architecture.md`.

## Out of scope

- `runs.rs:151` single-run-detail raw reclaimed (not a k/M formatter).
- `fmt_cost`, `fmt_tok_per_sec`, and any non-token formatter — different helpers,
  different phase if ever.
- Changing the canonical format after the fact, or making it configurable.
- The other M37 phases (04 calibrate-governor rendering, 05 completion writer).

## Update Log

(Filled in by the executor. See WORKFLOW.md § "Update Log entries".)

<!-- entries appended below this line -->

### Update — 2026-07-24 13:55 (progress)

Started phase. Added `fmt_tokens` to `executor/src/store/metrics.rs` with unit tests. Migrated all four callers (`costs.rs`, `runs.rs`, `profile_cli.rs`, `scorecard_cli.rs`) to the shared formatter and deleted the private `format_tokens`/`fmt_tokens` functions. Updated the pinned `runs.rs` test assertion from `"12k"` to `"12.3k"`. All verification commands pass.
### Update — ts=1784902352976 (complete, server-authored)

**Summary:** **Summary:** Consolidated four divergent token/count k-formatters across `mcp/` into a single `pub fn fmt_tokens(count: u64)` in `executor/src/store/metrics.rs`, using decimal SI (1000-based) with thousands and millions tiers at one decimal place. Migrated all callers in `costs.rs` (7 sites), `runs.rs` (named fn + inline reclaimed block), `profile_cli.rs`, and `scorecard_cli.rs` (inline `Option<f64>` mean), deleting the private `format_tokens` and `fmt_tokens` functions. Updated the pinned `runs.rs` test assertion from `"12k"` to `"12.3k"`. The `context_window` inline formatter in `runs.rs` was left as-is since it formats window sizes (a different domain) and was not in scope. All verification commands pass clean: `cargo fmt --all --check`, `cargo build`, `cargo clippy --all-targets --all-features -- -D warnings`, `cargo test` (1045 passed). `grep -rn "fn format_tokens\|fn fmt_tokens" mcp/src` returns no matches — both private formatters are gone.

**Gates:** format=run, build=run, lint=run, test=run

**Command output tails:**

```
FORMAT


BUILD
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.07s


LINT
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.08s


TEST
st tools::update_task::tests::flips_pending_task_to_active ... ok
test tools::update_task::tests::invalid_args_hint_lists_incomplete_ids ... ok
test tools::update_task::tests::invalid_args_hint_reports_all_complete ... ok
test tools::update_task::tests::invalid_state_returns_advisory_error ... ok
test tools::update_task::tests::result_flags_redundant_remark ... ok
test tools::update_task::tests::malformed_args_returns_advisory_error ... ok
test tools::update_task::tests::null_args_returns_recovery_hint ... ok
test tools::update_task::tests::metadata_shape_is_unchanged ... ok
test tools::update_task::tests::result_lists_remaining_incomplete_ids ... ok
test tools::update_task::tests::result_reports_all_complete_when_last_done ... ok
test tools::update_task::tests::unknown_id_returns_advisory_error ... ok
test tools::update_task::tests::success_output_names_task ... ok
test tools::symbols::tests::finds_rust_function_by_name ... ok
test tools::write_file::tests::append_false_overwrites ... ok
test tools::write_file::tests::creates_new_file ... ok
test tools::write_file::tests::append_creates_file_if_missing ... ok
test tools::write_file::tests::appends_to_existing_file ... ok
test tools::write_file::tests::missing_path_returns_recovery_hint ... ok
test tools::write_file::tests::non_object_args_do_not_panic ... ok
test tools::write_file::tests::overwrites_existing_file ... ok
test tools::write_file::tests::rejects_malformed_args ... ok
test tools::write_file::tests::scope_escape_returns_advisory_error_and_writes_nothing ... ok
test tools::write_file::tests::reports_missing_parent_dir ... ok
test tools::write_file::tests::success_output_includes_line_count ... ok
test tools::symbols::tests::references_respects_max_results ... ok
test tools::symbols::tests::finds_python_function_and_class ... ok
test tools::symbols::tests::references_snippet_shows_source_line ... ok
test ai::backends::openai::tests::is_retriable_transport_true_for_reqwest_error ... ok
test tools::symbols::tests::references_truncation_note_omits_kind_filter ... ok
test tools::symbols::tests::references_across_multiple_files ... ok
test tools::symbols::tests::metadata_carries_definitions_and_files_count ... ok
test tools::symbols::tests::unsupported_extension_skipped_in_dir_walk ... ok
test tools::bash::tests::cargo_command_records_cargo_filter_label ... ok
test tools::symbols::tests::respects_gitignore ... ok
test tools::symbols::tests::reports_line_and_column ... ok
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
test tools::bash::tests::default_timeout_used_when_arg_absent ... ok
test tools::bash::tests::times_out_advisory_failure ... ok
test ai::backends::openai::tests::first_token_stall_exhausts_retries_then_errors ... ok
test health::tests::check_returns_unreachable_on_connection_error ... ok

test result: ok. 1045 passed; 0 failed; 2 ignored; 0 measured; 0 filtered out; finished in 6.08s


running 0 tests

test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

    Finished `test` profile [unoptimized + debuginfo] target(s) in 0.08s
     Running unittests src/main.rs (target/debug/deps/rexymcp-4e85b51f198fbe9f)
     Running unittests src/lib.rs (target/debug/deps/executor-c1650299697d7408)
   Doc-tests executor

```

**Files changed:**
- `docs/dev/milestones/M37-governor-read-only-calibration/README.md` — +1 -1
- `docs/dev/milestones/M37-governor-read-only-calibration/phase-03-token-formatter-consolidation.md` — +5 -1
- `executor/src/store/metrics.rs` — +46 -0
- `mcp/src/costs.rs` — +8 -20
- `mcp/src/profile_cli.rs` — +3 -2
- `mcp/src/runs.rs` — +4 -20
- `mcp/src/scorecard_cli.rs` — +2 -2

**Commit:** f9ec0c81e81c5968ca2a42ec2338408a9ba27f3a

**Notes:** server-authored completion entry (executor no longer owns the bookkeeping tail; see M27 phase-03).

