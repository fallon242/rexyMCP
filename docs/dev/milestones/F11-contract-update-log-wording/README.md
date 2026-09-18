# F11 — Contract Update Log wording

**Goal:** the executor contract asks for the Update Log entries `WORKFLOW.md`
requires, instead of limiting the executor to one.

**Status:** open — opened 2026-09-18 on human go-ahead.

**Depends on:** none.

**Source:** F10 retrospective. "Executor writes no Update Log entries" is at
2× (F08 phase-02 round 1, F10 phase-01). Contract step 2 calls the started
entry "the only Update Log entry you write" and step 7 repeats it, while steps
3–4 ask for progress and blocker entries and nothing asks for the
`(end-to-end verification)` entry `WORKFLOW.md` requires.

**Exit criteria:**

- [ ] The contract no longer limits the executor to one Update Log entry.
- [ ] The contract requires an `(end-to-end verification)` entry.
- [ ] Tests pin both.
- [ ] All four gates pass.

## Phases

| #  | Phase                                                                  | Status |
|----|------------------------------------------------------------------------|--------|
| 01 | log-entry-wording ([phase-01-log-entry-wording.md](phase-01-log-entry-wording.md)) | todo   |

## Notes

- **Routing: local.** Text edit plus two tests.
- **Takes effect** only after a release rebuild and `serve` restart.
