# Phase 05: Server-authored `Executor:` line from the dispatched model

**Milestone:** M37 — Governor Read-Only Calibration
**Status:** todo
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
