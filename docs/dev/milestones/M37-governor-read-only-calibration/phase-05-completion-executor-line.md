# Phase 05: Server-authored `Executor:` line from the dispatched model

**Milestone:** M37 — Governor Read-Only Calibration
**Status:** review
**Depends on:** none
**Estimated diff:** ~70 lines
**Tags:** language=rust, kind=feature, size=s

## Goal — and the deliberately narrow scope

The server-authored completion entry (`finalize::baseline_entry`) carries a
summary, gate line, command tails, files-changed, and the commit sha — but **no
authoritative record of which model ran the phase.** The only `Executor:` line
that reaches the phase doc today is the executor's own *self-reported* progress
note, and models misidentify themselves (M36 phase-03's note claimed
`Claude Sonnet 4.5` on a run every telemetry surface records as
`Qwen/Qwen3.6-27B-FP8`).

This phase adds one authoritative line: **`**Executor:** <dispatched model>`**,
sourced from the resolved dispatch model the server already holds — never from
the model's self-report.

**Scope decision (user, 2026-07-24):** this phase is *only* the `Executor:` line.
The other two defects the milestone note bundles under "completion bookkeeping"
are **out of scope, by decision, not omission:**

- **Ticking the acceptance-criteria checkboxes stays the reviewer's job.**
  Ticking is a *verification* act and belongs at approval (review→done), which is
  the `/rexymcp:review` skill's step, not the completion entry (which fires at
  in-progress→review, before verification). No `finalize.rs` change; the review
  skill already ticks after verifying.
- **A structured `End-to-end verification:` block is deferred.** The executor's
  E2E output arrives as free prose in `completion_summary`, not a structured
  field, so the server cannot reliably extract it. Making it real needs a
  contract change to the executor loop (a structured E2E field on `PhaseResult`)
  that is outside this phase's `mcp/`-only footprint.

## Architecture references

Read before starting:

- `docs/architecture.md` § Status #37 — this milestone; the completion-entry
  defects and why the `Executor:` line is charged as cosmetic-but-worth-fixing.
- `docs/architecture.md` § Status #35 — `PhaseRun.model` is the config-derived,
  authoritative model identity every aggregator already trusts. The completion
  entry must agree with it.

## Pre-flight

1. Read `docs/dev/STANDARDS.md` top to bottom.
2. Read the architecture references above.
3. Read this entire phase doc before touching any code.
4. Confirm the repo is on a clean branch with no uncommitted changes.

## Current state

**`mcp/src/finalize.rs:97-117` `baseline_entry`** builds the entry and takes no
model:

```rust
fn baseline_entry(result: &PhaseResult, now_ms: u64, code_sha: &str) -> String {
    let summary = if result.completion_summary.trim().is_empty() {
        "(no summary provided by executor)".to_string()
    } else {
        result.completion_summary.trim().to_string()
    };
    let gates = gate_line(&result.command_outputs);
    let command_tails = command_output_tails(&result.command_outputs);
    let files = files_changed_list(&result.files_changed);
    format!(
        "### Update — ts={now_ms} (complete, server-authored)\n\n\
         **Summary:** {summary}\n\n\
         **Gates:** {gates}\n\n\
         ... **Files changed:** ... **Commit:** {code_sha}\n\n\
         **Notes:** server-authored completion entry ...\n"
    )
}
```

**`mcp/src/finalize.rs:6-13` `FinalizeInput`** — the struct the caller fills. It
does **not** carry the model:

```rust
pub struct FinalizeInput<'a> {
    pub phase_doc_path: &'a Path,
    pub repo_root: &'a Path,
    pub result: &'a PhaseResult,
    pub now_ms: u64,
    pub runner: &'a dyn CommandRunner,
}
```

**`PhaseResult` does not carry the model either** (`executor/src/phase/result.rs:78-100`)
— so the model must come through `FinalizeInput`, not the result.

