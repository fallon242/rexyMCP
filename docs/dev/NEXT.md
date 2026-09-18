# NEXT — Active phase pointer

Single source of truth for which phase is active. The principal engineer
(architect) maintains this file; every session reads it (per `REXYMCP.md`
§ "Read these first") to know which phase to work next.

> **Numbering.** This fork's milestones carry an `F` prefix (`F01`…);
> upstream's keep `M`. The two numbering lines collided four times (two each of
> M43–M46) before the 2026-09-16 merge, so fork work was renumbered F01–F05 and
> upstream's M numbers were left alone. Never open a new `M` milestone here.

**Active milestone: none. Active phase: none.**

**F12 — Telemetry unparsed count: DONE 2026-09-18** at one phase,
`approved_first_try`. `read_all` counts unusable current-schema lines and
`rexymcp costs` prints them. Retrospective in
[F12/README.md](milestones/F12-telemetry-unparsed-count/README.md).
**Rebuild pending** for F12 to reach the installed `rexymcp`.

**Next milestone needs human sign-off before it opens.** Do not draft or
dispatch anything until the user names one.

**F11 — Contract Update Log wording: DONE 2026-09-18** at one phase,
`approved_first_try`. The contract now requires an `(end-to-end verification)`
entry and no longer calls the started entry the only one. Retrospective in
[F11/README.md](milestones/F11-contract-update-log-wording/README.md).
**Rebuilt 2026-09-18 13:51** with F11; `serve` restarted. The next dispatch
is the first live check of the F11 wording.

**F10 — Remove PII debug print: DONE 2026-09-18** at one phase,
`approved_first_try`. F05 bug-08-1 closed. Retrospective in
[F10/README.md](milestones/F10-remove-pii-debug-print/README.md).

**F09 — Executor-contract calibration folds: DONE 2026-09-18** at one phase,
`approved_first_try`. The contract no longer asks the executor to name itself
and requires pasted result lines for pinned counts. Retrospective in
[F09/README.md](milestones/F09-executor-contract-folds/README.md).

**Rebuilt 2026-09-18 13:28** with F08 and F09; `serve` restarted. The next
dispatch is the first live check of the F09 contract folds.

**F08 — Architect tokens by milestone: DONE 2026-09-18** at two phases (one
`approved_first_try`, one `approved_after_1`), all local. Milestone-scope costs
now include architect tokens and fork runs carry their milestone id. **Live
only after the release binary is rebuilt and `serve` restarted.** Retrospective
in [F08/README.md](milestones/F08-architect-tokens-by-milestone/README.md).

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
git worktree, and gate output still reaches the model. `max_turns` is 220 as of 2026-09-18. The NER engine is on port 8000,
model `RedHatAI/Qwen3.8-27B-INT4`.

## Open items

Live state only. Anything resolved has been removed — see § History.

- **`docs/rexymcp_dashboard.png` is a Jul 20 capture**, pre-M46. Regenerating it
  is a live-capture chore carried from the M46 close.
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
  **Partly closed by F12:** `read_all` now counts them and `rexymcp costs`
  prints the count. The four per-type readers (used by the scorecard,
  `runs`, `review`) stay silent by decision; `costs` is the canary.
- **Two open nits**, neither blocking: the `missing_spec_test` / broken-fixture
  failure shape (M37 phase-06), and the `$`-less `executor_val` debit (M38).
- **`generic-array` 0.14.9** was dropped as unreachable, not deferred:
  `crypto-common 0.1.7` pins `=0.14.7` and is the last release in its line.
  Reopening trigger is `cargo tree -i generic-array` showing a dependent that is
  not `crypto-common 0.1.x`.

## Calibration counters (below fold threshold)

Kept here because deleting them resets the clock. Fold at 3; see
`WORKFLOW.md` § Calibration. Two rows folded in F09 (2026-09-18) and were
removed: executor misreports its own model, and executor claims a verification
it did not run. If either recurs after the rebuild, restart it at 1× and note
the fold did not hold.

| Pattern | Count | Last seen |
|---|---|---|
| Architect E2E-block syntax errors | 2× | M46 |
| Executor undisclosed scope deviation | 1× | M46 |
| Environment failure looks like a bad spec | 1× | F05 phase 01 |
| Executor skips the spec's test-first step | 1× | F09 phase-01 |
| Architect edit deletes adjacent doc text | 1× | F09 open |
| Executor places entries above the Update Log marker | 1× | F12 phase-01 |

## Candidate milestones (none opened, none drafted)

- **PII guard false positives.** `plugin/hooks/pii-guard.sh` blocks harmless
  prompts: two epoch-ms values separated by a space, any 14+ digit run,
  `git@github.com` remotes, dotted 3-3-4 digits, and "tele…" (telemetry)
  next to a 9–10 digit number. Reproduced 2026-09-18; blocking the user in
  two terminals. Proposed: Luhn + bounded card match, whole-word `tel`, scan
  only the `prompt` field, name the matched pattern. Local-only (privacy code).

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
