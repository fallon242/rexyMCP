# Phase 3: Qwen NER engine + gateway

**Milestone:** M44 — PII Ingestion Gate
**Status:** review
**Depends on:** phase-01, phase-02
**Estimated diff:** ~320 lines (two modules + tests)
**Tags:** language=rust, kind=feature, size=m

## Goal

Catch the PII deterministic detectors cannot — person names, street addresses,
organizations — with the **local Qwen model**, and combine it with the
deterministic pass behind one `Gateway::anonymize` call. This is where the
`[privacy]` engine (Qwen3.5 on the LAN) is finally wired in; detection stays on
the LAN, so it never leaks.

## Architecture references

Read before starting:

- `docs/dev/milestones/M44-pii-ingestion-gate/README.md` — engine-vs-executor,
  thinking-off requirement, bias-to-false-positive.
- `executor/src/ai/mod.rs` — the `AiClient` trait (streaming `AiEvent`s over a
  channel), `SamplingParams`, `OpenAiClient::new`, `make_client`.
- `executor/src/ai/testing.rs` — `MockAiClient` (one scripted string per `chat`).
- `docs/dev/STANDARDS.md` §3.4 (live-LLM tests are opt-in, `#[ignore]`-gated).

## Pre-flight

1. Read `docs/dev/STANDARDS.md` top to bottom.
2. Read the phase-01/02 privacy code and the `ai` module interface above.
3. Read this entire phase doc.
4. Clean branch (`m44-pii-ingestion-gate`).

## Current state

- `privacy/` has deterministic detection (`detector::detect_deterministic`,
  `merge_spans`), the reversible `TokenMap`, and the encrypted `Vault`. No model
  is called anywhere in `privacy/`.
- `AiClient::chat(system, messages, tx, tools)` streams `AiEvent::Token(String)`
  chunks to an `mpsc` sender and returns when done (dropping `tx`, closing the
  channel). `OpenAiClient::new(api_key, model, base_url, first_token_timeout,
  stream_idle_timeout, SamplingParams)`; `SamplingParams.enable_thinking = false`
  sends `chat_template_kwargs.enable_thinking = false` — the knob that made Qwen
  return direct JSON in the M44 spike.

## Spec

1. **NER engine** — new `executor/src/privacy/ner.rs`.
   - `NerEngine { client: Box<dyn AiClient> }`, `new(Box<dyn AiClient>)` (for
     tests) and `from_config(&PrivacyConfig) -> Result<Self>` building an
     `OpenAiClient` at the `[privacy]` endpoint/model with `temperature = 0`,
     `max_tokens = 1024`, `enable_thinking = false`; errors (`Error::Privacy`) if
     endpoint or model is unset.
   - `async fn detect(&self, text: &str) -> Result<Vec<PiiSpan>>`: empty/blank
     input returns no spans without calling the model; otherwise send a
     PII-extraction system prompt + the text, collect the streamed tokens into one
     string, extract the JSON array (tolerating prose / code fences — slice from
     first `[` to last `]`), parse `[{ "text", "type" }]`, and emit a `PiiSpan`
     for **every** occurrence of each reported `text` in the source (located by
     substring). Unparseable output yields no spans (best-effort). Map labels →
     `PiiKind` (`address`→`StreetAddress`, `org`→`Org`, else `PersonName` — an
     unknown-but-flagged artifact is still anonymized, the safe direction).

2. **Gateway** — new `executor/src/privacy/gateway.rs`.
   - `Gateway { ner: NerEngine }`, `new(NerEngine)`.
   - `async fn anonymize(&self, text: &str, map: &mut TokenMap) -> Result<String>`:
     run `detect_deterministic`, then add each NER span that does **not** overlap
     a deterministic span (deterministic wins — a validator beats a guess),
     `merge_spans`, and `map.anonymize`.

3. **Module declarations** — `pub mod ner;` and `pub mod gateway;` in
   `privacy/mod.rs`.

## Acceptance criteria

- [ ] `cargo build` / `clippy -D warnings` / `fmt --check` / `test` all pass.
- [ ] With a `MockAiClient` returning `[{"text":"Alice","type":"person_name"}]`,
      `Gateway::anonymize("Alice emailed a@b.com", &mut map)` yields
      `"Person_1 emailed Email_1"` and `map.reconstitute` inverts it.
- [ ] When the model wrongly labels an email as a person name, the deterministic
      `Email` span wins (token is `Email_*`, not `Person_*`).
