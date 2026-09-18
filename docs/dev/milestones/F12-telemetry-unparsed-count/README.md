# F12 — Telemetry unparsed count

**Goal:** a telemetry record that no longer parses is counted and reported,
not dropped silently.

**Status:** done — opened and closed 2026-09-18. One phase,
`approved_first_try`.

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

- [x] `read_all` counts current-version lines of a known type that fail to parse.
- [x] `rexymcp costs` reads the store once via `read_all` and prints the count
      when it is non-zero.
- [x] Old-schema and unknown-type lines are not counted.
- [x] All four gates pass.

## Phases

| #  | Phase                                                                  | Status |
|----|------------------------------------------------------------------------|--------|
| 01 | count-unparsed ([phase-01-count-unparsed.md](phase-01-count-unparsed.md)) | done   |

## Notes

- **Routing: local.** In-place edit in existing code.
- **Not in scope:** the four per-type readers (`read`, `read_reviews`,
  `read_architect_activities`, `read_architect_ledger`) keep their behaviour.
  A drift breaks them and `read_all` together, so `read_all`'s count is the
  canary. The dashboard surface is left for later.

## F12 retrospective

**Closed 2026-09-18 at one phase**, `approved_first_try`: 78 turns on the
local `RedHatAI/Qwen3.8-27B-INT4` (code `a73e8c2`, approval `22f19e6`).
Gates 729 + 2 + 1218. `rexymcp costs` shows no unreadable records on the live
store and `Unreadable telemetry records: 2` on a copy with two bad lines.
Mutation at review: dropping the ledger arm's count fails a test.

**Spec applied in a scratch worktree before dispatch** — clippy, fmt and the
existing suite passed on the architect's own application of every code block,
and the live-store run showed 0 unparsed. The executor matched it; no bounce.

**F11 fold held on its first live run:** started, progress and
`(end-to-end verification)` entries present, no model named, pasted
`test result:` lines. The "no Update Log entries" counter is closed.

**Calibration — held as data:**

- Executor placed its entries above the `<!-- entries appended below this
  line -->` marker: 1×.
- Executor Summary misattributed a spec gotcha to the wrong file: 1× (no code
  impact).
