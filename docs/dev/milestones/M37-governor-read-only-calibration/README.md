# M37 — Governor Read-Only Calibration

**Goal:** Stop the governor from hard-killing read-only diagnosis at
write-thrash thresholds, and clear the last of M35's accounting debt.

**Status:** in-progress *(opened 2026-07-24)*

**Depends on:** M34 (`NoProgressStall`, which this milestone makes the sole
read-only terminator), M35 (source of the calibration data and the debt)

## Why this milestone exists

**The STRONG fold from the M35 close — 4 occurrences, well past the
three-strike "fold immediately" line.** `check_oscillation`
(`executor/src/governor/hard_fail.rs:225-256`) and
`check_identical_repetition` (`:137-157`) key on `(tool, arguments)` and are
blind to whether a call **mutates** anything. So a model re-running
`sed -n`/`cat`/`python3 -c` to diagnose a confusing failure is terminated on the
same threshold as a genuine write-thrash loop.

M34 already shipped `check_no_progress_stall` (`:274`) for exactly this case —
N consecutive non-mutating calls, threshold 60 — and it already calls
`crate::tools::mutates_files`. The two are duplicate coverage at wildly
different thresholds, and the tighter one wins, which is why it pre-empts.

Across the M35 arc every one of the 4 oscillations recovered on a resume or a
refined re-dispatch carrying one specific hint — the production code had been
correct or nearly so each time. The runs were killed mid-diagnosis, not
mid-thrash.

**User decision (2026-07-23):** exempt windows containing no file-mutating call
from both detectors, leaving read-only loops to `NoProgressStall`.

Rejected alternatives, recorded so they are not re-litigated:

- **Advisory mode**, on M34 phase-05's `novelty_action` precedent — keeps a
  signal, but keeps the pre-emption risk and adds a config knob nobody has data
  to tune.
- **Separate looser read-only thresholds** — still hard-kills, just later, and
  doubles the threshold surface.

## Exit criteria

- A window of tool calls containing **no** file-mutating call fires neither
  `Oscillation` nor `IdenticalToolCallRepetition`. A window containing at least
  one mutating call behaves exactly as it does today (pinned by a negative
  test — this must not become a blanket disable).
- `NoProgressStall` still terminates a purely read-only run at its configured
  threshold; the exemption must not create an unterminated loop.
- `oscillation_stall` is in `FAILURE_CLASSES`
  (`executor/src/store/telemetry.rs:319`) and `is_known_failure_class` accepts
  it. Recorded 2× as an unknown open-vocab class during M35.
- **`missing_spec_test`** is in `FAILURE_CLASSES` too. Recorded open-vocab at
  the M38 phase-01 bounce (2026-07-24): the executor implemented the production
  change correctly but omitted one of the four tests the spec's § Test plan
  named. None of the nine existing classes fits — it is not
  `false_completion` (gates were green), not `scope_deviation` (nothing extra
  was touched), and not `spec_bug` (the spec named the test explicitly). A
  spec'd-but-unwritten test is a distinct and recurring enough failure mode to
  deserve its own label, or the scorecard buckets it as noise.
- One token formatter. `runs::fmt_tokens`, the inline formatter in `scorecard`,
  and `costs::format_tokens` collapse into the shared `metrics` helper, with
  every call site migrated.
- `calibrate-governor`'s output-flood byte columns render k/M-compacted, in line
  with the shared rendering 07c established.
