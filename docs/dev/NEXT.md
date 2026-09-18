# NEXT — Active phase pointer

Single source of truth for which phase is active. The principal engineer
(architect) maintains this file; every session reads it (per `REXYMCP.md`
§ "Read these first") to know which phase to work next.

> **Numbering.** This fork's milestones carry an `F` prefix (`F01`…);
> upstream's keep `M`. The two numbering lines collided four times (two each of
> M43–M46) before the 2026-09-16 merge, so fork work was renumbered F01–F05 and
> upstream's M numbers were left alone. Never open a new `M` milestone here.

**Active milestone: F08 — Architect tokens by milestone** ([README](milestones/F08-architect-tokens-by-milestone/README.md)), opened 2026-09-18 on human go-ahead.
**Active phase: [phase-01-fork-milestone-ids](milestones/F08-architect-tokens-by-milestone/phase-01-fork-milestone-ids.md)** — `todo`, route local.

**F07 — Completion-entry date: DONE 2026-09-18** at one phase,
`approved_first_try`, 54 local turns. Server completion entries now head
themselves `YYYY-MM-DD HH:MM` (UTC). Takes effect once `serve` is rebuilt.
Retrospective in [F07/README.md](milestones/F07-completion-entry-date/README.md).

**F05 — Privacy and security hardening: DONE 2026-09-18** at eleven phases, all
eleven findings closed. Six `approved_first_try`, five `approved_after_1`, five
bugs filed (four fixed, bug-08-1 waived), zero takeovers. Retrospective in
[F05/README.md § F05 retrospective](milestones/F05-privacy-security-hardening/README.md).
**Two folds landed in `WORKFLOW.md` on 2026-09-18, on human sign-off:**
(1) § "Verify the mechanism before you pin a test to it" (3×, all architect
error); (2) a "count the construction sites" block in § "Prefer additive change
shapes" (1× that cost a 200-turn budget). Both are mirrored into
`plugin/templates/WORKFLOW.md`.

**Executor routing (2026-09-18).** Local first. `deepseek-flash` scored 30/30 on
a greenfield benchmark but `hard_fail`ed on phase 05, inventing an unrelated
edit and truncating a comment mid-word; the local model finished the same phase
in 89 turns. Route cloud work to greenfield or additive phases; keep in-place
edits in dense existing code local. Cloud goes through the CLI
(`REXYMCP_API_KEY=$DEEPSEEK_API_KEY REXYMCP_BASE_URL=https://api.deepseek.com/v1 REXYMCP_MODEL=deepseek-flash rexymcp run-phase …`);
the pre-scan index is cached now. A cloud executor still cannot commit inside a
git worktree, and gate output still reaches the model. A debug `eprintln!` of
the PII dictionary remains in `executor/src/privacy/egress.rs` (bug-08-1,
waived). `max_turns` is 220 as of 2026-09-18. The NER engine is on port 8000,
model `RedHatAI/Qwen3.8-27B-INT4`.

## Open items

Live state only. Anything resolved has been removed — see § History.

- **`docs/rexymcp_dashboard.png` is a Jul 20 capture**, pre-M46. Regenerating it
  is a live-capture chore carried from the M46 close.
- **The waived `eprintln!` of the PII dictionary** in
  `executor/src/privacy/egress.rs` ([bug-08-1](milestones/F05-privacy-security-hardening/bugs/bug-08-1.md)).
  Delete it in the next phase that touches that file.
- **F01 is parked, not blocked** — it is not a prerequisite for anything, and
  `deny_unknown_fields` on `ModelOverride` does not gate F02's `[privacy]`
  config. **Before landing it, reconcile its phase-01 doc:** it claims
  `ModelOverride` has no `thinking` field, but `a2fdbe2` merged
  `pub thinking: Option<String>`, so its end-to-end step (expecting
  `thinking = "disabled"` to be rejected) would now fail.
- **Telemetry readers swallow schema mismatches silently.** The
  `filter_map(|l| serde_json::from_str::<Value>(l).ok())` pairs in
  `executor/src/store/telemetry.rs` (lines 244/249, 429/434, 548/553, 647/652,
  verified 2026-09-18) drop both malformed lines *and* records whose schema no
  longer matches, so a future field rename or type change goes equally quiet.
  Named rather than scheduled: if a numbers discrepancy ever appears with no
  obvious cause, look here first.
- **Two open nits**, neither blocking: the `missing_spec_test` / broken-fixture
  failure shape (M37 phase-06), and the `$`-less `executor_val` debit (M38).
- **`generic-array` 0.14.9** was dropped as unreachable, not deferred:
  `crypto-common 0.1.7` pins `=0.14.7` and is the last release in its line.
  Reopening trigger is `cargo tree -i generic-array` showing a dependent that is
  not `crypto-common 0.1.x`.

## Calibration counters (below fold threshold)

Kept here because deleting them resets the clock. Fold at 3; see
`WORKFLOW.md` § Calibration. The model-misreport fold is already known and
mechanical if it recurs: stop asking the executor to write that field and let
the server own it, as it already owns the completion tail.

| Pattern | Count | Last seen |
|---|---|---|
| Executor claims a verification it did not run | 2× | F05 phases 06, 09 |
| Architect E2E-block syntax errors | 2× | M46 |
| Executor undisclosed scope deviation | 1× | M46 |
| Environment failure looks like a bad spec | 1× | F05 phase 01 |
| Executor misreports its own model in its Update Log | 2× | M44 phase-01 |

## Candidate milestones (none opened, none drafted)

- None listed.

## History

This file used to carry every closed milestone's pointer blocks back to M35 —
4 437 lines, re-read every session, and internally contradictory by the end (it
simultaneously called M37 "OPEN, active" with all six phases `done`, and listed
two M38 folds as "NOT landed" fifteen lines below a note saying both had
landed). Trimmed 2026-09-18.

**Where history lives now:**

- **Per-milestone retrospectives** — `docs/dev/milestones/<id>/README.md`. This
  is the durable record; every closed milestone has one.
- **Per-phase verdicts** — the Review verdict block at the bottom of each phase
  doc's Update Log.
- **Telemetry** — `rexymcp runs` / `rexymcp scorecard` over the `PhaseRun` store.
- **The full pre-trim file** — `git show f26f519:docs/dev/NEXT.md`, or
  `git log -p docs/dev/NEXT.md` for how it got that way.

**Keep it this size.** A closed milestone gets one line here at most, and only
while something about it is still live; the retrospective is the record.
