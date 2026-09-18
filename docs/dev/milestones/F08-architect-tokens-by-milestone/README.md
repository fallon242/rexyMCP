# F08 — Architect tokens by milestone

**Goal:** `rexymcp costs` and the dashboard report architect (Claude) tokens
per milestone, not only per project.

**Status:** open — opened 2026-09-18 on human go-ahead.

**Depends on:** none.

**Source:** candidate recorded at the M46 close. The architect ledger is keyed
`(project, session, model, skill)`, so `costs.rs` returns zero architect tokens
for any milestone scope. Drafting found a second gap: `milestone_id_from_path`
(`mcp/src/runner.rs:181`) only accepts `M<n>-…` directories, so every fork
phase run is stored with `milestone_id: None` — **24 of 24 F-milestone runs** in
the telemetry store, measured 2026-09-18. Milestone scope is blind to executor
tokens for fork work as well.

**Exit criteria:**

- [ ] A phase run under `milestones/F<n>-…/` is stored with its milestone id.
- [ ] Harvested ledger records carry a milestone id, attributed from the transcript.
- [ ] Re-harvesting a legacy (milestone-less) store does not double-count.
- [ ] Milestone-scope costs include architect tokens.
- [ ] All four gates pass.

## Architecture references

- `mcp/src/runner.rs` — `milestone_id_from_path()`.
- `mcp/src/harvest.rs` — transcript → `ArchitectLedger`.
- `executor/src/store/telemetry.rs` — `ArchitectLedger`, `fold_ledger()`.
- `mcp/src/costs.rs` — `scope_costs()`.

## Phases

| #  | Phase                                                                          | Status |
|----|--------------------------------------------------------------------------------|--------|
| 01 | fork-milestone-ids ([phase-01-fork-milestone-ids.md](phase-01-fork-milestone-ids.md)) | review      |
| 02 | ledger-milestone-dimension (not drafted)                                       | —      |

## Notes

- **Phase 02 design (drafted after phase 01 lands; it reuses the fixed
  function).** Attribution is read from the transcript itself: a session's
  current milestone is the last `milestones/<slug>/` path named in an assistant
  `tool_use` input, where `<slug>` passes `milestone_id_from_path`. A single
  input that names two or more distinct slugs (a grep across milestones) does
  not change it. Messages before the first mention stay unattributed (`None`).
  Approximate by design: reading an old milestone's doc mid-milestone
  misattributes until the next mention.
- **Legacy double-count guard.** Old ledger records have no milestone and fold
  under the `None` key. Harvest must emit a `None` record for every
  `(session, model, skill)` it sees — zero-valued if every message was
  attributed — so the fresh record replaces the legacy full-sum one.
- **No backfill of the 24 historical F runs.** The store is append-only; those
  runs stay unattributed. Revisit only if a retroactive F-milestone view is
  wanted.
- **Routing: local** for both phases (in-place edits in existing code).