- The **server-authored completion entry** carries an authoritative
  `**Executor:**` line naming the **dispatched** model (the same value as
  `PhaseRun.model`), never the model's self-report — pinned by a test that a
  self-reported name in `completion_summary` cannot become the entry's
  `Executor:` line (phase 05). *(Re-scoped 2026-07-24: the original criterion
  also asked the entry to tick acceptance criteria and emit an E2E block. Ticking
  is the reviewer's job at approval, not the completion entry's at
  in-progress→review — a `/rexymcp:review` skill responsibility, already done, no
  code fix. The E2E block is deferred: the executor's E2E output is unstructured
  prose in `completion_summary`, so extracting it needs a `PhaseResult` contract
  change outside this milestone's scope.)*
- All four gates green.

## Architecture references

- `docs/architecture.md` § Status #37 — this milestone's design summary.
- `docs/architecture.md` § Status #34 — `NoProgressStall` and the
  advisory-until-calibrated pivot this milestone declines to repeat.
- `docs/dev/milestones/M35-metrics-cost-accounting/README.md`
  § "M35 retrospective" — folds 5 and 6, and the accepted debt.

## Phases

**01–03 + 05 done; 04 drafted — the last phase** (2026-07-24). After 04 lands
the milestone is complete and closes on human sign-off.

**Phase 05 was re-scoped at draft time (user decision, 2026-07-24).** The
milestone note bundled three "completion bookkeeping" defects; drafting found
they are not equally tractable, so 05 is now **only** the authoritative
`Executor:` line — a clean, `mcp/`-only, additive fix. The other two are
resolved out of the code path:

- **Ticking acceptance criteria stays the reviewer's job**, not `finalize.rs`.
  Ticking is verification and belongs at approval (review→done); the completion
  entry fires at in-progress→review, before verification. The `/rexymcp:review`
  skill already ticks. Not a code defect.
- **A structured E2E block is deferred** — the executor's E2E output is free
  prose in `completion_summary`, not a structured field, so the server can't
  extract it without a `PhaseResult` contract change outside `mcp/`. A future
  phase/milestone if it recurs as friction.

| #  | Phase | Status |
|----|-------|--------|
| 01 | Read-only exemption in the oscillation + identical-repetition detectors ([phase-01-read-only-exemption.md](phase-01-read-only-exemption.md)) — approved_first_try; 3 reviewer mutations, calibration distributions unmoved | done |
| 02 | `oscillation_stall` + `missing_spec_test` in `FAILURE_CLASSES` ([phase-02-failure-class-vocabulary.md](phase-02-failure-class-vocabulary.md)) — approved_first_try; negative control holds, 2 guards mutation-checked | done |
| 03 | Consolidate the token formatters into `metrics::fmt_tokens` ([phase-03-token-formatter-consolidation.md](phase-03-token-formatter-consolidation.md)) — **4** divergent formatters, not 3; canonical = decimal-SI-with-M, changes `runs`/`scorecard` output — approved_first_try; decimal decision pinned by mutation, cxt_win correctly excluded | done |
| 04 | `calibrate-governor` deterministic row order + k/M byte columns ([phase-04-calibrate-governor-render.md](phase-04-calibrate-governor-render.md)) — reuses phase-03's `metrics::fmt_tokens`; both fixes in the pure `format_report` | todo |
| 05 | Server completion entry: authoritative `**Executor:**` line from the dispatched model ([phase-05-completion-executor-line.md](phase-05-completion-executor-line.md)) — **re-scoped to defect 3 only** (tick=reviewer's job; E2E block deferred) — approved_first_try; negative self-report test bites, live proof deferred to phase-04 dispatch | done |

Phase 01 is the milestone; 02–05 are carried debt and can run in any order after
it. Phase 01 changes governor termination behavior, so it needs negative tests
pinning that mutating windows are untouched — a blanket disable would pass a
positive-only suite.

## Notes

**Phase 05 — why it exists (added 2026-07-23 at the M36 phase-01 review).**
STANDARDS §1 requires that every acceptance criterion be ticked and that any
criterion referencing a real artifact be verified end-to-end with **the actual
output quoted** in the completion Update Log. Since M27 phase-03 moved the
bookkeeping tail server-side, the server-authored completion entry does
neither: it writes a summary, gate labels, command-output tails, files-changed
and the commit sha, but leaves the phase doc's `- [ ]` boxes untouched and
emits no `**End-to-end verification:**` section.

The result is a `done` phase doc whose own acceptance criteria read as unmet,
and an E2E claim asserted in prose rather than evidenced. Reproduced on M35
phase-06e, 07g, 07h and M36 phase-01, 02, 03 — **6 occurrences**, well past the
fold-immediately threshold. It is not an executor defect and **cannot be fixed
by re-dispatch**: the executor no longer owns that output. Each review has been
silently absorbing the gap by verifying and ticking manually.

**A third defect, same writer (added 2026-07-23 at the M36 phase-03 review).**
The Update Log's `**Executor:**` line is written from the model's *self-report*,
and models misidentify themselves. M36 phase-03's entry claims
`Executor: Claude Sonnet 4.5 (executor)` when `rexymcp.toml`, `executor_health`,
and the run's own `PhaseRun` telemetry record all say `Qwen/Qwen3.6-27B-FP8`.

Severity is **cosmetic, not corrupting**: the scorecard, `profile`, and
`calibrate-governor` all read the config-derived `PhaseRun.model` field, so no
aggregate is polluted — but the phase doc is the human-readable record, and a
milestone retrospective read months later would attribute the work to the wrong
model. The server dispatched the run and knows which model it used; it should
write that value rather than let the model name itself.

Scope: the server's completion-entry writer, three defects in one place.

1. **Tick the acceptance criteria.** Deciding what justifies a tick is the
   design question — the safe shape is to tick only criteria whose verifying
   command the server actually ran and observed pass, leaving the rest for the
   reviewer rather than ticking optimistically. A false tick is worse than a
   blank box.
2. **Emit an `**End-to-end verification:**` block** with the actual output. The
   writer already receives the command outputs it would need to quote.
3. **Write `Executor:` from the dispatched model**, not from model self-report.
   Source it from the same value that populates `PhaseRun.model`, so the prose
   and the telemetry can never disagree. Pin a test that a self-reported model
   name in the transcript does **not** reach the Update Log.

**Sequencing against M36.** Independent — M36 is `mcp/` accounting and display,
M37 is `executor/governor` plus small `mcp/` cleanups. Phase 03 (token
formatters) touches `costs::format_tokens`, which M36 phase-02 also edits; run
M36 phase-02 first or expect a trivial conflict.

**Calibration data available.** `rexymcp calibrate-governor` (M34 06a/06b)
replays the session-log corpus and reports per-model and global distributions
by run outcome for every governor signal. Phase 01 should check its
`oscillation_min_distinct` low-tail output (M35 07a) before and after, and the
result belongs in the phase's Update Log — this is the first change to a
terminator since that tooling existed.