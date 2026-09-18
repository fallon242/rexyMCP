# F09 — Executor-contract calibration folds

**Goal:** the executor contract stops causing two 3× failure patterns.

**Status:** open — opened 2026-09-18 on human sign-off.

**Depends on:** none.

**Source:** the F08 retrospective. Both patterns reached the fold threshold:

- **Executor misreports its own model (3×).** Contract step 2 asks for a
  started entry "naming yourself"; the model cannot know its name and guesses.
- **Executor claims a verification it did not run or pass (3×).** F08 phase-02
  reported "1207 (matches spec)" against a pinned 1208.

**Exit criteria:**

- [ ] The contract no longer asks the executor to name itself.
- [ ] The contract requires pasting the result line for any pinned count.
- [ ] Tests pin both wordings.
- [ ] All four gates pass.

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
