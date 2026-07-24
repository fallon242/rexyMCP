# Phase 06: Budget panel — align debit decimals + `Tokens` header

**Milestone:** M37 — Governor Read-Only Calibration
**Status:** done
**Depends on:** none (M38's `ledger_lines`; independent of 01–05)
**Estimated diff:** ~70 lines
**Tags:** language=rust, kind=bugfix, size=s

## Goal

Two small budget-panel (`ledger_lines`) fixes found using the dashboard, plus
the repair of a fake test that let the first one regress:

1. **Debit decimals sit one column left of credit decimals.** A debit renders
   `($1940.72)` and a credit `$1414.70`; right-aligned in the same column, the
   debit's trailing `)` pushes its `.` one column left of the credit's `.`. This
   is the M35 07d–07h alignment problem, reintroduced when M38 phase-02 rewrote
   this renderer. Align them via a trailing **sign-gutter** on non-debit values.

2. **Tokens-mode header reads `Spend (tok)`; make it `Tokens`.** When the `b`
   toggle is on tokens, the header should say `Tokens`, not `Spend (tok)`.

3. **The guard test is fake and must be fixed.**
   `savings_lines_debit_digits_align_with_non_debit`
   (`mcp/src/dashboard/panels.rs:1920`) has a docstring promising "the decimal
   point of a debit row and a non-debit row must be at the same column index"
   but a body that only asserts **all rows are equal width**. Equal width ≠
   aligned decimals — which is exactly why defect 1 shipped. The test must assert
   the decimal-column equality it claims.

## Architecture references

Read before starting:

- `docs/dev/milestones/M35-metrics-cost-accounting/phase-07g-dash-decimal-align.md`
  and `phase-07h-dash-tight-parens.md` — the sign-gutter / decimal-column
  convention this restores (`—` and `.` land 3 chars from a cell's right edge).
- `docs/dev/milestones/M38-discount-accounting/phase-02-ledger-layout-shared-renderer.md`
  — where `ledger_lines` was written and where the alignment regressed.

## Pre-flight

1. Read `docs/dev/STANDARDS.md` top to bottom.
2. Read the architecture references above.
3. Read this entire phase doc before touching any code.
4. Confirm the repo is on a clean branch with no uncommitted changes.

## Current state

**`mcp/src/costs.rs` `ledger_lines` / `paren` / `make_row`** own the rendering.
The dashboard's `savings_lines` delegates to `ledger_lines` entirely
(`panels.rs:519`), so this fixes both the CLI `rexymcp costs` table and the
dashboard Budget panel at once.

**The exact misalignment** (reproduced; `make_row` uses `{:>10}` per value cell
with a milestone, `{:>9}` without):

```
  Architect:       (—)        (—)  ($1940.72)     <- '.' at column 38
  Executor:     $13.76     $27.97    $1414.70     <- '.' at column 39
  Net:               —          —   ($526.03)     <- '.' at column 38
```

Debit forms (`($1940.72)`, `(—)  `) place their marker 3 chars from the cell's
right edge; credit forms (`$1414.70`) place theirs 2 chars in, and the
non-debit `DASH = "—  "` also 2 in — one column right of the debits.

**The value producers in `ledger_lines`:**
- `paren(v)` — debit wrapper: `($X.XX)` for a value, `"(—)  "` for no-value.
  **Debit forms are already correct; do not touch `paren`.**
- `executor_val` / `net_val` — return a **credit** `$X.XX` (via `fmt_dollars`),
  a **debit** `(X.XX)` / `(${...})` when negative, or the non-debit
  `DASH = "—  "` when `None`.

## Spec

### 1. Add a 1-char sign-gutter to non-debit values

The rule: **debit forms (parenthesised, ending in `)`) are unchanged; every
non-debit form gains one trailing space** so its marker lands 3 chars from the
right edge, matching the debits.

Two concrete edits — the worked target (reproduced, exact):

- **Credit dollars** — where `executor_val`/`net_val` return `fmt_dollars(val)`
  (`$X.XX`), append one trailing space: `format!("{} ", fmt_dollars(val))` →
  `"$1414.70 "`. Do **not** add the space to the negative/debit branch
  (`(X.XX)` / `(${...})`), which is already a debit form.
- **Non-debit dash** — change `const DASH: &str = "—  ";` to
  `const DASH: &str = "—   ";` (three trailing spaces).

After the fix, all three rows' markers land at the same column:

```
  Architect:       (—)        (—)  ($1940.72)     <- '.' col 38
  Executor:     $13.76     $27.97    $1414.70     <- '.' col 38  (number shifted 1 left)
  Net:               —          —   ($526.03)     <- '.' col 38
```

The debit rows (Architect, and Net/Executor when negative) are visually
unchanged; the credit numbers and dashes shift one column left into alignment.

**Tokens mode is unaffected** — token cells (`fmt_tokens`) are never
parenthesised, so they are already internally consistent. Do not add a gutter in
tokens mode.

### 2. Tokens-mode header `Spend (tok)` → `Tokens`

In `ledger_lines`, the `LedgerUnits::Tokens` header branches (both the
`has_milestone` and no-milestone arms) render `"Spend (tok)"`. Change the label
to `"Tokens"`. The `LedgerUnits::Dollars` header stays `"Spend"`.

### 3. Fix the fake alignment test

In `mcp/src/dashboard/panels.rs`, `savings_lines_debit_digits_align_with_non_debit`
(`:1920`) currently asserts only equal row width. Replace the body's assertion
with the decimal-column-equality check its docstring already promises: find the
`Architect:` and `Executor:` rendered lines and assert
`architect.find('.') == executor.find('.')` (for a fixture where both rows carry
a dollar value in the same scope). Keep the equal-width check too if convenient —
but the decimal-equality assertion is the one that must be present. This test
**must fail** against the pre-fix code.

## Acceptance criteria

- [x] `cargo build` is green.
- [x] `cargo clippy --all-targets --all-features -- -D warnings` is clean.
- [x] `cargo fmt --all --check` reports no diff in the files this phase touched.
- [x] `cargo test` passes.
- [x] In dollars mode, the `.` (or `—`) of the `Architect:`, `Executor:`, and
      `Net:` rows are at the **same column index** in each scope — pinned by a
      real decimal-equality test, not a width check.
- [x] Tokens mode renders the header `Tokens` (not `Spend (tok)`); dollars mode
      still renders `Spend`.
- [x] `savings_lines_debit_digits_align_with_non_debit` asserts decimal-column
      equality and would fail against the old rendering.

## Test plan

`ledger_lines` is pure; `savings_lines` is a thin adapter over it. Test through
whichever the existing tests use — the module already has `savings_lines`-based
render tests (`panels.rs`) and `ledger_lines` tests (`costs.rs`).

- **Strengthen** `savings_lines_debit_digits_align_with_non_debit` per § Spec 3 —
  decimal-column equality between Architect and Executor. This is the load-bearing
  test; it must fail on the pre-fix code (verify by reverting the gutter locally).
- `ledger_dash_and_decimal_share_column` — a fixture where one scope's Architect
  is `(—)` (debit dash), Executor is a `$X.XX` credit, and Net is `—` (non-debit
  dash); assert the `—`/`.`/`—` markers are at the same column index across the
  three rows.
- `ledger_tokens_header_is_tokens` — tokens mode header equals `Tokens` (exact),
  and does **not** contain `Spend`. (Negative case: guards against renaming to
  something that still says Spend.)
- `ledger_dollars_header_still_spend` — dollars mode header still begins `Spend`.
  Pins that fix 2 is scoped to tokens mode only.

Pin **marker column equality** and **exact header text**, not full-line byte
layout, per WORKFLOW § "Specs pin behavior, not rendering".

## End-to-end verification

Against the real binary — the CLI `costs` table shares the renderer, so it shows
the alignment directly:

```bash
cargo run -p rexymcp -- costs --config rexymcp.toml --repo .
cargo run -p rexymcp -- costs --config rexymcp.toml --repo . --tokens | head -3
```

Paste both. Expected: in the dollars output, the `.`/`—` of the Architect,
Executor, and Net rows line up vertically in each column; in the `--tokens`
output, the header reads `Tokens`. (The dashboard Budget panel renders the same
`ledger_lines`, so it is fixed in lock-step — note that rather than claiming a
separate TUI screenshot.)

## Authorizations

None. No new dependencies. No edits to `docs/architecture.md`.

## Out of scope

- Widening the value columns to align at the *credit* column instead (the
  "keep Executor put, move Architect right" alternative). The chosen fix aligns
  at the debit column via the sign-gutter — the minimal, standard-accounting
  change (user decision).
- Any tokens-mode alignment change — token cells are never parenthesised.
- The `b`-key handler, panel borders, `budget_lines`, or any non-ledger panel.
- Governor / calibrate-governor / completion-entry code (the rest of M37).

## Update Log

(Filled in by the executor. See WORKFLOW.md § "Update Log entries".)

<!-- entries appended below this line -->

### Completion — 2026-07-24 (architect takeover)

**Executor:** Claude (architect, takeover). The dispatched executor run
(`Qwen/Qwen3.6-27B-FP8`, run `95b407a2`) was stopped by the human at turn 126,
looping a read-only search. It had landed the two mechanical fixes (tokens header
rename; the fake test's docstring assertion) but **not** the sign-gutter
production fix — and both new tests it wrote carried broken fixtures:
`ledger_dash_and_decimal_share_column` set `net: Some(-10.0)` (rendering Net as a
debit, so `net.find('—').expect()` would have panicked), and the panels test's
rates made the Executor row a *debit* `(2.00)` rather than a credit (so it
compared two debit rows and could not fail pre-fix). The user authorized a
takeover to complete the phase.

**Summary:** Restored the M35 07d–07h sign-gutter in `ledger_lines`
(`mcp/src/costs.rs`): `DASH` gains a third trailing space, and both positive
credit branches (`executor_val`, `net_val`) append one trailing space, so every
non-debit marker lands 3 chars from the field's right edge — the debit column.
Renamed the tokens-mode header `Spend (tok)` → `Tokens` (dollars stays `Spend`).
Rewrote the two executor tests to genuine same-column marker-equality checks and
updated the pre-existing `ledger_dash_aligns_with_decimal_column` (dash moves
col 18 → 17) and `savings_lines_tokens_mode_shows_token_counts` (header assertion
`"tok"` → `"Tokens"`).

**Gates:** `cargo build` clean; `cargo clippy --all-targets --all-features -D
warnings` clean; `rustfmt --check` clean on both touched files; `cargo test` =
657 (bin) + 1045 (lib) passing, 0 failed.

**Mutation check (falsifiability):** reverted the sign-gutter production edits and
confirmed all three alignment tests fail with the exact misalignment
(marker at col 17 vs 18): `savings_lines_debit_digits_align_with_non_debit`,
`ledger_dash_and_decimal_share_column`, and `ledger_dash_aligns_with_decimal_column`.
Restored the fix; all pass.

**End-to-end verification:** `cargo run -p rexymcp -- costs --config rexymcp.toml
--repo .` — parsed marker columns per row:

```
Spend          Session Milestone   Project
  Architect:     (—)       (—)  ($2022.82)     Session —@18  Milestone —@28  Project .@38
  Executor:    $19.02    $67.06  $1453.78       Session .@18  Milestone .@28  Project .@38
  Net:            —         —    ($569.04)      Session —@18  Milestone —@28  Project .@38
```

Every marker aligns per scope (18 / 28 / 38); pre-fix the Executor decimals sat
one column right. `--tokens` renders the header `Tokens`. The dashboard Budget
panel renders the same `ledger_lines`, so it is fixed in lock-step.

**Code:** commit `00283cb`.

### Review verdict — 2026-07-24

- **Verdict:** escalated (architect takeover; implementation and review were the
  same agent — the mutation-check + independent-gate + parsed-E2E discipline
  substitutes for reviewer independence)
- **Bounces:** none (the dispatched run was human-stopped mid-loop, not bounced)
- **Executor:** Claude (architect, takeover) — dispatched model was
  `Qwen/Qwen3.6-27B-FP8`
- **Scope deviations:** none. Touched only `ledger_lines` and its tests, plus two
  pre-existing tests that pinned the old rendering.
- **Notes for review:** while working `executor_val`'s negative branch, noted it
  emits `(X.XX)` **without** a `$` where `net_val` emits `($X.XX)` — a pre-existing
  M38 inconsistency, out of scope here (debit forms were explicitly not to be
  touched), not fixed. Worth a follow-up nit.
- **Calibration:** two folds candidate — (1) *a test whose name/docstring promises
  more than its body checks is worse than none*; the equal-width guard's docstring
  claimed decimal-column equality but asserted width, and it shipped the very
  regression it named — this slipped my own M38 review. (2) The executor's failure
  shape here (correct mechanical edits, broken reasoning on the geometric core,
  plus **broken test fixtures that would panic/no-op**) is a variant of
  `missing_spec_test` — the tests existed but were non-falsifiable. Recorded for
  the M37 close retrospective.
