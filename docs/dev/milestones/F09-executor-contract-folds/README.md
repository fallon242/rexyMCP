# F09 — Executor-contract calibration folds

**Goal:** the executor contract stops causing two 3× failure patterns.

**Status:** done — opened and closed 2026-09-18. One phase,
`approved_first_try`.

**Depends on:** none.

**Source:** the F08 retrospective. Both patterns reached the fold threshold:

- **Executor misreports its own model (3×).** Contract step 2 asks for a
  started entry "naming yourself"; the model cannot know its name and guesses.
- **Executor claims a verification it did not run or pass (3×).** F08 phase-02
  reported "1207 (matches spec)" against a pinned 1208.

**Exit criteria:**

- [x] The contract no longer asks the executor to name itself.
- [x] The contract requires pasting the result line for any pinned count.
- [x] Tests pin both wordings.
- [x] All four gates pass.

## Architecture references

- `executor/templates/executor_contract.md` — compiled in via `include_str!`
  (`executor/src/agent/contract.rs:5`).

## Phases

| #  | Phase                                                                | Status |
|----|----------------------------------------------------------------------|--------|
| 01 | contract-folds ([phase-01-contract-folds.md](phase-01-contract-folds.md)) | done        |

## Notes

- **Routing: local.** Text edit plus two tests.
- **Takes effect** only after the release binary is rebuilt and `serve`
  restarted; the first dispatch after that is the first real check.

## F09 retrospective

**Closed 2026-09-18 at one phase**, `approved_first_try`: zero bugs, 28 turns
on the local `RedHatAI/Qwen3.8-27B-INT4` (code `bfb8bc8`, approval `fc16ba3`).
Gates at close: 727 + 2 + 1210 passed. Mutation at review: the pre-phase
contract fails both new tests.

**Folds landed.** Both 3× counters from F08 are closed by contract text:
step 2 and "Resuming a phase" no longer ask the executor to name itself; step 8
and the completion checklist require pasting the result line for any pinned
count. The phase's own Summary already did so.

**Drafting caught three errors before dispatch**, all by dry-running the
replacements against a copy of the contract: a second "naming yourself" at
line 116, a checklist line number off by one, and a pinned phrase split
across a line break (which the test's `contains` would never match). The dry
run is the cheapest check this milestone had.

**Not live until rebuilt.** The contract is compiled in; the next dispatch
after the release binary is rebuilt and `serve` restarted is the first real
test of both folds.

**Calibration — held as data:**

- Executor skipped the spec's test-first step (write test 1, quote its
  failure): 1×.
- Architect edit to `NEXT.md` deleted adjacent text (the `## History` heading
  and intro) via a too-greedy split; caught by reading the diff before
  moving on: 1×.
