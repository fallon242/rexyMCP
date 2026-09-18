# F11 — Contract Update Log wording

**Goal:** the executor contract asks for the Update Log entries `WORKFLOW.md`
requires, instead of limiting the executor to one.

**Status:** done — opened and closed 2026-09-18. One phase,
`approved_first_try`.

**Depends on:** none.

**Source:** F10 retrospective. "Executor writes no Update Log entries" is at
2× (F08 phase-02 round 1, F10 phase-01). Contract step 2 calls the started
entry "the only Update Log entry you write" and step 7 repeats it, while steps
3–4 ask for progress and blocker entries and nothing asks for the
`(end-to-end verification)` entry `WORKFLOW.md` requires.

**Exit criteria:**

- [x] The contract no longer limits the executor to one Update Log entry.
- [x] The contract requires an `(end-to-end verification)` entry.
- [x] Tests pin both.
- [x] All four gates pass.

## Phases

| #  | Phase                                                                  | Status |
|----|------------------------------------------------------------------------|--------|
| 01 | log-entry-wording ([phase-01-log-entry-wording.md](phase-01-log-entry-wording.md)) | done   |

## Notes

- **Routing: local.** Text edit plus two tests.
- **Takes effect** only after a release rebuild and `serve` restart.

## F11 retrospective

**Closed 2026-09-18 at one phase**, `approved_first_try`: 43 turns on the
local `RedHatAI/Qwen3.8-27B-INT4` (code `90b57f8`, approval `015bfd7`).
Gates 727 + 2 + 1212. Mutation at review: the pre-phase contract fails both
new tests.

**Fixed before the fold threshold.** "Executor writes no Update Log entries"
was at 2×, not 3×; the milestone went ahead on the human's call because the
cause was a plain contradiction in the contract, not a behaviour to observe.
The counter stays open at 2× until a dispatch on the rebuilt binary shows
whether the new wording works.

**The phase's own run had every entry the contract now asks for** (started
entry with no model name, the test-first failure, a pasted end-to-end entry,
pasted `test result:` lines) — but it ran on the pre-F11 contract, so it is
not evidence for the change.

**Architect note.** The drafting dry run first reported every check as FAIL:
a Python chained comparison (`needle in c == want`) in the architect's own
checker. Caught because the result was implausible (the F09 phrases were known
present). Not counted as a calibration pattern.