**The caller already has the resolved model.** `mcp/src/runner.rs:111` — the
inner `run_phase` takes `model: &'a str`, the **already-resolved** dispatch model
(`model_override.unwrap_or(cfg.executor.model)`, resolved at `runner.rs:352-355`).
It is the same value that feeds `LoopDeps.model` at `runner.rs:295` and that
becomes `PhaseRun.model`. At the finalize call site (`runner.rs:316-321`):

```rust
    let finalize_input = crate::finalize::FinalizeInput {
        phase_doc_path: inp.phase_doc_path,
        repo_root: inp.repo_path,
        result: &result,
        now_ms: (seams.clock)(),
        runner: seams.runner,
    };
```

`inp.model` is in scope here — this is the one-line wiring.

## Spec

### 1. Add `model` to `FinalizeInput`

`mcp/src/finalize.rs` — add the field:

```rust
pub struct FinalizeInput<'a> {
    pub phase_doc_path: &'a Path,
    pub repo_root: &'a Path,
    pub result: &'a PhaseResult,
    pub now_ms: u64,
    pub runner: &'a dyn CommandRunner,
    /// The resolved dispatch model (same value as `PhaseRun.model`). Written as
    /// the authoritative `**Executor:**` line — never the model's self-report.
    pub model: &'a str,
}
```

### 2. Thread it to `baseline_entry` and emit the line

Change `baseline_entry`'s signature to take the model and add the line
**immediately after `**Summary:**`** so it sits at the top of the entry where a
reader looks for attribution:

```rust
fn baseline_entry(result: &PhaseResult, now_ms: u64, code_sha: &str, model: &str) -> String {
    // ... summary/gates/tails/files unchanged ...
    format!(
        "### Update — ts={now_ms} (complete, server-authored)\n\n\
         **Summary:** {summary}\n\n\
         **Executor:** {model}\n\n\
         **Gates:** {gates}\n\n\
         ... (rest unchanged) ..."
    )
}
```

Update the one call site in `finalize_complete` (`finalize.rs:~99`, inside the
active path) to pass `inp.model`.

### 3. Wire the caller

`mcp/src/runner.rs:316-321` — add `model: inp.model,` to the `FinalizeInput`
literal. No other change; the value is already in scope.

### 4. Tests

Per § Test plan.

## Acceptance criteria

- [ ] `cargo build` is green.
- [ ] `cargo clippy --all-targets --all-features -- -D warnings` is clean.
- [ ] `cargo fmt --all --check` reports no diff in the files this phase touched.
- [ ] `cargo test` passes.
- [ ] The server completion entry contains `**Executor:** <model>` where
      `<model>` is the value passed via `FinalizeInput.model`.
- [ ] The `Executor:` line is sourced from `FinalizeInput.model` and **not** from
      `result.completion_summary` — pinned by the negative test below.

## Test plan

In `mcp/src/finalize.rs`'s `#[cfg(test)] mod tests`. `baseline_entry` is a pure
function over its args — test it directly; the module already builds
`PhaseResult` fixtures for the `finalize_*` tests, reuse that shape.

- `baseline_entry_includes_executor_line_from_model` — call `baseline_entry`
  with `model = "Qwen/Qwen3.6-27B-FP8"`; assert the output contains
  `**Executor:** Qwen/Qwen3.6-27B-FP8`.
- `baseline_entry_executor_line_ignores_self_report` — **the load-bearing
  negative test.** Build a `PhaseResult` whose `completion_summary` contains a
  *different, wrong* self-reported model, e.g.
  `"**Executor:** Claude Sonnet 4.5 — all done"`, and call `baseline_entry` with
  `model = "Qwen/Qwen3.6-27B-FP8"`. Assert the entry's authoritative line is
  `**Executor:** Qwen/Qwen3.6-27B-FP8`, and assert the entry does **not** contain
  `**Executor:** Claude Sonnet 4.5` as a standalone attribution. (The self-report
  string may still appear *inside* the quoted `**Summary:**` block — that is the
  executor's prose, unchanged. The test must distinguish "the server's Executor
  line is correct" from "the self-report text is scrubbed"; only the former is in
  scope. Assert on the `**Executor:** ` line's value, e.g. by finding the line
  that starts with `**Executor:**` and checking it equals the dispatched model.)
