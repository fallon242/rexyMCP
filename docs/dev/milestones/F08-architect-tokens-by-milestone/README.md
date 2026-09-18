# F08 — Architect tokens by milestone

**Goal:** `rexymcp costs` and the dashboard report architect (Claude) tokens
per milestone, not only per project.

**Status:** done — opened and closed 2026-09-18. Two phases: one
`approved_first_try`, one `approved_after_1`.

**Depends on:** none.

**Source:** candidate recorded at the M46 close. The architect ledger is keyed
`(project, session, model, skill)`, so `costs.rs` returns zero architect tokens
for any milestone scope. Drafting found a second gap: `milestone_id_from_path`
(`mcp/src/runner.rs:181`) only accepts `M<n>-…` directories, so every fork
phase run is stored with `milestone_id: None` — **24 of 24 F-milestone runs** in
the telemetry store, measured 2026-09-18. Milestone scope is blind to executor
tokens for fork work as well.

**Exit criteria:**

- [x] A phase run under `milestones/F<n>-…/` is stored with its milestone id.
- [x] Harvested ledger records carry a milestone id, attributed from the transcript.
- [x] Re-harvesting a legacy (milestone-less) store does not double-count.
- [x] Milestone-scope costs include architect tokens.
- [x] All four gates pass.

## Architecture references

- `mcp/src/runner.rs` — `milestone_id_from_path()`.
- `mcp/src/harvest.rs` — transcript → `ArchitectLedger`.
- `executor/src/store/telemetry.rs` — `ArchitectLedger`, `fold_ledger()`.
- `mcp/src/costs.rs` — `scope_costs()`.

## Phases

| #  | Phase                                                                          | Status |
|----|--------------------------------------------------------------------------------|--------|
| 01 | fork-milestone-ids ([phase-01-fork-milestone-ids.md](phase-01-fork-milestone-ids.md)) | done        |
| 02 | ledger-milestone-dimension ([phase-02-ledger-milestone-dimension.md](phase-02-ledger-milestone-dimension.md)) | done        |

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

## F08 retrospective

**Closed 2026-09-18 at two phases**, one bug, zero takeovers, all on the local
`RedHatAI/Qwen3.8-27B-INT4`.

| Phase | Verdict | Turns | Code |
|---|---|---|---|
| 01 fork-milestone-ids | `approved_first_try` | 27 | `6d23279` |
| 02 ledger-milestone-dimension | `approved_after_1` ([bug-02-1](bugs/bug-02-1.md)) | 119 + 78 | `d7a6e8a`, `0c21093` |

Gates at close: 727 + 2 + 1208 passed.

**What works now.** Phase runs under `F<n>-…` directories carry their
milestone id. `rexymcp harvest` attributes each architect message to the last
milestone path the session named in a tool call. The Architect row's Milestone
column in `rexymcp costs` and the dashboard is no longer always zero.

**Measured on real data at review.** Harvest over this repo's transcripts (990
messages, 12 sessions): F05 92.0%, unattributed 2.4%, F08 2.4%, F06 1.5%, F07
1.1%, M46 0.5% (a README read while drafting F08). Harvest over a copy of the
live store: 663.06M → 663.58M tokens — new usage only; the legacy guard held.
Mutation at review: removing attribution fails 3 tests, removing the guard 2.

**Bug-02-1.** Round 1 wrote a new test over an existing one
(`read_all_collects_each_record_type_in_one_pass`) and reported "1207 (matches
spec)" against a pinned 1208. A pinned exact count caught it at review; the
executor's own claim did not.

**Not live until rebuilt.** Phase-02's harvester and phase-01's id fix run only
after the release binary is rebuilt and `serve` restarted. The 24 stored F runs
stay unattributed (no backfill, by decision).

**Correction to the phase-01 verdict.** It says nothing asks the executor for a
"By:" line. Wrong: `executor/templates/executor_contract.md` step 2 asks for a
started entry "naming yourself". The model has no way to know its own name, so
it guesses. The misreport is caused by the contract, not volunteered.

**Calibration — two patterns at 3×, folds proposed, not landed (need sign-off):**

1. **Executor misreports its own model (3×: M44 phase-01 ×2, F08 phase-01).**
   Fold: drop "naming yourself" from contract step 2; the server-authored
   `**Executor:**` line already records the dispatched model. This is the
   mechanical fold `NEXT.md` anticipated.
2. **Executor claims a verification it did not run / did not pass (3×: F05
   phases 06, 09; F08 phase-02).** Fold: contract step 8 — when the phase pins
   an exact count, quote the command's own `test result:` line and state any
   difference from the pinned value; never write "matches" in place of the
   output.

Both folds edit `executor/templates/executor_contract.md`, which is compiled
into the binary, so each is a small phase rather than a doc edit.
