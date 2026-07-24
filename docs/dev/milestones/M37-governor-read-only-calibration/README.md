# M37 — Governor Read-Only Calibration

**Goal:** Stop the governor from hard-killing read-only diagnosis at
write-thrash thresholds, and clear the last of M35's accounting debt.

**Status:** done *(opened 2026-07-24; closed 2026-07-24)*

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
- The Budget ledger (`ledger_lines`, shared by `rexymcp costs` and the dashboard
  panel) aligns the `.`/`—` markers of the Architect, Executor, and Net rows in
  the same column — pinned by a **real** decimal-column test, not the
  equal-width check M38 phase-02 shipped. Tokens mode's header reads `Tokens`
  (phase 06). *(Added 2026-07-24: two dashboard bugs found in use — a debit/credit
  decimal misalignment M38 reintroduced, and the fake test that hid it.)*
- All four gates green.

## Architecture references

- `docs/architecture.md` § Status #37 — this milestone's design summary.
- `docs/architecture.md` § Status #34 — `NoProgressStall` and the
  advisory-until-calibrated pivot this milestone declines to repeat.
- `docs/dev/milestones/M35-metrics-cost-accounting/README.md`
  § "M35 retrospective" — folds 5 and 6, and the accepted debt.

## Phases

**01–06 done** (2026-07-24). Phase 06 was added after 04's approval: two
budget-panel bugs found using the dashboard (debit decimals misaligned; a fake
alignment test that let it regress; tokens-mode header wording). Folded into M37
by user decision rather than reopening the closed M38 that owns `ledger_lines`.
Phase 06 landed via an **architect takeover** — the dispatched executor was
human-stopped mid-loop having written only the two mechanical fixes (both new
tests carrying broken fixtures), so the architect completed the sign-gutter fix,
repaired the fixtures, and verified with a mutation-check + parsed E2E. **All M37
phases are now `done`; the milestone is ready to close on human sign-off.**

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
| 04 | `calibrate-governor` deterministic row order + k/M byte columns ([phase-04-calibrate-governor-render.md](phase-04-calibrate-governor-render.md)) — reuses phase-03's `metrics::fmt_tokens`; both fixes in the pure `format_report` — approved_first_try; STABLE-diff E2E, 2 mutations bite; phase-05's live Executor-line proof landed in this doc | done |
| 05 | Server completion entry: authoritative `**Executor:**` line from the dispatched model ([phase-05-completion-executor-line.md](phase-05-completion-executor-line.md)) — **re-scoped to defect 3 only** (tick=reviewer's job; E2E block deferred) — approved_first_try; negative self-report test bites, live proof deferred to phase-04 dispatch | done |
| 06 | Budget panel: align debit decimals + `Tokens` header + fix the fake alignment test ([phase-06-budget-panel-alignment.md](phase-06-budget-panel-alignment.md)) — M38 `ledger_lines` regression found in use — **escalated** (architect takeover: executor stopped mid-loop having landed only the 2 mechanical fixes with broken test fixtures); mutation-checked, E2E parsed | done |

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

## M37 retrospective (2026-07-24)

**Six phases, all done in one day.** 01 (the milestone: read-only exemption in
`check_oscillation` + `check_identical_repetition`, keyed on
`!mutates_files`, with negative tests pinning that a mutating window still
fires) and five carried-debt phases (02 failure-class vocabulary, 03 token-
formatter consolidation into `metrics::fmt_tokens`, 04 `calibrate-governor`
deterministic render, 05 server `Executor:` line from the dispatched model).
**01–05 approved_first_try.** Phase 06 (added mid-milestone) escalated.

**Phase 03 grew on contact and that was right.** The milestone note said "three
divergent token formatters"; drafting found **four**. The executor consolidated
all four and pinned the decimal-SI decision by mutation. Counting the real call
sites at draft time, not from memory, is the lesson — already the standing
"derive every spec fact from its source" fold; this was another clean instance.

**Phase 05 re-scoped at draft time (recorded, correct).** The milestone note
bundled three completion-bookkeeping defects; drafting found only one is a clean
`mcp/`-only fix (the authoritative `Executor:` line). Ticking acceptance criteria
stays the reviewer's job (it happens at review→done, after the completion entry
fires at in-progress→review); a structured E2E block needs a `PhaseResult`
contract change outside this milestone. Narrowing the phase to the tractable
defect rather than forcing all three was the right call and is worth remembering
as a pattern: a milestone note's "three defects in one place" is a hypothesis,
not a spec.