- `finalize_complete_writes_executor_line` — the existing `finalize_complete`
  happy-path test (search for the test that asserts the entry is appended)
  already drives the full path with a fake runner; extend its assertion (or add a
  sibling) to confirm the written doc contains the `Executor:` line with the
  fixture's model. Do not duplicate the whole test — reuse the existing harness.

Pin the **line content** (`**Executor:** <model>`), not the entry's byte layout
or the line's index — per WORKFLOW § "Specs pin behavior, not rendering".

## End-to-end verification

The completion entry is written by the running server, so the honest E2E is a
real dispatch — but that is the next phase's dispatch, not something forced here.
Instead, verify against the real binary by exercising the pure function through a
tiny harness is not possible from the CLI; so:

- Run `cargo test -p rexymcp finalize` and paste the output showing the two new
  named tests passing.
- State that live confirmation is deferred to the **next** dispatched phase's
  completion entry, which will carry `**Executor:** Qwen/Qwen3.6-27B-FP8` — and
  that a reviewer can eyeball it there. Do **not** claim a live entry was
  produced by this phase.

This is a completion-bookkeeping change with no CLI surface of its own; the unit
tests over the pure `baseline_entry` are the primary artifact, and the negative
test is what makes them meaningful.

## Authorizations

None. No new dependencies. No edits to `docs/architecture.md`.

The change touches `mcp/` only (`finalize.rs`, `runner.rs`). `PhaseResult` and the
executor loop are **not** modified — the model is threaded through
`FinalizeInput` from the caller, which sidesteps any executor-crate change.

## Out of scope

- **Ticking acceptance-criteria checkboxes** — stays the reviewer's job (scope
  decision above). No change to `finalize.rs`'s status/entry logic beyond the
  one added line.
- **A structured `End-to-end verification:` block** — deferred; needs a
  `PhaseResult` contract change outside this phase.
- **Scrubbing or rewriting the executor's self-reported progress note.** The
  server adds an authoritative line; it does not mutate the executor's own prose.
- Anything in `executor/`, and the other M37 phases (04 calibrate-governor
  rendering).

## Update Log

(Filled in by the executor. See WORKFLOW.md § "Update Log entries".)

<!-- entries appended below this line -->

### Update — 2026-07-24 15:57 (started)

**Executor:** Qwen/Qwen3.6-27B-FP8

Added `model: &'a str` to `FinalizeInput`, threaded it through `baseline_entry` to emit `**Executor:** {model}` immediately after `**Summary:**`, and wired the caller in `runner.rs`. Updated all existing test `FinalizeInput` fixtures and added three new tests: `baseline_entry_includes_executor_line_from_model`, `baseline_entry_executor_line_ignores_self_report`, and `finalize_complete_writes_executor_line`.
### Update — ts=1784909598122 (complete, server-authored)

**Summary:** **Summary + Notes for review:**

Added an authoritative `**Executor:** {model}` line to the server-authored completion entry, sourced from the resolved dispatch model rather than the executor's self-report. The change adds `model: &'a str` to `FinalizeInput`, threads it to `baseline_entry` which emits the line immediately after `**Summary:**`, and wires `inp.model` at the `runner.rs` call site. All 6 existing test `FinalizeInput` fixtures were updated, and 3 new tests were added: a positive test confirming the Executor line carries the dispatched model, a negative test confirming a wrong self-report in `completion_summary` doesn't override it, and an integration test confirming the full `finalize_complete` path writes the line to the phase doc. All 1045 tests pass, clippy is clean, and the tree is committed. Live confirmation is deferred to the next dispatched phase's completion entry.

**Gates:** format=run, build=run, lint=run, test=run

**Command output tails:**

```
FORMAT


BUILD
   Compiling rexymcp v0.9.1 (/home/matt/src/rexyMCP/mcp)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.74s


