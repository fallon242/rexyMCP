# M45 — Executor Work-Preservation Guards

**Goal:** the runtime, not the spec, protects the executor's own uncommitted
work from self-revert, and the identical-repetition detector stops missing
trivially-varied loops.

**Status:** planning

**Depends on:** none (builds on M37's governor calibration)

**Origin:** DaemonEye's M12 upstream-folds proposal
(`docs/daemoneye-proposed-upstream-folds.md`, §4 "Executor self-sabotage on
delete-heavy rewrites is a runtime concern"). Both pathologies were observed
repeatedly downstream on delete-heavy rewrite phases; DaemonEye's spec-level
mitigations (phase splitting) are folded into the plugin's WORKFLOW template,
but the durable fixes are runtime-side and belong here.

**Current state (verified 2026-08-09):**

- `executor/src/security/bash_classify.rs` blocks `git reset --hard`
  (line 57) and `git checkout .` (line 83), but **not** `git checkout
  <file>`, `git checkout HEAD -- <path>`, `git restore …`, or `git stash` —
  all forms observed downstream wiping a run's own correct, uncommitted work
  mid-phase, followed by a confusion loop when the diff "disappeared."
- `executor/src/governor/hard_fail.rs::check_identical_repetition`
  (line 141) compares exact `(tool, arguments)` pairs, so a loop whose calls
  vary by whitespace or argument ordering never trips it. One downstream
  near-identical verify-loop ran 529 turns until a human stopped it.
- The third DaemonEye runtime ask — trip on N consecutive read-only calls
  with no intervening write — **already landed** in M37
  (`read_only_stall_threshold` + the `normalize_target` oscillation
  machinery). It is out of scope here; do not re-implement it.

**Exit criteria:**

- Any `git checkout|restore|reset|stash` invocation that would discard
  uncommitted changes the current run authored is hard-blocked (or preceded by
  an automatic throwaway checkpoint commit so nothing is lost), covering the
  bare-`<path>`, `HEAD -- <path>`, and `stash` forms — proven by mutation:
  each form attempted against a dirty run-authored file is refused, and the
  refusal is a model-visible `ToolResult`, not a `Result::Err`.
- The identical-repetition detector normalizes tool-call arguments
  (whitespace, argument ordering) before comparison, and a test demonstrates
  a loop of trivially-varied calls now trips at `identical_call_threshold`
  where the pre-M45 comparison did not.
- Blocking a git self-revert never strands the executor: the refusal message
  names why and what to do instead (the work is this run's own; proceed, or
  use `patch` to change it).

## Architecture references

- `docs/architecture.md` §17 (security/scope model), §37 (governor read-only
  calibration)

## Phases

| #  | Phase | Status |
|----|-------|--------|
| —  | expanded on demand at milestone open | — |

## Notes

Scope question to settle at phase drafting: hard-block vs.
checkpoint-then-allow for the git guard. DaemonEye's proposal prefers
checkpoint-then-allow ("nothing is ever lost"), but it adds repo-state
side effects (throwaway commits) the reviewer must understand; hard-block
with a clear advisory is the smaller first phase, and checkpointing can be a
follow-up if refusals prove disruptive.

Pending at milestone open: `docs/architecture.md` §Status needs the M45
roadmap entry (architecture.md is edit-gated; entry added at activation, not
at proposal).
