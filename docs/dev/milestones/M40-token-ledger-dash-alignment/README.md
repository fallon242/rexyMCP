# M40 — Token-ledger Dash Alignment

**Goal:** In the Budget ledger's **tokens** mode (`b` toggle in the dashboard;
`rexymcp costs --tokens` on the CLI), align each `—` dash on the decimal column of
the `X.Xk`/`X.XM` values above and below it, matching what M37 phase-06 did for
dollars mode.

**Status:** done *(opened 2026-07-24; closed 2026-07-24)*

**Depends on:** M37 phase-06 (the dollars-mode sign-gutter convention this mirrors),
M38 (the shared `ledger_lines` renderer), M35 phase-03 (`metrics::fmt_tokens`).

## Why this milestone exists

Found in use on the dashboard. In tokens mode the `—` dashes sit one decimal-width
(2 columns) to the **right** of the `.` in the token values above/below them:

```
Tokens         Session Milestone   Project
  Architect:         —         —   2395.2M     <- — @ cols 21, 31 ; . @ col 39
  Executor:      90.2k    733.9k    288.5M     <- . @ cols 19, 29, 39
  Net:               —         —         —     <- — @ cols 21, 31, 41
```

Measured against the live `costs --tokens` output: decimals land at cols
**19/29/39**, dashes at **21/31/41** — a uniform 2-column offset in every scope.

**Root cause.** Token cells render via `metrics::fmt_tokens`, whose zero value is a
**bare `"—"`** (`costs.rs` tokens branch, Architect/Executor cells), and the Net
row uses a bare `"—"` literal. A bare em-dash right-aligns to the field's right
edge; a `X.Xk`/`X.XM` value keeps its decimal 2 columns in (`.` + one digit + a
`k`/`M` suffix). So the dash sits 2 columns right of where the decimals line up.

**Why this slipped M37 phase-06.** Phase-06 fixed the *dollars*-mode debit/credit
decimal alignment and explicitly wrote: *"Tokens mode is unaffected — token cells
(`fmt_tokens`) are never parenthesised, so they are already internally consistent.
Do not add a gutter in tokens mode."* That was the miss — it reasoned about the
*paren* problem (which tokens mode indeed lacks) and never checked the
*dash-vs-decimal* case, which tokens mode does have.

## Exit criteria

- In tokens mode, the `—` markers of the Architect, Executor, and Net rows sit at
  the **same column index** as the `.` in the `X.Xk`/`X.XM` token values in each
  scope — pinned by a real marker-column-equality test (dash col == decimal col),
  the tokens-mode analogue of phase-06's dollars test.
- `metrics::fmt_tokens` is **unchanged** — the padding is applied at the
  `ledger_lines` render level only. `scorecard` / `runs` / `calibrate-governor`,
  which also call `fmt_tokens`, keep their bare `—` (a negative check: those call
  sites' output is not altered).
- Dollars mode is untouched (phase-06's alignment holds).
- All four gates green.

## Architecture references

- `docs/architecture.md` § Status #40 — this milestone's summary.
- `docs/dev/milestones/M37-governor-read-only-calibration/phase-06-budget-panel-alignment.md`
  — the dollars-mode sign-gutter this mirrors, and the scoping miss it made.
- `mcp/src/costs.rs` — `ledger_lines` tokens branch (`LedgerUnits::Tokens`),
  `make_row`.
- `executor/src/store/metrics.rs` — `fmt_tokens` (shared; must NOT change).

## Phases

| #  | Phase | Status |
|----|-------|--------|
| 01 | Pad tokens-mode dashes to the decimal column + marker-equality test | done (direct architect fix) |

Single-phase milestone. The fix is ~5 lines in the `ledger_lines` tokens branch
plus a tokens-mode alignment test; the dashboard Budget panel and the CLI `costs
--tokens` share `ledger_lines`, so both fix together.

**Implemented directly by the architect (user-authorized), not dispatched.** No
phase doc was drafted and no `PhaseRun` exists — the fix was ~5 lines in the exact
spot the executor stumbled on in M37 phase-06 (broke its own test fixtures, needed
a takeover), so the user chose the direct path for this cosmetic change. Commit
`c942fd3`.

## Notes

## M40 completion (2026-07-24)

**Fixed directly, verified independently.** The `ledger_lines` tokens branch now
routes zero/Net cells through a local `tok`/`TOK_DASH` that pads the em-dash to
`"—  "`, landing it on the decimal column. Gates all green (`cargo build`,
`clippy -D warnings`, `rustfmt --check`, `cargo test` 658 bin + 1050 lib).
**Mutation check:** reverting `TOK_DASH` to a bare `"—"` fails the new
`ledger_tokens_dash_aligns_with_decimal_column` (dash col 20 vs decimal col 18);
restored. **E2E** (`costs --tokens`): all markers now land at cols 19/29/39 per
scope (Architect dashes, Executor decimals, Net dashes), and dollars mode is
unchanged (M37 phase-06 alignment holds). `fmt_tokens` was not touched, so
`scorecard`/`runs`/`calibrate-governor` keep their bare `—`.

**Retrospective — the one-line lesson.** M37 phase-06 declared tokens mode "already
consistent" by reasoning about the *paren* problem it lacked, without checking the
*dash-vs-decimal* problem it had. A negative-scope claim ("mode X is unaffected")
deserves the same falsifiable check as a positive one — a quick `costs --tokens`
glance would have caught it then. This is the second alignment miss in this
renderer family (phase-06's fake width test was the first); both are "a claim
asserted without the check that would refute it." Still recorded, not folded — the
phase-06 fake-test note and this are related but distinct sub-forms; watch the
pair.

**Bare-value edge case (called out, not fixed — out of scope).** Token
counts under 1000 format bare (`fmt_tokens(500) == "500"`, no decimal, ends at the
right edge). Those have no decimal to align to, and are vanishingly rare in this
ledger (scope totals are k/M). The fix targets the decimal column (2 from the
field's right edge), which is correct for the k/M values that dominate; a bare
`500` would sit at the right edge, one column right of the aligned dashes — an
orthogonal, non-reported case the phase should note as out of scope rather than
try to special-case.
