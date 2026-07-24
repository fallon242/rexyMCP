# M42 — Bookkeeping Format Hygiene

**Goal:** Make the server-authored bookkeeping tail — the completion Update Log
entry and the milestone README status row — **well-formed markdown**, so a
completed phase stops arriving at review with a failing `format` gate the executor
never caused.

**Status:** done *(opened and closed 2026-07-24; phase 01 done and approved,
phase 02 not planned per the decision below. GitHub issue #4 closed on the
strength of the code, with the reporter reopening if Prettier still complains —
see § "Closing without the live confirmation".)*

**Depends on:** M27 phase-03 (the server-authored finalize this fixes), M32 (the
`flip_readme_row` slice fix this builds on).

## Why this milestone exists

GitHub issue [#4](https://github.com/ryanczak/rexyMCP/issues/4). Every completed
phase writes bookkeeping that its own `format` gate then rejects:

```
$ bunx prettier --check .
[warn] docs/dev/milestones/M1-foundations/phase-01-scaffold.md
[warn] docs/dev/milestones/M1-foundations/README.md
$ bunx prettier --check src/ test/
All matched files use Prettier code style!
```

The executor's output is clean; the **server's** writes are not. `format` is a
hard DoD gate (STANDARDS §1), so the reviewer must either bounce the executor for
a defect it did not cause — burning a dispatch and writing a false `bounced`
datapoint into the scorecard that routing decisions read — or hand-normalize at
every approval, which is what has actually been happening, silently.

The reporter notes this fired on **every phase** of an earlier milestone (≥3
occurrences, past the fold-immediately threshold) and was papered over with a
reviewer stopgap. It then reproduced on the first phase of a deliberately fresh
0.9.1 run, so it is a live defect and not stale project state.

### Confirmed still present in the current tree

Each claim checked against the code rather than inferred (M41 touched none of
this — different file, different failure):

| Defect | Site | Why |
|---|---|---|
| No blank line before the appended `###` heading | `finalize.rs:174-176` | `format!("{}\n{}\n", doc.trim_end(), entry)` — a single `\n` between the prior line and the entry |
| Trailing blank line at EOF | same | `entry` already ends in `\n`; the format adds another |
| No blank line before the files-changed list | `finalize.rs:118` | `**Files changed:**\n{files}` — a list must be preceded by a blank line |
| README table cell not re-padded | `finalize.rs:196-201` | the cell is rewritten as `"{} review \|{}"`, a fixed width regardless of the column's |
| README left with no trailing newline | `finalize.rs:216` | `lines.join("\n")` — `.lines()` strips the final newline and `join` never restores it |
| `format_fix` never runs on the server's writes | absent from `finalize.rs` / `runner.rs` | the hook (`executor/src/agent/command.rs:182`) is per-turn, and the server writes after the executor's final turn |

## The split, and its limit

**Phase 01 makes the output well-formed.** Pure string fixes with hermetic tests,
no config and no command execution. This is the fix that works for **every**
project, including those with `format_fix` unset (where the hook is inert by
design) — and it is the one that makes the generated markdown correct rather than
merely laundered.

**Phase 02 runs the project's `format_fix` over the server's writes** before the
bookkeeping commit, as the issue proposes. That is the belt to phase 01's braces,
and it is what absorbs formatter conventions we cannot predict.

**Phase 01 deliberately does not chase byte-identity with any particular
formatter.** Prettier normalizes a table's column widths to its widest cell, so a
`todo` → `review` flip can legitimately change the whole table's shape — including
the header separator row, as the issue's diff shows. Reproducing that in
`flip_readme_row` would mean writing a table-aware markdown formatter, which is
scope invention. Phase 01's target is a **rectangular, well-formed** table whose
column widths are unchanged from what the formatter already accepted; closing the
last gap to formatter-idempotence is exactly phase 02's job.

## ⚠️ Pre-dispatch decision required for phase 02

`format_fix` is a whole-repo command string (`bunx prettier --write .`), and
nothing in it can be scoped to two paths without parsing the command — which is
not something rexyMCP should do. So phase 02 has a fork the **human** must settle,
not the architect:

- **(a) Run `format_fix` as configured, whole-repo**, then re-stage the two doc
  paths. Simple and matches the existing per-turn hook exactly. Risk: it reformats
  files outside the phase's scope at commit time — the same hazard that makes
  `cargo fmt --all` a standing prohibition in this very repo (`REXYMCP.md`
  § Commands).
- **(b) Only format when the project opts in** via a new config key (e.g.
  `[commands] format_fix_docs`), leaving (a)'s behavior off by default. Safer,
  costs a config surface.
- **(c) Ship phase 01 only** and close the milestone, treating any residual
  formatter drift as acceptable. Defensible if phase 01 turns out to fully satisfy
  Prettier in practice.

Recommendation: **(c) pending evidence, then (b) if needed.** Phase 01 is the real
fix; run it against the reporter's project first and see whether anything is left
over. Do not dispatch phase 02 before this is settled.

### ✅ Decided 2026-07-24: (c) — ship phase 01 only

The human chose **(c)**. Phase 02 is **not planned** and will not be dispatched:
no `format_fix` runs at finalize time, and no new config key is added. The
generated markdown is correct on its own, which is the property that holds for
every project including those with no formatter configured — laundering
well-formed output through a formatter buys nothing, and buying it would cost
either a whole-repo write at commit time or a config surface to maintain.

**Settled — do not re-litigate** absent new evidence. The evidence that would
reopen it is specific: a project whose `format` gate still flags the phase doc or
milestone README **after** phase 01 is live. If that appears, reopen with the
formatter's actual complaint attached and implement option (b), whose spec is
already written in `phase-02-format-server-writes.md`.

## Exit criteria

- The appended completion entry is separated from prior content by exactly one
  blank line, and the file ends with exactly one newline.
- The files-changed list is preceded by a blank line.
- `flip_readme_row` preserves the status column's width and the file's trailing
  newline.
- A round-trip over a realistic phase doc + milestone README fixture produces
  output with no missing blank lines, no trailing blank line, and an unchanged
  table shape — pinned by tests that fail against today's code.
- All four gates green.
- ~~**Reviewer-run, after a `serve` rebuild:** the next real phase's bookkeeping
  tail shows the blank line before `### Update` and a single trailing newline.~~
  **Waived at close** — see below. Still worth an eyeball on the next dispatch,
  but no longer gating.

## Architecture references

- `mcp/src/finalize.rs` — the whole milestone: `append_entry:174`,
  `baseline_entry:100`, `flip_readme_row:182`.
- `mcp/src/runner.rs:316-327` — where finalize is invoked, and where phase 02 would
  thread `format_fix` through `FinalizeInput`.
- `executor/src/agent/command.rs:182` — `run_post_write_hooks`, the per-turn hook
  phase 02 would mirror.
- `docs/architecture.md` § Status #27 — the server-authored finalize contract.

## Phases

| #  | Phase | Status |
|----|-------|--------|
| 01 | Well-formed bookkeeping output ([phase-01-wellformed-bookkeeping.md](phase-01-wellformed-bookkeeping.md)) — approved_first_try; reviewer hardened 3 weakened assertions | done |
| 02 | Format the server's own writes ([phase-02-format-server-writes.md](phase-02-format-server-writes.md)) — **not planned**; decision (c) taken 2026-07-24, spec kept for a possible reopen | not planned |

## Notes

**Phase 01 is also the live test vehicle for issue #5.** It is being dispatched to
the local executor specifically to exercise the M41 fixes end-to-end: a real run
reaching terminal state, `get_run_status` reaping it, and a terminal record landing
in `~/.rexymcp/runs/`. That is the live verification M41 phase-03 left outstanding.
The phase's own value is independent of that — it would be worth doing regardless —
but the choice to dispatch it rather than implement it directly is deliberate.

**The fix cannot verify itself.** The bookkeeping tail for phase 01's own run is
written by the **currently running** `serve`, i.e. the pre-fix binary. So phase
01's own completion entry will still show the defect. That is expected, not a
failure, and it is why the exit criteria put the live confirmation on the *next*
phase after a rebuild.

## Closing without the live confirmation (2026-07-24)

The milestone closed with its live criterion **unmet by design**, and that is worth
being explicit about rather than quietly dropping.

What is verified: ten exact-equality tests, a mutation check on each of the four
production changes, and the fixed template present in the installed binary
(`**Files changed:**\n\n`). What is **not**: a real completed phase on a Prettier
project producing a clean `prettier --check`. This repo's `format` gate is
`cargo fmt`, which does not check markdown — so this project structurally cannot
adjudicate the reporter's symptom. Waiting for a local dispatch would have produced
a *weaker* signal than the reporter simply reopening.

So the human closed issue #4 with an explicit invitation to reopen with the
`prettier --check` output attached, and the opt-in formatter hook
(`phase-02-format-server-writes.md`) is the ready answer if it does. **The right
lesson is not "skip live verification" — it is that a verification this repo cannot
perform is better delegated to the environment that can, with a stated reopening
condition, than simulated locally and called done.**

The next dispatch's tail is still worth an eyeball; it just is not blocking.
