# Phase 02: Add `oscillation_stall` and `missing_spec_test` to `FAILURE_CLASSES`

**Milestone:** M37 — Governor Read-Only Calibration
**Status:** done
**Depends on:** none (independent of phase-01; both may land in either order)
**Estimated diff:** ~50 lines
**Tags:** language=rust, kind=feature, size=s

## Goal

Two failure classes have been recorded as **open-vocabulary** values — accepted
with a warning, and bucketed as noise by every aggregator that groups on the
canonical list. Both have recurred enough to earn a name:

- **`oscillation_stall`** — recorded 2× during M35 for runs the governor
  terminated on an oscillation / identical-repetition / stall detector.
- **`missing_spec_test`** — recorded 2× during M38 phase-01, for an otherwise
  correct implementation that omitted a test the spec's § Test plan named.

The code change is small. **The load-bearing part is the doc comment on each
new entry**, because the taxonomy's only defence against drift is that two
reviewers reading the same bounce pick the same class.

## Architecture references

Read before starting:

- `docs/architecture.md` § Status #37 — this milestone, including why
  `missing_spec_test` fits none of the nine existing classes.
- `docs/architecture.md` § Status #7 — the `PhaseReview` / failure-class design
  and the reason `spec_bug`/`infra_blip` exist: so a bounce that is *not the
  model's fault* is not charged against its competency. The two new classes sit
  on opposite sides of that line, which is the distinction the comments must
  carry.

## Pre-flight

1. Read `docs/dev/STANDARDS.md` top to bottom.
2. Read the architecture references above.
3. Read this entire phase doc before touching any code.
4. Confirm the repo is on a clean branch with no uncommitted changes.

## Current state

**`executor/src/store/telemetry.rs:319-329`** — the canonical list. Note the
doc comment above it already says the vocabulary is *intentionally open* and
that "new classes fold in as they recur (WORKFLOW § Calibration)". This phase is
that fold; no policy change is needed.

```rust
pub const FAILURE_CLASSES: &[&str] = &[
    "none",              // clean approval
    "false_completion",  // self-reported complete on a red gate
    "prod_unwrap",       // unwrap/expect in a production path (STANDARDS §2.1)
    "multi_site_break",  // breaking multi-site type change ran out of verifier runway
    "parse_format",      // tool-call format / forgiving-parser repair churn
    "masked_diagnostic", // #[allow]/#[ignore] used to hide a warning/error
    "scope_deviation",   // touched out-of-scope files or widened scope
    "spec_bug",          // the bounce was the architect's spec fault, not the model's
    "infra_blip",        // transient backend/decode error, not a work defect
];

/// True if `class` is in the canonical `FAILURE_CLASSES` vocabulary.
pub fn is_known_failure_class(class: &str) -> bool {
    FAILURE_CLASSES.contains(&class)
}
```

**The warning path is automatic.** `mcp/src/review.rs:62-67` filters the
supplied classes through `is_known_failure_class`; `mcp/src/main.rs:862` prints
`warning: unknown failure class …` when any survive. Adding entries to the const
silences both — **no change is needed in `review.rs` or `main.rs`.**

**The existing vocabulary test** is
`telemetry.rs:1288` `is_known_failure_class_validates_vocabulary`, which asserts
four known classes and one made-up one.

## Spec

### 1. Add the two entries

Append to `FAILURE_CLASSES` in `executor/src/store/telemetry.rs`, keeping the
existing alignment of the trailing comments:

```rust
    "oscillation_stall", // governor terminated the run (oscillation / identical-repetition / stall)
    "missing_spec_test", // implementation correct, but a test the spec named was not written
```

Order matters only for readability — append at the end, after `infra_blip`, so
the diff is additive and the existing entries keep their positions.

### 2. Document the boundaries in the const's doc comment

This is the substantive part. Extend the doc comment above `FAILURE_CLASSES`
with a short paragraph distinguishing the two new classes from their nearest
neighbours, so the taxonomy stays usable:

- **`oscillation_stall` vs `parse_format`** — both show up as a run that made no
  progress, but `parse_format` is *tool-call syntax* churn the forgiving parser
  had to repair, while `oscillation_stall` is the **governor** ending a run that
  was repeating or cycling. If a `HardFail` event names an oscillation,
  identical-repetition, or stall detector, it is `oscillation_stall`.
