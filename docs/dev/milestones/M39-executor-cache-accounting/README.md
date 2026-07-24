# M39 — Executor Cache Accounting

**Goal:** Capture the executor's prefix-cache token counts from the vLLM
`usage.prompt_tokens_details` block — both the cache **read** (already parsed but
never populated until now) and the cache **write** / creation (surfaced by vLLM
but currently dropped) — so the discount ledger prices cached tokens at their
cheaper rate instead of the full input rate.

**Status:** in-progress *(opened 2026-07-24)*

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
| 01 | Capture `created_cache_tokens` as `cache_write_tokens` + disjoint `input_tokens`, with cold/warm/absent/underflow fixtures ([phase-01-capture-cache-write.md](phase-01-capture-cache-write.md)) | review |

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