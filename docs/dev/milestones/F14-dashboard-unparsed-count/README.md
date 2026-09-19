# F14 — Dashboard unparsed count

**Goal:** the dashboard shows the unreadable-telemetry count that
`rexymcp costs` already prints (F12).

**Status:** open — opened 2026-09-19 on human go-ahead.

**Depends on:** F12 (`StoreRecords::unparsed`).

**Exit criteria:**

- [ ] The Budget panel shows `Unreadable telemetry: N` when N > 0, nothing at 0.
- [ ] `load_data` carries the count from `read_all`.
- [ ] All four gates pass.

## Phases

| #  | Phase                                                          | Status |
|----|----------------------------------------------------------------|--------|
| 01 | budget-warning ([phase-01-budget-warning.md](phase-01-budget-warning.md)) | in-progress |

## Notes

- **Routing: local.** Small in-place edit; no think tags involved.
- **First spec drafted under the mutation-check fold** (`WORKFLOW.md`):
  prototyped in a scratch worktree, and each of the three new tests was shown
  to fail when its piece of the fix was removed.
