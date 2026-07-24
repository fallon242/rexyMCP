# Phase 01: Capture `created_cache_tokens` + disjoint `input_tokens`

**Milestone:** M39 — Executor Cache Accounting
**Status:** todo
**Depends on:** none (first phase of M39)
**Estimated diff:** ~40 lines (a ~10-line parser change + tests)
**Tags:** language=rust, kind=bugfix, size=s

## Goal

The vLLM backend now surfaces prefix-cache tokens in every chat response's
`usage.prompt_tokens_details` (after the human enabled
`--enable-prompt-tokens-details`). The cache **read** (`cached_tokens`) is already
parsed; the cache **write** (`created_cache_tokens`, a vLLM extension) is dropped
because the parser hardcodes `cache_write_tokens: 0`. Capture it, and correct the
`input_tokens` arithmetic so the three input classes stay **disjoint** and sum to
`prompt_tokens`.

This is the whole code change for M39. Everything downstream (the `PhaseRun`
telemetry fields, `scope_costs`, `scope_report` pricing, the M38 discount ledger)
already consumes `cache_write_tokens` — it has just been receiving `0`.

## Architecture references

Read before starting:

- `docs/dev/milestones/M39-executor-cache-accounting/README.md` — the milestone,
  including the live-probe findings this phase implements and the modeling caveat.
- `docs/architecture.md` § Status #39 — design summary.

## Pre-flight

1. Read `docs/dev/STANDARDS.md` top to bottom.
2. Read this entire phase doc before touching any code.
3. Confirm the repo is on a clean branch with no uncommitted changes.

## Current state

**The single choke point** is `parse_openai_usage`
(`executor/src/ai/backends/openai.rs:11-28`). Both the non-streaming and the
streaming (`openai.rs:314`) paths route their `usage` object through it, so this
one function is the only site to change. Here it is verbatim:

```rust
pub(crate) fn parse_openai_usage(u: &serde_json::Map<String, Value>) -> TokenBreakdown {
    let total_prompt = u.get("prompt_tokens").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
    let output_tokens = u
        .get("completion_tokens")
        .and_then(|v| v.as_u64())
        .unwrap_or(0) as u32;
    let cache_read = u
        .get("prompt_tokens_details")
        .and_then(|d| d.get("cached_tokens"))
        .and_then(|v| v.as_u64())
        .unwrap_or(0) as u32;
    TokenBreakdown {
        input_tokens: total_prompt.saturating_sub(cache_read),
        output_tokens,
        cache_read_tokens: cache_read,
        cache_write_tokens: 0,
    }
}
```

**`TokenBreakdown`** (`executor/src/ai/types.rs:44-53`) already carries the field:

```rust
pub struct TokenBreakdown {
    #[serde(default)] pub input_tokens: u32,
    #[serde(default)] pub output_tokens: u32,
    #[serde(default)] pub cache_read_tokens: u32,
    #[serde(default)] pub cache_write_tokens: u32,
}
```

**What the live vLLM actually returns** (captured 2026-07-24 against `brain:8000`,
Qwen3.6-27B-FP8, with `--enable-prefix-caching --enable-prompt-tokens-details`).
A cold call (prompt newly cached) and a warm call (same prefix, cache hit):

```json
// cold — 1728 of the 3017 prompt tokens were written to cache
"usage": {"prompt_tokens": 3017, "completion_tokens": 2,
          "prompt_tokens_details": {"cached_tokens": 0, "created_cache_tokens": 1728, "multimodal_tokens": null}}

// warm — 1728 of the 3017 prompt tokens were read from cache
"usage": {"prompt_tokens": 3017, "completion_tokens": 2,
          "prompt_tokens_details": {"cached_tokens": 1728, "created_cache_tokens": 0, "multimodal_tokens": null}}
```

Note: `cached_tokens` (read) and `created_cache_tokens` (write) were **mutually
exclusive** in every observed call — a token is either freshly created in the
cache or read from it, not both in one request. `multimodal_tokens` is irrelevant
and must be ignored.

## Spec

### 1. Parse `created_cache_tokens` into `cache_write_tokens`

In `parse_openai_usage`, read `prompt_tokens_details.created_cache_tokens` with
the **same optional-chaining shape** the existing `cached_tokens` read uses
(`.get(...).and_then(...).and_then(|v| v.as_u64()).unwrap_or(0) as u32`), and put
it in `cache_write_tokens`. When the field is absent (LM Studio, Ollama, older
vLLM, or a details block that omits it), it must default to `0` — the `unwrap_or(0)`
gives that for free. **Do not** rename or restructure the existing `cached_tokens`
read.

### 2. Make the three input classes disjoint

`prompt_tokens` is the *whole* prompt and already includes both the cached-read
and the newly-created-cache tokens. So `input_tokens` (the fresh, uncached,
non-cache-creating remainder) must subtract **both**:

```
input_tokens = prompt_tokens - cache_read - cache_write
```

Worked against the fixtures above:

- Warm: `3017 - 1728 - 0 = 1289` fresh input; `cache_read = 1728`; `cache_write = 0`.
  Sum `1289 + 1728 + 0 = 3017 = prompt_tokens`. ✓
- Cold: `3017 - 0 - 1728 = 1289` fresh input; `cache_read = 0`; `cache_write = 1728`.
  Sum `1289 + 0 + 1728 = 3017 = prompt_tokens`. ✓

Use a **saturating** subtraction that cannot underflow if a malformed backend
reports `cache_read + cache_write > prompt_tokens`. `u32::saturating_sub` is not
enough on its own for two subtractions — chain them
(`total_prompt.saturating_sub(cache_read).saturating_sub(cache_write)`) so the
result clamps to `0` rather than wrapping. It must **never panic**.

