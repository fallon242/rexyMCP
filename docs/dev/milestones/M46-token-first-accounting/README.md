# M46 — Token-First Accounting

**Goal:** Retire dollar-denominated cost accounting and reporting entirely;
token counts become the sole accounting currency, and token reporting is
enhanced so nothing observable is lost in the trade.

**Status:** done *(opened 2026-08-16; closed 2026-08-16)*

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
| 02 | Dashboard token views ([phase-02-dashboard-token-views.md](phase-02-dashboard-token-views.md)): reintroduce `b` as a token-view cycler (totals ⇄ cache split — first surface for the ledger's 5m/1h cache-creation split), wire the session-scope cache classes via a backward-compatible `SessionEvent::Metrics` extension | done        |
| 03 | Pricing plumbing removal ([phase-03-pricing-plumbing-removal.md](phase-03-pricing-plumbing-removal.md)): config rate fields + `known_model_rates` + `token_cost` + `ArchitectLedger::cost` + multipliers; `runs`/`profile` `COST` columns → token/cache-hit columns; `init`/`calibrate` scaffolds; stale `[dashboard]` block in `rexymcp.toml`; README config blocks (guard test) | done        |
| 04 | Docs sweep ([phase-04-docs-sweep.md](phase-04-docs-sweep.md)): README release-notes/dashboard/CLI-reference prose, `plugin/skills/auto/SKILL.md` + `escalate/SKILL.md` wording, ASCII mock; screenshot regeneration stays a human action | done        |

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

## M46 retrospective — closed 2026-08-16

**Four phases, four `approved_first_try`, zero bugs, zero bounces, zero
assists.** Phase-01 ran interactively; phases 02–04 ran inside a single
`/rexymcp:auto` loop (dispatch/review delegated to `claude-sonnet-5`
subagents, drafting and loop control on the session model). Every review was
an independent re-run — gates, acceptance greps, and E2E against the real
binary — not a transcript read.

**Executor: `deepseek-v4-flash-0731`, first milestone on this model.**
Scorecard after M46: N=4, gates 1.00, AFT rate 1.00, parse-fail 0.08,
turns-mean 267.5 (high — 237/336/370/127 per phase; the model grinds in
small steps but lands clean), verifier-retries mean 16.75 (also high, same
grinding style), peak context 18% of a 1024k window, cache-hit 96.7–98.1%
per run. Tokens per phase: 26.9M / 28.1M / 39.2M / 4.2M.

**What the milestone shipped, verified end to end:** no dollar value on any
surface; the `Architect / Executor / Cache` token ledger on `costs` + the
dashboard; the `b` token-view cycler (Totals ⇄ Cache split) giving the
ledger's 5m/1h cache-creation split its first reader; session-scope cache
classes via a backward-compatible `SessionEvent::Metrics` extension;
`CACHE%` columns on `runs`/`profile`; the whole `$/Mtok` config/rate layer
deleted (leftover keys silently ignored, live `rexymcp.toml` cleaned); docs
token-first. Every M46 exit criterion checked out at phase reviews.

**Findings:**

- **`rexymcp.toml` is gitignored**, so phase-03's live-config cleanup exists
  on disk only — a phase doc that treats the live config as a committed
  artifact is subtly wrong about what review can see in the diff. Reviewers
  verified the file directly; worth remembering when a future phase touches
  it.
- **Undisclosed executor scope deviation, 1× (data, no fold):** phase-03
  also removed README rate-table prose that was named phase-04 territory —
  let stand because phase-03's own deletions had made that prose false, but
  the executor did not list it in Notes for review.
- **Architect E2E-block syntax errors, 2× (trend, watch):** phase-01's block
  had a wrong cargo package name; phase-03's had a wrong `runs show` arg
  order and an `init` flag mismatch. Both times the executor adapted and the
  pinned behavior was still exercised, but the pattern is the architect
  pinning CLI invocations it never dry-ran. Third occurrence folds a
  "dry-run the E2E block before pinning it" rule.
- **Server-authored completion entries still head themselves
  `ts=<epoch-ms>`, 3rd occurrence (threshold reached).** The fix is server
  runtime code (the entry writer), not a doc fold — recorded as a candidate
  work item for a future milestone, needs human go-ahead.
- **Pre-injecting exact replacement prose works for docs phases:** phase-04
  (a pure-markdown rewrite by a local code model) landed in 127 turns with
  zero bounces because the release-notes bullet, mock, and report-line text
  were pasted into the spec verbatim rather than described.

**Human follow-ups (outstanding):**

1. **Reinstall the CLI binary** — `~/.cargo/bin/rexymcp` is still the
   pre-M46 0.9.1 build; day-to-day `rexymcp costs`/`runs` show dollars until
   `cargo install --path mcp` (and restart any live `rexymcp serve`, which
   never hot-swaps a rebuilt binary).
2. **Regenerate `docs/rexymcp_dashboard.png`** — live capture of the new
   Budget ledger + cache-split view.
3. Optional editorial: the README ASCII mock's pre-existing one-column
   border misalignment on the `Architect:` row (predates M46).

**Future milestone candidates recorded, not started:** server-authored
entry-header date format fix (the 3× item above); architect
tokens-by-milestone attribution (needs a ledger milestone dimension —
deliberately out of M46 scope).
