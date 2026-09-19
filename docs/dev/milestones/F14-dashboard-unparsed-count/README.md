# F14 — Dashboard unparsed count

**Goal:** the dashboard shows the unreadable-telemetry count that
`rexymcp costs` already prints (F12).

**Status:** done — opened and closed 2026-09-19. One phase,
`approved_first_try`.

**Depends on:** F12 (`StoreRecords::unparsed`).

**Exit criteria:**

- [x] The Budget panel shows `Unreadable telemetry: N` when N > 0, nothing at 0.
- [x] `load_data` carries the count from `read_all`.
- [x] All four gates pass.

## Phases

| #  | Phase                                                          | Status |
|----|----------------------------------------------------------------|--------|
| 01 | budget-warning ([phase-01-budget-warning.md](phase-01-budget-warning.md)) | done   |

## Notes

- **Routing: local.** Small in-place edit; no think tags involved.
- **First spec drafted under the mutation-check fold** (`WORKFLOW.md`):
  prototyped in a scratch worktree, and each of the three new tests was shown
  to fail when its piece of the fix was removed.

## F14 retrospective

**Closed 2026-09-19 at one phase**, `approved_first_try`: 35 turns on the
local `RedHatAI/Qwen3.8-27B-INT4` (code `3ed61b9`, approval `cbbedc4`). The
committed code equals the spec byte for byte. Gates 732 + 2 + 1220;
`dashboard::` 183.

**First milestone under the mutation-check fold.** Every new test was shown
red at draft with its piece of the fix removed, and again at review. No bounce,
and the fastest run on this code today.

**Doc defect, fixed at review.** The executor's line-number patch overwrote the
header's `**Depends on:** none` with a second `**Status:**` line and left a
3-line stray fragment after its end-to-end entry. Header restored; the fragment
stays (append-only log). Held as data, 1×.
