# F12 — Telemetry unparsed count

**Goal:** a telemetry record that no longer parses is counted and reported,
not dropped silently.

**Status:** open — opened 2026-09-18 on human go-ahead.

**Depends on:** none.

**Source:** `NEXT.md` open item "Telemetry readers swallow schema mismatches
silently." A field rename or type change in `PhaseRun`, `ArchitectLedger`,
`ArchitectActivity` or `PhaseReview` would make every affected record vanish
from `costs`, the dashboard and the scorecard with no signal.

**Measured 2026-09-18 on the live store:** 11 864 lines, all at
`schema_version` 1, none malformed; the typed `PhaseRun` reader returns 323 of
323. **Nothing is being dropped today** — this is a canary for the next schema
change, not a repair.

**Exit criteria:**

- [ ] `read_all` counts current-version lines of a known type that fail to parse.
- [ ] `rexymcp costs` reads the store once via `read_all` and prints the count
      when it is non-zero.
- [ ] Old-schema and unknown-type lines are not counted.
- [ ] All four gates pass.

## Phases

| #  | Phase                                                                  | Status |
|----|------------------------------------------------------------------------|--------|
| 01 | count-unparsed ([phase-01-count-unparsed.md](phase-01-count-unparsed.md)) | todo   |

## Notes

- **Routing: local.** In-place edit in existing code.
- **Not in scope:** the four per-type readers (`read`, `read_reviews`,
  `read_architect_activities`, `read_architect_ledger`) keep their behaviour.
  A drift breaks them and `read_all` together, so `read_all`'s count is the
  canary. The dashboard surface is left for later.
