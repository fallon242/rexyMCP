# M39 — Executor Cache Accounting

**Goal:** Capture the executor's prefix-cache token counts from the vLLM
`usage.prompt_tokens_details` block — both the cache **read** (already parsed but
never populated until now) and the cache **write** / creation (surfaced by vLLM
but currently dropped) — so the discount ledger prices cached tokens at their
cheaper rate instead of the full input rate.

**Status:** done *(opened 2026-07-24; closed 2026-07-24)*

**Depends on:** M38 (the discount ledger that consumes these fields:
`scope_costs` sums cache tokens, `scope_report` prices them against
`[models] cache_read_per_mtok` / `cache_creation_per_mtok`), M35 (the telemetry
schema version that carries `cache_read_tokens` / `cache_write_tokens` on
`PhaseRun`).

## Why this milestone exists

Logged as a candidate at the M38 close: the executor's `cache_read_tokens` and
`cache_write_tokens` read **zero across all 41 in-schema `PhaseRun` records**.
The whole pricing path was wired and receiving nothing. The M38 note hypothesised
two causes — (a) the backend doesn't surface prefix-cache hits, or (b) a parser
bug — and mandated **investigation before scoping**.

**The investigation is done (2026-07-24, architect probe against the live
`brain:8000` deployment).** Findings:

- **Cause (b) is ruled out.** `parse_openai_usage`
  (`executor/src/ai/backends/openai.rs:11-28`) already reads
  `prompt_tokens_details.cached_tokens` and correctly subtracts it from
  `input_tokens` (line 23) — no double-count. The parser was never the problem.
- **The backend was the problem, and it is now fixed by an ops flag.** The
  original vLLM process returned `"prompt_tokens_details": null` even on a
  confirmed cache hit (the `/metrics` counter `vllm:prompt_tokens_cached_total`
  incremented 1728 for a request whose `usage` still showed `null`). Prefix
  caching was active the whole time (~93% hit rate); the OpenAI-compatible
  `usage` object simply did not expose it. **The human restarted vLLM with
  `--enable-prompt-tokens-details` (alongside the already-present
  `--enable-prefix-caching`)**, and the field now populates.
- **vLLM surfaces BOTH halves of the cache**, one via a non-standard field:

  ```
  cold call:  "prompt_tokens_details": {"cached_tokens": 0,    "created_cache_tokens": 1728, "multimodal_tokens": null}
  warm call:  "prompt_tokens_details": {"cached_tokens": 1728, "created_cache_tokens": 0,    "multimodal_tokens": null}
  ```

  `cached_tokens` is the cache **read** (OpenAI-standard; already parsed).
  `created_cache_tokens` is the cache **write** / creation — a **vLLM extension**,
  not in the OpenAI spec — and the parser hardcodes `cache_write_tokens: 0`
  (`openai.rs:26`), so it is dropped. `multimodal_tokens` is irrelevant here.

So the milestone is a small, well-bounded **capture** change at a single choke
point, plus the disjointness correction the second field forces — not the
metrics-endpoint integration or the "stop pricing the unmeasurable" cleanup the
M38 note also floated (both now moot; the per-request contract delivers the data).

### The disjointness the second field forces

`prompt_tokens` is the whole prompt (3017 in the probe). The three token classes
must stay **disjoint and sum to `prompt_tokens`**, matching the Anthropic billing
model the rates assume:

```
prompt_tokens = input_tokens + cache_read_tokens + cache_write_tokens
```

- Warm: `3017 = 1289 + 1728 + 0`
- Cold: `3017 = 1289 +    0 + 1728`

The parser today computes `input_tokens = prompt_tokens - cache_read` and never
subtracts cache-write (it was always 0). Once cache-write is captured, it must
become `input_tokens = prompt_tokens - cache_read - cache_write`, or the cold-call
input is overcounted by the creation tokens and the discount is wrong.

### Modeling caveat (recorded, not blocking)

