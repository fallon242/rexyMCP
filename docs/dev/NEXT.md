# NEXT — Active phase pointer

Single source of truth for which phase is active. The principal engineer
(architect) maintains this file; every session reads it (per `REXYMCP.md`
§ "Read these first") to know which phase to work next.

> **Numbering.** This fork's milestones carry an `F` prefix (`F01`…);
> upstream's keep `M`. The two numbering lines collided four times (two each of
> M43–M46) before the 2026-09-16 merge, so fork work was renumbered F01–F05 and
> upstream's M numbers were left alone. Never open a new `M` milestone here.

**Active milestone: none. Active phase: none.**

**F05 — Privacy and security hardening: DONE 2026-09-18** at eleven phases, all
eleven findings closed. Six `approved_first_try`, five `approved_after_1`, five
bugs filed (four fixed, bug-08-1 waived), zero takeovers. Retrospective in
[F05/README.md § F05 retrospective](milestones/F05-privacy-security-hardening/README.md).
**Two folds landed in `WORKFLOW.md` on 2026-09-18, on human sign-off:**
(1) § "Verify the mechanism before you pin a test to it" (3×, all architect
error); (2) a "count the construction sites" block in § "Prefer additive change
shapes" (1× that cost a 200-turn budget). Both are mirrored into
`plugin/templates/WORKFLOW.md`.

**Next milestone needs human sign-off before it opens.** Do not draft or
dispatch anything until the user names one.

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
- **Server-authored completion entries head themselves `ts=<epoch-ms>`** instead
  of the `WORKFLOW.md` date format. **4 occurrences — at threshold**, and the
  fix is a runtime change, so it needs a human go-ahead before anyone drafts it.
- **The waived `eprintln!` of the PII dictionary** in
  `executor/src/privacy/egress.rs` ([bug-08-1](milestones/F05-privacy-security-hardening/bugs/bug-08-1.md)).
  Delete it in the next phase that touches that file.
- **F01 is parked, not blocked** — it is not a prerequisite for anything, and
  `deny_unknown_fields` on `ModelOverride` does not gate F02's `[privacy]`
  config. **Before landing it, reconcile its phase-01 doc:** it claims
  `ModelOverride` has no `thinking` field, but `a2fdbe2` merged
  `pub thinking: Option<String>`, so its end-to-end step (expecting
  `thinking = "disabled"` to be rejected) would now fail.
- **`generic-array` 0.14.9** was dropped as unreachable, not deferred:
  `crypto-common 0.1.7` pins `=0.14.7` and is the last release in its line.
  Reopening trigger is `cargo tree -i generic-array` showing a dependent that is
  not `crypto-common 0.1.x`.

## Calibration counters (below fold threshold)

Kept here because deleting them resets the clock. Fold at 3; see
`WORKFLOW.md` § Calibration.

| Pattern | Count | Last seen |
|---|---|---|
| Executor claims a verification it did not run | 2× | F05 phases 06, 09 |
| Architect E2E-block syntax errors | 2× | M46 |
| Executor undisclosed scope deviation | 1× | M46 |
| Environment failure looks like a bad spec | 1× | F05 phase 01 |

## Candidate milestones (none opened, none drafted)

- The `ts=<epoch-ms>` completion-entry server fix (see § Open items).
- Architect tokens-by-milestone attribution — needs a milestone dimension on
  the ledger.

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
