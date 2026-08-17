# M46 — Token-First Accounting

**Goal:** Retire dollar-denominated cost accounting and reporting entirely;
token counts become the sole accounting currency, and token reporting is
enhanced so nothing observable is lost in the trade.

**Status:** planning *(opened 2026-08-16)*

**Depends on:** M35 (the metrics/cost overhaul being partially reversed),
M38 (the discount-ledger renderer this strips down), M39 (the cache-token
capture this finally makes visible), M40 (the tokens-mode alignment invariant
that must survive).

## Why this milestone exists

Dollar figures in rexyMCP were always *derived*, never measured: every `$`
in the system is computed at read time as token counts × config rates
(`known_model_rates` / `[architect]` / `[models."<id>"]`). The rates drift
with vendor pricing, local models are priced at $0 by fiat, and the numbers
the user actually acts on — budget pressure, cache efficiency, model
comparison — are all token-native underneath. The dollar layer is estimate
theater on top of exact data.

**The de-risking finding (verified 2026-08-16): no dollar value is ever
persisted.** `PhaseRun`, `PhaseReview`, `ArchitectActivity`, and
`ArchitectLedger` are all token/count-denominated; `TELEMETRY_SCHEMA_VERSION`
stays at 1, no JSONL row goes dark, no reader changes behavior. Deprecating
dollars is a presentation + config change, not a storage migration.

Meanwhile token reporting has real gaps that only existed because pricing was
the sole consumer of the token split:

- **Cache-hit ratio is computed nowhere.** M39 made the three input classes
  disjoint (`input` / `cache_read` / `cache_write`) but the only reader of
  the split is the pricing math. Remove dollars without adding a hit-rate
  view and the M39 capture work becomes unobservable.
- **Tokens mode has a dead `Net:` row** (`costs.rs:466-471` renders `—` for
  all three scopes) — a free slot for real content.
- **The by-skill table is dollars-only** (`costs.rs:353-368` prints a `COST`
  column and dollar-share percentages even under `--tokens`).
- **The architect ledger's 5m/1h cache-creation split**
  (`telemetry.rs:621,625`) would have zero readers once
  `ArchitectLedger::cost` goes — it needs a token-native display or it is
  dead weight the compiler won't flag.

## Exit criteria

- **No dollar anywhere.** No `$`-denominated value in any output surface —
  `rexymcp costs` (table and `--json`), dashboard, `runs`, `profile`,
  `scorecard` — and no `*_per_mtok` field parsed from config.
  `known_model_rates`, `ModelOverride`'s four rate fields, `ArchitectConfig`'s
  five rate fields + `rates` map (and `effective_rates` /
  `effective_architect_rates` / `rates_for`), `metrics::token_cost`,
  `ArchitectLedger::cost`, and the three `CACHE_*_RATE_MULTIPLIER` constants
  are deleted. `ArchitectConfig` itself survives — `dispatch_model` /
  `review_model` are non-cost role delegation.
- **Token capture is untouched.** The disjoint three-class input invariant
  (`openai.rs` `parse_openai_usage`), per-turn accumulation, `PhaseRun.tokens`,
  and the harvest → `ArchitectLedger` path are byte-identical. The 5m/1h
  cache-creation fields survive **with at least one real reader** (the
  cache-split view below).
- **Cache-hit ratio is derived and displayed** on at least `rexymcp costs`
  and the dashboard Budget panel, from the existing disjoint classes.
- **By-skill reporting is token-native:** token-denominated percentages,
  ordering keyed on tokens (note: the dashboard "Top skill" identity can
  legitimately change — top-by-dollars ≠ top-by-tokens when models' rates
  differed — and the `render.rs:263` `cost > 0.0` gate becomes `tokens > 0`,
  which *un-hides* skills run on unpriced models).
- **The M40 alignment invariant holds:** the tokens-mode marker-column-
  equality test passes unchanged; the dollars-mode `DASH`/`paren` constants
  and their tests are deleted with the dollars branch, not rewritten.
- **`mcp/tests/readme_config_reference.rs` stays green in the same commit**
  as each config-field removal (README sample-TOML edits land together with
  the code).
- All four gates green.

## Architecture references

- `docs/architecture.md` § Status #46 — this milestone's summary; #35, #38,
  #39, #40 — the accounting arc being reshaped.
- `mcp/src/costs.rs` — `ScopeReport` / `scope_report` / `skill_costs` /
  `ledger_lines` (`LedgerUnits`, `paren`, `TOK_DASH`).
- `executor/src/config.rs` — `known_model_rates`, `ArchitectConfig`,
  `ModelOverride`, `Config::model_rates`.
- `executor/src/store/telemetry.rs` — `ArchitectLedger::cost` + the three
  rate multipliers; the 5m/1h split fields that must outlive them.