The discount prices executor tokens at the **architect (Opus) rate** to estimate
what Claude would have charged. Whether *Claude* would have served a token from
*its* prompt cache depends on Claude's cache state, not on whether the local vLLM
prefix-cached it — the two caches are unrelated. So applying Claude's cache-read
rate to vLLM's cache hits is an approximation. The human elected to capture the
measurement regardless (enabling the vLLM flag); this milestone honours that
decision. The caveat is logged in `docs/architecture.md` §39 for the record and
is **not** a blocker — the alternative (pricing every cached token at the full
input rate) is strictly less accurate, and the direction of the error today is a
systematic *understatement* of savings.

## Exit criteria

- `parse_openai_usage` populates `cache_write_tokens` from
  `prompt_tokens_details.created_cache_tokens` (the vLLM extension field), and
  leaves it `0` when the field is absent (LM Studio / Ollama / older vLLM) — pinned
  by a negative test with a details block that omits it.
- `input_tokens = prompt_tokens - cache_read - cache_write`, with the three
  classes disjoint and summing to `prompt_tokens` — pinned by a **cold-call**
  fixture (`created_cache_tokens` set, `cached_tokens` 0) and a **warm-call**
  fixture (the reverse), asserting the sum identity in both.
- The `saturating_sub` chain never underflows when a malformed backend reports
  `cached_tokens + created_cache_tokens > prompt_tokens` (pinned negative test;
  it must clamp, not panic).
- Both the streaming (`openai.rs:314`) and non-streaming call sites benefit,
  because both route through the one `parse_openai_usage` — no second fix site.
- End-to-end: a real dispatched phase run against `brain:8000` records **non-zero**
  `cache_read_tokens` (and `cache_write_tokens` on its first turn) in its
  `PhaseRun`, and `rexymcp costs` / the dashboard Budget panel price the cache
  tokens through the existing M38 ledger — quoted in the phase Update Log. This is
  the criterion the whole milestone exists to satisfy; it must be shown against the
  live binary, not a unit fixture.
- All four gates green.

## Architecture references

- `docs/architecture.md` § Status #39 — this milestone's design summary + the
  modeling caveat.
- `docs/architecture.md` § Status #38 — the discount ledger that consumes these
  fields.
- `executor/src/ai/backends/openai.rs:11-28` — `parse_openai_usage`, the single
  choke point (called from the streaming aggregator at `:314`).
- `executor/src/ai/types.rs:44-61` — `TokenBreakdown`, which already carries
  `cache_read_tokens` / `cache_write_tokens`.
- `executor/src/store/metrics.rs:40-46` and `executor/src/store/telemetry.rs`
  — the pricing that already consumes the fields.

## Phases

| #  | Phase | Status |
|----|-------|--------|
| 01 | Capture `created_cache_tokens` as `cache_write_tokens` + disjoint `input_tokens`, with cold/warm/absent/underflow fixtures ([phase-01-capture-cache-write.md](phase-01-capture-cache-write.md)) — approved_first_try; reviewer mutation-check bites 3 tests, live E2E shows `cache_read_tokens=643680` on this very run | done |

**Single-phase milestone.** Phase 01 is the whole code change (`parse_openai_usage`
plus its tests). The originally-mooted phase-02 (live E2E) is **folded into phase-01
as a reviewer-run exit criterion**, not a separate executor phase: the live-network
confirmation cannot be done hermetically by the executor, so the architect runs it
at approval (dispatch a real phase, confirm non-zero `cache_read_tokens` in the
`PhaseRun` and that `rexymcp costs` prices them). Decided at draft time
(2026-07-24).

## Notes

**The probe methodology, for reproducibility.** The finding hinged on separating
"is caching happening?" from "is it reported per-request?". `/metrics`
(`vllm:prompt_tokens_cached_total`) answered the first (yes, always was);
capturing a single warm request's `usage` while measuring that counter's delta
answered the second (the delta moved 1728 while `usage.prompt_tokens_details` was
`null` — proving report-gap, not cache-miss). After `--enable-prompt-tokens-details`,
the per-request field populates. Re-run this probe if a backend swap or vLLM
upgrade makes the fields read zero again before assuming a parser regression.

