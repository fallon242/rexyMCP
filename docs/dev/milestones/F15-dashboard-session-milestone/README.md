# F15 — Dashboard session milestone

**Goal:** the dashboard names the milestone a session actually belongs to.

**Status:** open — opened 2026-09-19 on human go-ahead.

**Depends on:** none.

**Source:** found while capturing the README screenshot. An F14 run showed as
"M46 — Token First Accounting", with M46's numbers in the Milestone column.
`milestone_number` (`mcp/src/dashboard/mod.rs:293`) accepts only `M<n>`, and
`resolve_milestone_dir` guesses the milestone from the bare phase id
(`phase-01`) by highest number — ambiguous even with `F` accepted, because
F and M numbers overlap. The session log records only the phase id.

**Exit criteria:**

- [ ] Each session log records its phase doc path (`phase_doc` event).
- [ ] The dashboard names the milestone from that path; old logs still fall
      back to the guess.
- [ ] `F<n>-…` directories format as `F14 — Dash Count`.
- [ ] All four gates pass.

## Phases

| #  | Phase                                                          | Status |
|----|----------------------------------------------------------------|--------|
| 01 | log-phase-doc ([phase-01-log-phase-doc.md](phase-01-log-phase-doc.md)) | in-progress   |

## Notes

- **Routing: local.** No think tags involved.
- **Additive shape, chosen by count.** Adding a field to `SessionStart` would
  touch ~17 literal and pattern sites; a new `PhaseDoc` event touches 4
  exhaustive matches. `StatusSummary`'s 42 literals all use
  `..Default::default()`, so its new field is free.
- **Compatible both ways.** The installed binary reads a log containing the new
  event without error (checked 2026-09-19).
- **Unblocks** the `docs/rexymcp_dashboard.png` refresh.