**Phase 06 — the escalation, and the lesson.** A Budget-ledger decimal
misalignment that M38 phase-02 reintroduced (a credit `$X.XX` rendered its
decimal one column right of a debit `($X.XX)`), the fake equal-width guard test
that let it ship, and the tokens-header wording. The dispatched executor
(`Qwen/Qwen3.6-27B-FP8`) was **human-stopped in a read-only diagnosis loop** at
126 turns, having written only the two mechanical fixes (header rename, the
test's docstring) — the sign-gutter production fix was absent, and both new tests
it wrote carried **broken fixtures** (`ledger_dash_and_decimal_share_column` set
`net: Some(-10.0)`, rendering Net a debit so `net.find('—').expect()` panics; the
panels test's rates made the Executor row a *debit*, so it compared two debits
and could not fail pre-fix). The architect took over (user-authorized),
completed the fix, repaired the fixtures, and verified with a **mutation-check**
(all three alignment tests fail 17-vs-18 against pre-fix rendering) plus a
**parsed E2E** (markers align per scope at cols 18/28/38).

Two observations from phase 06:

1. **A test whose name or docstring promises more than its body asserts is worse
   than no test.** The equal-width guard's docstring claimed decimal-column
   equality but asserted only equal width; it shipped alongside the very
   regression it was named to catch, and slipped a prior review (mine). **This is
   the 1st clear occurrence — recorded, not yet folded** into WORKFLOW.md per the
   three-strike discipline. Watch for recurrence at review time: when spot-
   checking a test (WORKFLOW § "Spot-check tests are real"), read the *body*
   against the *name*, not just "does it assert something."

2. **`NoProgressStall` did not fire at 126 turns** (threshold 60) because the
   executor **interleaved** edits with the search loop, resetting its consecutive
   non-mutating streak below 60 each time. Phase 01's exemption behaved exactly as
   designed — the loop simply wasn't *purely* non-mutating. This is real signal
   for the phase-01 follow-up already logged in `architecture.md` §37 ("does the
   60-call backstop actually catch thrash?"): a **windowed** (not strictly-
   consecutive) no-progress variant may be the eventual answer. Not opened —
   needs the post-exemption corpus the §37 follow-up calls for.

**Deferred / follow-ups leaving M37:**

- **Phase-01 backstop calibration** (architecture.md §37 follow-up): re-run
  `calibrate-governor` after several post-exemption dispatches; check whether the
  `hard_fail` `max_read_only_run` distribution shifts toward 60. Do not tune on
  the pre-exemption corpus.
- **The `missing_spec_test`/broken-fixture failure shape** (phase 06): the
  executor wrote tests that existed but were non-falsifiable. Recorded against the
  `missing_spec_test` class added in phase 02; watch whether it recurs distinctly
  enough to warrant its own label.
- **`executor_val` `$`-less debit** (noted in phase 06 review, out of scope): the
  negative branch emits `(X.XX)` while `net_val` emits `($X.XX)` — a pre-existing
  M38 cosmetic inconsistency. Follow-up nit, unfiled.
- **M39 — Executor cache accounting** (candidate, logged at the M38 close): the
  executor's `cache_read`/`cache_write` read zero across all runs. Not opened.

**No WORKFLOW.md/STANDARDS.md folds landed at this close.** The read-only
calibration folds were already in WORKFLOW.md before M37 (M35 close); the
fake-test lesson is at one occurrence and held for recurrence.