- **`missing_spec_test` vs `false_completion`** — `false_completion` is a
  **red gate** the model reported as complete. `missing_spec_test` is the
  opposite situation: **all gates genuinely green**, production code correct, but
  a test the § Test plan named was never written, so the behavior is unpinned.
  It is charged to the model (the spec named the test), which is what separates
  it from `spec_bug`.
- **`missing_spec_test` vs `scope_deviation`** — `scope_deviation` is doing
  *more* than the spec asked; `missing_spec_test` is doing *less*.

Keep it tight — three or four sentences. This is a reference a reviewer reads
mid-verdict, not an essay.

### 3. Extend the vocabulary test

Per § Test plan below.

## Acceptance criteria

- [x] `cargo build` is green.
- [x] `cargo clippy --all-targets --all-features -- -D warnings` is clean.
- [x] `cargo fmt --all --check` reports no diff in the files this phase touched.
- [x] `cargo test` passes.
- [x] `is_known_failure_class("oscillation_stall")` and
      `is_known_failure_class("missing_spec_test")` both return `true`.
- [x] `rexymcp review … --failure-class oscillation_stall` records **without**
      the `warning: unknown failure class` line.
- [x] The nine pre-existing entries are unchanged, in their original order.
- [x] `mcp/src/review.rs` and `mcp/src/main.rs` are **unmodified** — the warning
      path needs no change.

## Test plan

In `executor/src/store/telemetry.rs`'s `#[cfg(test)] mod tests`:

- Extend the existing `is_known_failure_class_validates_vocabulary` with
  assertions for both new classes. **Add to it; do not replace its existing
  assertions and do not write a second near-duplicate test** — it is the
  vocabulary's single guard and should stay that way.
- `failure_classes_preserves_existing_vocabulary` — asserts every one of the
  nine pre-existing strings is still present. A negative-shaped guard: it fails
  if a future edit renames or drops one while adding new entries, which is the
  realistic way this list decays.

Do **not** assert the list's exact length or the new entries' index. Pinning
length or position makes every future fold a two-line change for no benefit,
and the const's own doc comment says the vocabulary is open.

## End-to-end verification

The warning path is the real artifact — exercise it against the actual binary,
writing to a throwaway telemetry file so the live store is untouched:

```bash
TMP=$(mktemp -d)
cargo run -p rexymcp -- review --config rexymcp.toml \
  --phase-id e2e-vocab --verdict bounced \
  --failure-class oscillation_stall --failure-class missing_spec_test \
  --telemetry-path "$TMP/phase_runs.jsonl" 2>&1

# Negative control — a genuinely unknown class must STILL warn:
cargo run -p rexymcp -- review --config rexymcp.toml \
  --phase-id e2e-vocab --verdict bounced \
  --failure-class definitely_not_a_class \
  --telemetry-path "$TMP/phase_runs.jsonl" 2>&1
```

Paste both outputs in the completion Update Log. Expected: the **first** command
prints only `recorded review for e2e-vocab -> …` with **no** warning line; the
**second** still prints `warning: unknown failure class "definitely_not_a_class"`.

The negative control matters more than the positive one — a change that silenced
the warning for *everything* would pass the first command and quietly destroy the
guard that keeps the vocabulary meaningful.

## Authorizations

None. No new dependencies.

**`docs/architecture.md` must not be edited.** Its § Status #7 mention of the
taxonomy (`false_completion`, `prod_unwrap`, `multi_site_break`, …) is
deliberately elliptical and stays accurate as the list grows.

## Out of scope

- Any change to `mcp/src/review.rs` or `mcp/src/main.rs`. Adding to the const is
  sufficient; touching the warning path risks the negative control above.
- Re-classifying historical `PhaseReview` records that used these strings as
  open-vocab. They already carry the right value — this phase makes them
  *recognised*, retroactively, at read time.
- Making `FAILURE_CLASSES` a closed enum. The const's doc comment states the
  open-vocabulary decision; changing it is an architecture decision, not a
  phase's.
- The other M37 phases (01 read-only exemption, 03 token formatters, 04
  calibrate-governor rendering, 05 completion-entry writer).

