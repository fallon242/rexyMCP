# F15 — Dashboard session milestone

**Goal:** the dashboard names the milestone a session actually belongs to.

**Status:** done — opened and closed 2026-09-19. One phase,
`approved_first_try`.

**Depends on:** none.

**Source:** found while capturing the README screenshot. An F14 run showed as
"M46 — Token First Accounting", with M46's numbers in the Milestone column.
`milestone_number` (`mcp/src/dashboard/mod.rs:293`) accepts only `M<n>`, and
`resolve_milestone_dir` guesses the milestone from the bare phase id
(`phase-01`) by highest number — ambiguous even with `F` accepted, because
F and M numbers overlap. The session log records only the phase id.

**Exit criteria:**

- [x] Each session log records its phase doc path (`phase_doc` event).
- [x] The dashboard names the milestone from that path; old logs still fall
      back to the guess.
- [x] `F<n>-…` directories format as `F14 — Dash Count`.
- [x] All four gates pass.

## Phases

| #  | Phase                                                          | Status |
|----|----------------------------------------------------------------|--------|
| 01 | log-phase-doc ([phase-01-log-phase-doc.md](phase-01-log-phase-doc.md)) | done   |

## Notes

- **Routing: local.** No think tags involved.
- **Additive shape, chosen by count.** Adding a field to `SessionStart` would
  touch ~17 literal and pattern sites; a new `PhaseDoc` event touches 4
  exhaustive matches. `StatusSummary`'s 42 literals all use
  `..Default::default()`, so its new field is free.
- **Compatible both ways.** The installed binary reads a log containing the new
  event without error (checked 2026-09-19).
- **Unblocks** the `docs/rexymcp_dashboard.png` refresh.

## F15 retrospective

**Closed 2026-09-19 at one phase**, `approved_first_try`: 49 turns on the
local `RedHatAI/Qwen3.8-27B-INT4` (code `10f0bb8`, approval `a7ec227`). All 8
code files equal the spec byte for byte. Gates 734 + 2 + 1220.

**Found by a chore, not a report.** The bug surfaced while capturing the README
screenshot, which would otherwise have published a wrong milestone label.

**Design chosen by counting sites.** A new `PhaseDoc` event (4 exhaustive
matches) instead of a `SessionStart` field (~17 sites). The prototype also
caught a second order-sensitive test the architect's grep missed, and the
drafted end-to-end check failed on its first run (the JSON is pretty-printed;
the `grep` assumed compact) — both fixed before dispatch. Both catches came
from running the spec, not reading it.

**Calibration — held as data:**

- Executor places its started entry above the Update Log marker: **2×**
  (F12 phase-01, F15 phase-01).
- Architect end-to-end check that could not pass as written: 1× (caught at
  draft by running it).

**Takes effect after a rebuild and one new dispatch** — the dashboard needs a
session log that carries the `phase_doc` event. That unblocks the README
screenshot.