- `executor/src/store/metrics.rs` — `token_cost` (goes); `fmt_tokens`,
  `tokens_per_sec` (stay).
- `mcp/src/dashboard/{event_loop,panels,render}.rs` — `BudgetDisplay`, the
  `b` keypress, Top skill.

## Phases

Planned decomposition — expanded into phase docs on demand; the split may
reflow as earlier phases reveal information.

| #  | Phase | Status |
|----|-------|--------|
| 01 | Token-native `costs` core ([phase-01-token-native-costs-core.md](phase-01-token-native-costs-core.md)): delete the dollars branch of `ledger_lines` + `LedgerUnits` + `paren` + `BudgetDisplay`/`BudgetRates` (the dashboard shares the renderer, so its dollars mode and the `b` toggle come out here as a compile-forced cascade), tokenize the by-skill table + Top skill, put a `Cache:` hit-ratio row in the dead `Net:` slot, strip `ScopeReport` to tokens, drop `--tokens` | done |
| 02 | Dashboard token views ([phase-02-dashboard-token-views.md](phase-02-dashboard-token-views.md)): reintroduce `b` as a token-view cycler (totals ⇄ cache split — first surface for the ledger's 5m/1h cache-creation split), wire the session-scope cache classes via a backward-compatible `SessionEvent::Metrics` extension | review      |
| 03 | Pricing plumbing removal: config rate fields + `known_model_rates` + `token_cost` + `ArchitectLedger::cost` + multipliers; `runs`/`profile` `COST` columns → token/cache-hit columns; `init`/`calibrate` scaffolds; stale `[dashboard]` block in `rexymcp.toml`; README config blocks (guard test) | todo |
| 04 | Docs sweep: README release-notes/dashboard/CLI-reference/rate-table prose, `plugin/skills/auto/SKILL.md` closing-report line, dashboard screenshot regeneration (human action) | todo |

Ordering is presentation-first (01–02) then plumbing (03) so the tree builds
green at every phase boundary: once no surface *renders* dollars, deleting
the computation and config behind them is a pure dead-code sweep.

## Notes

Design decisions made at milestone open (architect-recommended; overridable
before phase-01 dispatch):

- **Hard removal, not soft deprecation** — the M38 precedent
  (`architecture.md` § Status #38: `DashboardConfig` was deleted outright
  because "a documented-but-ignored knob actively misleads"). No
  accept-and-ignore shims: `--tokens` is removed (tokens are now the only
  mode), `profile --cost` loses its `COST` column, and unknown
  `*_per_mtok` config keys fail or warn per the existing unknown-field
  policy rather than being silently honored.
- **`costs --json` is a breaking change, shipped loudly.** `ScopeReport`
  loses `saved`/`executor`/`architect`/`net` (the `f64` dollar fields);
  consumers break on a missing key, not silently. Release-notes entry
  required in phase 04.
- **The `b` toggle is repurposed, not deleted:** it cycles token views
  (totals ⇄ cache split) instead of dollars ⇄ tokens, keeping the keybinding
  and giving the M39 capture and the ledger's 5m/1h split their first
  visible surface.
- **M38's discount concept ("saved"/"Net") retires with dollars.** Executor-
  tokens-valued-at-architect-rates only means anything in dollars; the
  two-line Architect/Executor ledger survives in token form, the Net row's
  slot is reassigned to cache reporting.
- **The M39 settled decision is mooted, not re-litigated.** The
  keep-as-is call on "Claude's cache state is unrelated to vLLM's"
  (`architecture.md` § Status #39) was about *pricing* executor cache
  tokens at Claude multipliers; with pricing gone the caveat has no
  referent. This is the "genuinely new reason" clause firing, and the
  outcome is deletion of the caveat's subject, not reversal of the call.
- **Architect tokens-by-milestone is out of scope.** The ledger is keyed
  `(project, session, model, skill)` with no milestone dimension
  (`costs.rs:190` returns empty architect data for milestone scope), and
  Claude Code transcripts prune at ~30 days, so retroactive attribution
  is impossible. Recorded as a future milestone candidate, not smuggled
  in here.
- **`u32` discipline for new aggregates:** any new token aggregate follows
  `scope_costs`'s pattern — widen to `u64` per-run *before* summing
  (`costs.rs:175-186`); never route milestone-scale totals through the
  `u32` `TokenBreakdown`. `PhaseCost.tokens` is a pre-existing exposure,
  noted, not fixed here.
- Housekeeping riding along in phase 03: the stale (already-ignored)
  `[dashboard]` saved-rates block in this repo's `rexymcp.toml:74-79`, and
  the `tier_telemetry` doc-comment that says "cost instrumentation" but
  holds no cost (`telemetry.rs:104`).
- False positives, do not touch: the three Levenshtein `cost` variables in
  `executor/src/parser/{repair/name.rs,score.rs,feedback.rs}` are edit
  distance, not money.