## Update Log

(Filled in by the executor. See WORKFLOW.md § "Update Log entries".)

<!-- entries appended below this line -->

### Update — 2026-07-24 13:33 (started)

**Executor:** Claude (Sonnet)

Added `oscillation_stall` and `missing_spec_test` to `FAILURE_CLASSES`, extended the doc comment with boundary distinctions, and added vocabulary tests.
### Update — ts=1784900237911 (complete, server-authored)

**Summary:** **Summary + Notes for review**

Added `oscillation_stall` and `missing_spec_test` to `FAILURE_CLASSES` in `executor/src/store/telemetry.rs`, with a doc comment distinguishing each from its nearest neighbours (`parse_format`, `false_completion`, `spec_bug`, `scope_deviation`). Extended the existing vocabulary test with assertions for both new classes and added `failure_classes_preserves_existing_vocabulary` as a negative guard. End-to-end verification confirmed: the first `rexymcp review` with both new classes printed only `recorded review for e2e-vocab` with no warning; the negative control with `definitely_not_a_class` correctly emitted `warning: unknown failure class`. The known-classes list in that warning output now includes all 11 entries. No changes to `mcp/src/review.rs` or `mcp/src/main.rs` — as expected, the warning path adapts automatically.

**Gates:** format=run, build=run, lint=run, test=run

**Command output tails:**

```
FORMAT


BUILD
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.06s


LINT
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.08s


TEST
symbols::tests::defaults_to_scope_root_when_no_path_given ... ok
test tools::update_task::tests::invalid_args_hint_reports_all_complete ... ok
test tools::update_task::tests::invalid_state_returns_advisory_error ... ok
test tools::update_task::tests::flips_pending_task_to_active ... ok
test tools::symbols::tests::kind_filter_returns_only_matching_kind ... ok
test tools::symbols::tests::references_respects_max_results ... ok
test tools::update_task::tests::null_args_returns_recovery_hint ... ok
test tools::update_task::tests::result_flags_redundant_remark ... ok
test tools::update_task::tests::malformed_args_returns_advisory_error ... ok
test tools::update_task::tests::metadata_shape_is_unchanged ... ok
test tools::update_task::tests::result_lists_remaining_incomplete_ids ... ok
test tools::update_task::tests::result_reports_all_complete_when_last_done ... ok
test tools::update_task::tests::success_output_names_task ... ok
test tools::update_task::tests::unknown_id_returns_advisory_error ... ok
test tools::write_file::tests::append_creates_file_if_missing ... ok
test tools::write_file::tests::appends_to_existing_file ... ok
test tools::write_file::tests::append_false_overwrites ... ok
test tools::write_file::tests::creates_new_file ... ok
test tools::write_file::tests::overwrites_existing_file ... ok
test tools::write_file::tests::rejects_malformed_args ... ok
test tools::write_file::tests::missing_path_returns_recovery_hint ... ok
test tools::write_file::tests::reports_missing_parent_dir ... ok
test tools::write_file::tests::scope_escape_returns_advisory_error_and_writes_nothing ... ok
test tools::write_file::tests::non_object_args_do_not_panic ... ok
test tools::write_file::tests::success_output_includes_line_count ... ok
test tools::symbols::tests::finds_python_function_and_class ... ok
test tools::symbols::tests::references_snippet_shows_source_line ... ok
test tools::symbols::tests::references_across_multiple_files ... ok
test tools::symbols::tests::references_truncation_note_omits_kind_filter ... ok
test ai::backends::openai::tests::is_retriable_transport_true_for_reqwest_error ... ok
test tools::symbols::tests::reports_line_and_column ... ok
test tools::symbols::tests::metadata_carries_definitions_and_files_count ... ok
test tools::symbols::tests::unsupported_extension_skipped_in_dir_walk ... ok
test tools::symbols::tests::respects_gitignore ... ok
test tools::symbols::tests::finds_rust_struct_and_trait ... ok
test tools::bash::tests::cargo_command_records_cargo_filter_label ... ok
test governor::verifier::tests::verify_rust_returns_checked_empty_on_clean_code ... ok
test governor::verifier::tests::capture_baseline_dedupes_by_project_root ... ok
test governor::verifier::tests::capture_baseline_skips_unsupported_files ... ok
test governor::verifier::tests::verify_rust_returns_checked_with_errors_on_broken_code ... ok
test tools::bash::tests::cargo_command_output_is_filtered_through_cargo_filter ... ok
test ai::backends::openai::tests::first_token_stall_retries_then_succeeds ... ok
test ai::backends::openai::tests::midstream_stall_is_not_retried ... ok
test ai::tests::stream_next_uses_supplied_timeout ... ok
test tools::bash::tests::default_timeout_used_when_arg_absent ... ok
test tools::bash::tests::arg_timeout_overrides_constructor_default ... ok
test tools::bash::tests::times_out_advisory_failure ... ok
test ai::backends::openai::tests::first_token_stall_exhausts_retries_then_errors ... ok
test health::tests::check_returns_unreachable_on_connection_error ... ok

test result: ok. 1040 passed; 0 failed; 2 ignored; 0 measured; 0 filtered out; finished in 6.08s


running 0 tests

test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

    Finished `test` profile [unoptimized + debuginfo] target(s) in 0.07s
     Running unittests src/main.rs (target/debug/deps/rexymcp-4e85b51f198fbe9f)
     Running unittests src/lib.rs (target/debug/deps/executor-c1650299697d7408)
   Doc-tests executor

```