**Backend portability.** `created_cache_tokens` is a vLLM extension. The parser
must treat it as optional and default to `0` so LM Studio / Ollama / older vLLM
(which omit the whole details block or the extension field) keep working with
cache-write correctly zero. The negative test pins this.

## M39 retrospective (2026-07-24)

**One phase, `approved_first_try`, opened and closed the same day.** The whole
milestone was a ~10-line change to `parse_openai_usage` plus five tests — the
smallest milestone since the early scorecard work — because the hard part
(diagnosis) was front-loaded into the *open* step, not spent on executor bounces.

**Investigation-first paid for itself.** The M38-close note said "investigate
before scoping," and doing the live probe *as the architect during milestone
open* — not as a phase-01 spike — is what made phase-01 a clean single-shot. Two
hypotheses (parser bug vs backend gap) collapsed to one empirical fact (vLLM
returns `prompt_tokens_details: null` despite a `/metrics`-confirmed cache hit),
which turned a fuzzy "cache accounting" milestone into a precise capture change.
The probe also surfaced the bonus the note never anticipated — vLLM's non-standard
`created_cache_tokens` (cache-write) — which became the actual code. **Lesson
reinforced: when a candidate milestone hinges on an unknown about a live system,
resolve it with a probe before writing the README, not after dispatching.**

**The ops dependency was the real gate, and it was the human's.** The field only
appears with vLLM's `--enable-prompt-tokens-details`; the architect cannot restart
the user's inference server. The milestone correctly routed that decision to the
human (who enabled it mid-open), rather than the architect assuming or a phase
trying to configure infra it can't reach. This is the healthy shape for any
milestone whose correctness depends on backend configuration.

**Two-stage go-live, recorded so telemetry readers aren't confused.** The
`--enable-prompt-tokens-details` restart made **cache-read** flow immediately —
even the pre-fix `serve` binary already parsed `cached_tokens` — so the phase-01
run's own `PhaseRun` recorded `cache_read_tokens = 643680` (the first non-zero in
project history), and the M38 ledger priced it (M39 milestone Executor total
733.9k tokens). **Cache-write** required the code fix *and* a `serve` rebuild:
approved with `cache_write_tokens = 0` in live telemetry (running binary was still
pre-fix, unit-proven by the cold fixture), then the human **rebuilt and restarted
`serve` post-approval**, activating cache-write capture for subsequent runs. So a
reader seeing the first non-zero `cache_write_tokens` should date the go-live to
the post-approval serve restart, not to phase-01 approval.

**Calibration (1 occurrence, no fold).** The phase-doc Test plan told the cold
test to assert `total() == 3017`; the correct value is `3019` (`total()` includes
`output_tokens`). Architect arithmetic slip — I hand-wrote a total instead of
summing the four token classes. The executor asserted the right number (`3019`)
and ignored the spec's wrong one. It's the "derive every spec fact from its
source" pattern again (a pre-injected *number* is a spec fact like any other), but
at one occurrence for this specific sub-form (a computed assertion value) it stays
recorded, not folded. Held for recurrence.

**Deferred / follow-ups leaving M39:**

- **The modeling caveat stands, unactioned by choice.** Pricing vLLM cache-hits at
  Claude's cache-read rate conflates two unrelated caches (architecture.md §39).
  The human elected to capture the measurement anyway; revisiting whether the
  discount should apply a cache rate at all is a future pricing-model question, not
  a bug. No milestone opened.
- **Carried past M37, still open, none blocking:** the phase-01 `NoProgressStall`
  backstop calibration on the post-exemption corpus (architecture.md §37); the
  `missing_spec_test`/broken-fixture failure shape; the `$`-less `executor_val`
  debit nit (M38).

**No WORKFLOW.md/STANDARDS.md folds landed at this close.**