LINT
    Checking rexymcp v0.9.1 (/home/matt/src/rexyMCP/mcp)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 1.16s


TEST
sts::invalid_state_returns_advisory_error ... ok
test tools::update_task::tests::malformed_args_returns_advisory_error ... ok
test tools::update_task::tests::metadata_shape_is_unchanged ... ok
test tools::symbols::tests::references_finds_call_sites ... ok
test tools::update_task::tests::null_args_returns_recovery_hint ... ok
test tools::update_task::tests::result_flags_redundant_remark ... ok
test tools::update_task::tests::result_lists_remaining_incomplete_ids ... ok
test tools::symbols::tests::references_single_file_path ... ok
test tools::update_task::tests::success_output_names_task ... ok
test tools::update_task::tests::result_reports_all_complete_when_last_done ... ok
test tools::update_task::tests::unknown_id_returns_advisory_error ... ok
test tools::write_file::tests::append_creates_file_if_missing ... ok
test tools::write_file::tests::appends_to_existing_file ... ok
test tools::write_file::tests::append_false_overwrites ... ok
test tools::write_file::tests::creates_new_file ... ok
test tools::write_file::tests::missing_path_returns_recovery_hint ... ok
test tools::write_file::tests::non_object_args_do_not_panic ... ok
test tools::write_file::tests::overwrites_existing_file ... ok
test tools::write_file::tests::rejects_malformed_args ... ok
test tools::symbols::tests::references_no_matches_advisory ... ok
test tools::symbols::tests::caps_at_max_results ... ok
test tools::symbols::tests::finds_rust_function_by_name ... ok
test tools::write_file::tests::reports_missing_parent_dir ... ok
test tools::write_file::tests::scope_escape_returns_advisory_error_and_writes_nothing ... ok
test tools::write_file::tests::success_output_includes_line_count ... ok
test tools::symbols::tests::no_symbols_returns_advisory_error ... ok
test tools::symbols::tests::finds_python_function_and_class ... ok
test tools::symbols::tests::references_truncation_note_omits_kind_filter ... ok
test tools::symbols::tests::references_snippet_shows_source_line ... ok
test tools::symbols::tests::reports_line_and_column ... ok
test ai::backends::openai::tests::is_retriable_transport_true_for_reqwest_error ... ok
test tools::symbols::tests::references_across_multiple_files ... ok
test tools::symbols::tests::metadata_carries_definitions_and_files_count ... ok
test tools::symbols::tests::respects_gitignore ... ok
test tools::symbols::tests::unsupported_extension_skipped_in_dir_walk ... ok
test tools::bash::tests::cargo_command_records_cargo_filter_label ... ok
test tools::symbols::tests::finds_rust_struct_and_trait ... ok
test governor::verifier::tests::verify_rust_returns_checked_empty_on_clean_code ... ok
test governor::verifier::tests::capture_baseline_dedupes_by_project_root ... ok
test governor::verifier::tests::verify_rust_returns_checked_with_errors_on_broken_code ... ok
test governor::verifier::tests::capture_baseline_skips_unsupported_files ... ok
test tools::bash::tests::cargo_command_output_is_filtered_through_cargo_filter ... ok
test ai::backends::openai::tests::first_token_stall_retries_then_succeeds ... ok
test ai::backends::openai::tests::midstream_stall_is_not_retried ... ok
test ai::tests::stream_next_uses_supplied_timeout ... ok
test tools::bash::tests::default_timeout_used_when_arg_absent ... ok
test tools::bash::tests::arg_timeout_overrides_constructor_default ... ok
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
- `docs/dev/milestones/M37-governor-read-only-calibration/phase-05-completion-executor-line.md` — +7 -1
- `mcp/src/finalize.rs` — +121 -2
- `mcp/src/runner.rs` — +1 -0

**Commit:** fb8fd62f96e61aa0048e892dbc8ef2ace2fcace3

**Notes:** server-authored completion entry (executor no longer owns the bookkeeping tail; see M27 phase-03).