- [ ] NER JSON wrapped in prose / ```json fences is still parsed.
- [ ] Unparseable model output yields no NER spans (no error).
- [ ] Blank input does not call the model.

## Test plan

Hermetic `#[tokio::test]` with `MockAiClient` (in `ner.rs` and `gateway.rs`):

- `ner`: `detects_names_from_json_response`,
  `extracts_json_wrapped_in_prose_and_fences`,
  `maps_address_label_and_locates_every_occurrence`,
  `unparseable_response_yields_no_spans`, `empty_input_skips_the_model` (assert
  `mock.calls()` is empty via a cloned handle).
- `gateway`: `anonymizes_deterministic_and_ner_together`,
  `deterministic_wins_over_overlapping_ner_span`, `ner_only_text_is_anonymized`.

Plus **one live-LLM test** (authorized below), `#[ignore]`-gated: `from_config`
against the real Qwen endpoint, asserting two person names in a sentence are
found. Never runs on CI.

## End-to-end verification

The hermetic tests use a mock; the `#[ignore]` live test is the real end-to-end
check against Qwen. Run it and quote its result in the completion Update Log:

```
cargo test -p rexymcp-executor privacy::ner::tests::live_qwen -- --ignored --nocapture
```

## Authorizations

- One live-LLM `#[ignore]` test hitting the `[privacy]` engine endpoint
  (`http://192.168.50.138:8080/v1`, `qwen3.5-9b`).
- No new dependencies.
- New files: `executor/src/privacy/{ner,gateway}.rs`.
- No `docs/architecture.md` edit.

## Out of scope

- Persisting through the `Vault` inside the gateway — the gateway takes a
  `&mut TokenMap`; wiring a `Vault` around it is phase-06.
- Content-hash change tracking / incremental ingestion — phase-04.
- CLI — phase-05.
- Executor/`PhaseResult`/prompt-hook enforcement — phase-06/07.
- Confidence scoring, batching, or chunking long inputs — not required now.

## Update Log

(Filled in by the executor. See WORKFLOW.md § "Update Log entries".)

<!-- entries appended below this line -->

### Update — 2026-08-07 15:15 (complete)

**Summary:** Added `executor/src/privacy/ner.rs` (`NerEngine`) and `gateway.rs`
(`Gateway`). `NerEngine` wraps a `Box<dyn AiClient>` (mockable); `from_config`
builds an `OpenAiClient` at the `[privacy]` endpoint with `temperature=0`,
`max_tokens=1024`, `enable_thinking=false`. `detect` skips the model on blank
input, otherwise streams the completion, slices the JSON array from first `[` to
last `]` (tolerating prose / ```json fences), parses `[{text,type}]`, and emits a
span for every occurrence of each reported text; unparseable output yields no
spans. `Gateway::anonymize` runs `detect_deterministic`, adds only NER spans that
do not overlap a deterministic one (deterministic wins), merges, and tokenizes
through the `TokenMap`. `pub mod ner/gateway` added to `privacy/mod.rs`. No new
dependencies, no deviations from the spec.

**Acceptance criteria:** all ticked above.

**Commands:**

```
$ cargo fmt --all --check      # clean
$ cargo build                  # Finished, zero warnings
$ cargo clippy --all-targets --all-features -- -D warnings   # Finished, clean
$ cargo test 2>&1 | grep "^test result"
test result: ok. 685 passed; 0 failed; 0 ignored; ...     (mcp)
test result: ok. 2 passed; 0 failed; 0 ignored; ...       (readme_config_reference)
test result: ok. 1106 passed; 0 failed; 3 ignored; ...    (executor lib)
```

Post-phase-02 baseline was 1785; now 1793 (+8 hermetic: 5 `ner`, 3 `gateway`).
The 3rd ignored test is the live Qwen check below.

**End-to-end verification:** Ran the live `#[ignore]` test against the real Qwen
engine (`http://192.168.50.138:8080/v1`, `qwen3.5-9b`):

```
$ cargo test -p rexymcp-executor privacy::ner::tests::live_qwen -- --ignored --nocapture
running 1 test
test privacy::ner::tests::live_qwen_detects_person_names ... ok
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 1108 filtered out; finished in 2.12s
```

`NerEngine::from_config` → `OpenAiClient` → Qwen returned JSON that parsed to
spans for both "John Smith" and "Maria Gonzalez" in the test sentence — the real
LAN detection path, thinking-off, working end to end.
