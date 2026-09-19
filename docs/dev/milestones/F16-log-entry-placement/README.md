# F16 — Log entry placement

**Goal:** the executor adds Update Log entries below the log marker.

**Status:** done — opened and closed 2026-09-19. One phase,
`approved_first_try`.

**Depends on:** none.

**Source:** calibration counter "Executor places entries above the Update Log
marker", 2× (F12 phase-01, F15 phase-01). The contract says the log is
append-only but never names the `<!-- entries appended below this line -->`
marker.

**Exit criteria:**

- [x] The contract says where new entries go.
- [x] A test pins the wording.
- [x] All four gates pass.

## Phases

| #  | Phase                                                              | Status |
|----|--------------------------------------------------------------------|--------|
| 01 | marker-rule ([phase-01-marker-rule.md](phase-01-marker-rule.md))   | done   |

## Notes

- **Routing: local.** One contract line and one test.
- **Its run doubles as the fresh session log** the README screenshot needs
  (F15's `phase_doc` event).

## F16 retrospective

**Closed 2026-09-19 at one phase**, `approved_first_try`: 32 turns on the
local `RedHatAI/Qwen3.8-27B-INT4` (code `93f3ea1`, approval `a58ab34`). Both
files equal the spec byte for byte. Gates 734 + 2 + 1221; `contract` 14.

**Fold landed at 2×** on human sign-off: the contract now names the marker and
says entries go below it. The run placed its own entries correctly even though
the phase doc quoted the marker four times inside code blocks.

**Doubled as F15's live check.** Its session log (`6aae1b61`) is the first
carrying the `phase_doc` event, which unblocks the README screenshot.

**Calibration:** started entry heading missing its `(started)` label — nit,
1×. The "entries above the marker" counter is closed; restart at 1× if it
recurs after the rebuild.