**Files changed:**
- `docs/dev/milestones/M37-governor-read-only-calibration/README.md` — +1 -1
- `docs/dev/milestones/M37-governor-read-only-calibration/phase-02-failure-class-vocabulary.md` — +7 -1
- `executor/src/store/telemetry.rs` — +37 -0

**Commit:** 8183834b3dc4de129dc0cb2cd46041828d361be4

**Notes:** server-authored completion entry (executor no longer owns the bookkeeping tail; see M27 phase-03).


### Review verdict — 2026-07-24

- **Verdict:** approved_first_try
- **Bounces:** none
- **Executor:** Qwen/Qwen3.6-27B-FP8 (32 turns, no oscillation)
- **Scope deviations:** none. Diff is `telemetry.rs` + the two doc files;
  `review.rs` and `main.rs` untouched across both executor commits, exactly as
  the spec scoped and as the executor confirmed in Notes.
- **Calibration:** none.

**Reviewer verification.** Four gates re-run independently with a forced
recompile of both crates, zero warnings. Tests **1039 → 1040** — one net new
(`failure_classes_preserves_existing_vocabulary`); the vocabulary test was
*extended* rather than duplicated, as the spec required.

**The two guards are mutation-sensitive, each against a distinct failure:**

| mutation | fails | catches |
|---|---|---|
| remove the two new entries | `is_known_failure_class_validates_vocabulary` | the feature not landing |
| rename `parse_format` → `parse_fmt` | `failure_classes_preserves_existing_vocabulary` | the realistic decay — a fold that drops/renames an old entry while adding new ones |

The nine originals are present, in their original order, with the two new
entries appended.

**E2E — the negative control is the one that matters, and it holds:**

```
$ rexymcp review … --failure-class oscillation_stall --failure-class missing_spec_test
recorded review for e2e-vocab -> …          # no warning ✓

$ rexymcp review … --failure-class definitely_not_a_class
warning: unknown failure class "definitely_not_a_class" (recorded anyway);
  known classes: [… "oscillation_stall", "missing_spec_test"]   # still warns ✓
```

Both new classes record silently; a genuinely unknown class still warns, and its
known-classes list now shows all 11. A change that had silenced the warning
wholesale would have passed the positive check and failed here — it didn't.

**The doc comment — the phase's actual deliverable — is correct and complete.**
All three specified boundaries are drawn: `oscillation_stall` vs `parse_format`
(governor terminator vs parser churn), `missing_spec_test` vs `false_completion`
(green gates vs red), and `missing_spec_test` vs `scope_deviation` (doing less
vs more). It also correctly states `missing_spec_test` is charged to the model,
unlike `spec_bug` — the `PhaseReview` distinction that motivated the class.

**Closes two open-vocab taxonomy gaps** carried since M35 (`oscillation_stall`,
2×) and M38 phase-01 (`missing_spec_test`, 2×). Both are now recognised
retroactively at read time on the records that first used them.