### 3. Nothing else changes

`output_tokens`, `cache_read_tokens`, and the function signature stay as they are.
No downstream file needs editing — `TokenBreakdown` already has the field and the
telemetry/pricing path already reads it. If you find yourself editing
`types.rs`, `metrics.rs`, `telemetry.rs`, `costs.rs`, or config, stop: that is out
of scope and a sign of a wrong turn.

## Acceptance criteria

- [ ] `cargo build` is green.
- [ ] `cargo clippy --all-targets --all-features -- -D warnings` is clean.
- [ ] `cargo fmt --all --check` reports no diff in the files this phase touched.
- [ ] `cargo test` passes (existing parser tests included — they must stay green
      unchanged, since neither carries `created_cache_tokens`).
- [ ] `parse_openai_usage` sets `cache_write_tokens` from
      `prompt_tokens_details.created_cache_tokens`, defaulting to `0` when absent.
- [ ] `input_tokens == prompt_tokens - cache_read - cache_write`, clamped to `0`
      (never panics) when the two exceed `prompt_tokens`.
- [ ] For any parsed usage, `input_tokens + cache_read_tokens + cache_write_tokens
      == prompt_tokens` whenever `cache_read + cache_write <= prompt_tokens`.

## Test plan

Add unit tests in the existing `#[cfg(test)] mod tests` block in
`executor/src/ai/backends/openai.rs`, matching the style of the two present tests
(`openai_parses_cached_tokens_from_details`,
`openai_parses_zero_cache_when_details_absent`) — build the usage object with
`serde_json::json!({...}).as_object().cloned().unwrap()` and call
`parse_openai_usage`. Pin **behavior** (the field values and the sum identity),
not exact wording:

- `openai_parses_created_cache_tokens_as_cache_write` — the **cold** fixture
  (`prompt_tokens: 3017`, `cached_tokens: 0`, `created_cache_tokens: 1728`):
  assert `cache_write_tokens == 1728`, `cache_read_tokens == 0`,
  `input_tokens == 1289`, and `total() == 3017`.
- `openai_warm_call_reads_cache_not_writes` — the **warm** fixture
  (`cached_tokens: 1728`, `created_cache_tokens: 0`): assert `cache_read_tokens
  == 1728`, `cache_write_tokens == 0`, `input_tokens == 1289`.
- `openai_input_plus_cache_classes_equal_prompt_tokens` — for at least the cold
  and warm fixtures, assert `input_tokens + cache_read_tokens + cache_write_tokens
  == prompt_tokens` (the disjointness invariant, stated as one assertion).
- `openai_created_cache_tokens_absent_is_zero` — a details block with
  `cached_tokens` present but **no** `created_cache_tokens` key: assert
  `cache_write_tokens == 0` and the read still works. (Portability: LM Studio /
  Ollama / older vLLM.) This is the **negative** case — it must fail if the new
  read is written to `unwrap`/panic on a missing key instead of defaulting.
- `openai_cache_over_report_clamps_input_to_zero` — a malformed fixture where
  `cached_tokens + created_cache_tokens > prompt_tokens` (e.g. `prompt_tokens:
  100`, `cached_tokens: 80`, `created_cache_tokens: 40`): assert the call does
  **not** panic and `input_tokens == 0`.

**Mutation self-check before you finish:** temporarily change the production
`input_tokens` to subtract only `cache_read` (the old behavior) and confirm the
cold-fixture and disjointness tests **fail**; then restore. A test that passes
against the old arithmetic is not pinning the fix. (Do not commit the mutation.)

## End-to-end verification

**Hermetic boundary — do NOT hit the network.** Tests must not call `brain:8000`
or any endpoint (STANDARDS: no real network). Your end-to-end proof stays inside
the process: the fixtures above are the *exact* JSON vLLM emits, so a unit test
that feeds them through `parse_openai_usage` **is** the end-to-end parse of a real
backend response. If you want to exercise the streaming path too, feed a usage
chunk through the same parser — but a live network call is out of bounds and will
fail the hermeticity gate.

Quote, in your Update Log, the `cargo test` output for the new tests and the
before/after of your mutation self-check (the failing assertion when
`input_tokens` subtracts only `cache_read`).

*(The live-network confirmation — that a real dispatched phase now records
non-zero `cache_read_tokens` in its `PhaseRun` and that `rexymcp costs` prices
them — is the milestone's exit criterion and is run by the **reviewer** at
approval, not by you. It cannot be done hermetically, so it is not your task.)*

## Authorizations

None. No new dependencies. No edits outside
`executor/src/ai/backends/openai.rs`. No edits to `docs/architecture.md`.

## Out of scope

- Any edit to `TokenBreakdown` (`types.rs`), the telemetry schema
  (`telemetry.rs`), pricing (`metrics.rs`), `costs.rs`, or config rates — all
  already consume `cache_write_tokens`; this phase only makes the parser *produce*
  it.
- The `[models] cache_read_per_mtok` / `cache_creation_per_mtok` **values** or the
  discount's use of architect-vs-executor rates — a pricing-model question logged
  as the M39 modeling caveat, not a code task.
- Sourcing cache stats from vLLM `/metrics` — moot; the per-request `usage` now
  carries the data.
- `multimodal_tokens` — ignore it.

## Update Log

(Filled in by the executor. See WORKFLOW.md § "Update Log entries".)

<!-- entries appended below this line -->
