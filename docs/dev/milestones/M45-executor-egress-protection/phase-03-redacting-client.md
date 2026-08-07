# Phase 3: RedactingAiClient outbound chokepoint

**Milestone:** M45 — Executor Egress Protection
**Status:** review
**Depends on:** phase-01, phase-02
**Estimated diff:** ~180 lines (module + tests)
**Tags:** language=rust, kind=feature, size=m

## Goal

The interception that actually stops PII reaching the cloud executor: an
`AiClient` decorator that redacts every outbound message — deterministic PII (live)
plus the phase-02 dictionary (names/addresses by exact substring) — to
`[REDACTED:kind]` **irreversibly**, then forwards to the wrapped client. One
chokepoint covers every content source (file reads, bash, search, verifier
output) because they all arrive as `Message`s.

## Architecture references

- `docs/dev/milestones/M45-executor-egress-protection/README.md` — why
  irreversible (the 06b corruption failure mode does not apply — nothing to
  reverse).
- `executor/src/ai/mod.rs` — `AiClient` trait (`chat` → `anyhow::Result<()>`),
  `Message`, `ToolResult { content }`.
- `executor/src/privacy/{detector,prescan}.rs` — `detect_deterministic`,
  `merge_spans`, `PiiIndex::redaction_terms`.

## Current state

- `PiiKind` has `token_prefix` but no redaction-marker tag. No PII redactor over
  free text exists in the privacy module (`security/redact.rs` redacts *secrets*).

## Spec

1. **`PiiKind::marker_tag(self) -> &'static str`** in `privacy/mod.rs`
   (`PersonName`→`"name"`, `Email`→`"email"`, `Phone`→`"phone"`, `Ssn`→`"ssn"`,
   `CreditCard`→`"card"`, `Ipv4`→`"ip"`, `Mac`→`"mac"`, `StreetAddress`→
   `"address"`, `Org`→`"org"`).

2. **`executor/src/privacy/redact.rs`** (new; `pub mod redact;`):
   - `pub fn redact_pii(text: &str, terms: &[(String, PiiKind)]) -> String`:
     `detect_deterministic` spans ∪ every occurrence of each `term` (substring),
     `merge_spans` (longest wins on overlap), replace right-to-left with
     `[REDACTED:{tag}]`.
   - `pub struct RedactingAiClient { inner: Box<dyn AiClient>, terms:
     Vec<(String, PiiKind)> }` implementing `AiClient`: redact `system_prompt` and
     each message's `content` + `tool_results[].content`, then `inner.chat`.

## Acceptance criteria

- [ ] `cargo build` / `clippy -D warnings` / `fmt --check` / `test` all pass.
- [ ] `redact_pii` turns an email into `[REDACTED:email]` and a dictionary name
      into `[REDACTED:name]`; overlapping terms prefer the longer.
- [ ] `RedactingAiClient::chat` forwards **redacted** `system_prompt` and message
      content to the inner client (asserted via `MockAiClient::calls`).
- [ ] `tool_results[].content` is redacted too.

## Test plan

`redact.rs`: `redacts_deterministic_email`, `redacts_dictionary_name`,
`longer_term_wins_on_overlap`, `leaves_clean_text_untouched`;
`redacts_outbound_messages_and_system_prompt` and `redacts_tool_result_content`
(`#[tokio::test]`, `MockAiClient` cloned handle asserting the forwarded content).

## End-to-end verification

Not applicable — the decorator forwards to a `MockAiClient` in tests; the live
wiring (engage for cloud endpoints via `make_client`) is phase-05, dogfooded there.

## Authorizations

- No new dependencies. New file `executor/src/privacy/redact.rs`.
- No `docs/architecture.md` edit.

## Out of scope

- Wiring the decorator into `make_client` for cloud endpoints — phase-05.
- The write-refuse guard — phase-04.

## Update Log

(Filled in by the executor. See WORKFLOW.md § "Update Log entries".)

<!-- entries appended below this line -->

### Update — 2026-08-07 16:49 (complete)

**Summary:** Added `PiiKind::marker_tag` and `executor/src/privacy/redact.rs`.
`redact_pii(text, terms)` unions deterministic spans with every substring match of
the pre-scan dictionary, merges (longest wins), and replaces right-to-left with
`[REDACTED:<tag>]` — irreversible. `RedactingAiClient` wraps a `Box<dyn AiClient>`
and redacts the system prompt + each message's `content` and
`tool_results[].content` before `inner.chat`, so every content channel is covered
at one chokepoint. `pub mod redact;`. No new dependencies.

**Acceptance criteria:** all ticked above.

**Commands:**

```
$ cargo fmt --all --check      # clean
$ cargo clippy --all-targets --all-features -- -D warnings   # Finished, clean
$ cargo test 2>&1 | grep "^test result"
test result: ok. 691 passed; 0 failed; 0 ignored; ...     (mcp)
test result: ok. 2 passed; 0 failed; 0 ignored; ...       (readme_config_reference)
test result: ok. 1130 passed; 0 failed; 3 ignored; ...    (executor lib: +6 redact)
```

Post-phase-02 baseline was 1817; now 1823 (+6 redact tests).

**End-to-end verification:** Not applicable — the decorator forwards to a
`MockAiClient`. `redacts_outbound_messages_and_system_prompt` asserts the inner
client receives `help [REDACTED:name] today` and
`[REDACTED:name]'s email is [REDACTED:email]`; `redacts_tool_result_content`
confirms a `read_file` tool result carrying a name + private IP is redacted before
forwarding. Live wiring (cloud endpoints via `make_client`) is phase-05.
