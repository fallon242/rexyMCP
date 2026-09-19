# F16 — Log entry placement

**Goal:** the executor adds Update Log entries below the log marker.

**Status:** open — opened 2026-09-19 on human sign-off (fold at 2×).

**Depends on:** none.

**Source:** calibration counter "Executor places entries above the Update Log
marker", 2× (F12 phase-01, F15 phase-01). The contract says the log is
append-only but never names the `<!-- entries appended below this line -->`
marker.

**Exit criteria:**

- [ ] The contract says where new entries go.
- [ ] A test pins the wording.
- [ ] All four gates pass.

## Phases

| #  | Phase                                                              | Status |
|----|--------------------------------------------------------------------|--------|
| 01 | marker-rule ([phase-01-marker-rule.md](phase-01-marker-rule.md))   | in-progress   |

## Notes

- **Routing: local.** One contract line and one test.
- **Its run doubles as the fresh session log** the README screenshot needs
  (F15's `phase_doc` event).
