# F10 — Remove PII debug print

**Goal:** the privacy module never prints the contents of a PII dictionary.

**Status:** done — opened and closed 2026-09-18. One phase,
`approved_first_try`.

**Depends on:** none.

**Source:** F05 [bug-08-1](../F05-privacy-security-hardening/bugs/bug-08-1.md),
waived 2026-09-17 with the line left in the tree. `NEXT.md` has carried it as
an open item since.

**Exit criteria:**

- [x] `executor/src/privacy/` contains no `println!` / `eprintln!`.
- [x] All four gates pass with counts unchanged.

## Phases

| #  | Phase                                                                        | Status |
|----|------------------------------------------------------------------------------|--------|
| 01 | drop-debug-print ([phase-01-drop-debug-print.md](phase-01-drop-debug-print.md)) | done   |

## Notes

- **Routing: local only.** Privacy code.

## F10 retrospective

**Closed 2026-09-18 at one phase**, `approved_first_try`: 12 turns on the
local `RedHatAI/Qwen3.8-27B-INT4`, one deleted line (code `12eb0e4`, approval
`27fdba8`). Gates 727 + 2 + 1210, unchanged. The architect ran the `#[ignore]`d
live test against the engine at review: passed, no terms printed. F05
bug-08-1 is closed.

**First dispatch on the F09 contract.** The pinned-count fold held: the Summary
pasted all four `test result:` lines. The model-naming fold was not exercised:
the executor wrote no started entry at all.

**Calibration — held as data:**

- Executor writes no Update Log entries (no started entry, no start flip, no
  end-to-end entry): **2×** (F08 phase-02 round 1, F10 phase-01). Suspected
  cause: contract step 2 still says the started entry is "the only Update Log
  entry you write", which contradicts `WORKFLOW.md`'s required
  `(end-to-end verification)` entry. Listed as a candidate milestone.
