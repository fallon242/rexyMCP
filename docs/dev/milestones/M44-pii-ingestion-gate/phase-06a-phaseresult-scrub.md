# Phase 6a: PhaseResult boundary scrub (deterministic)

**Milestone:** M44 — PII Ingestion Gate
**Status:** review
**Depends on:** phase-01, phase-02
**Estimated diff:** ~90 lines (fn + wiring + tests)
**Tags:** language=rust, kind=feature, size=s

## Goal

Stop structured PII (email/phone/SSN/card/IP/MAC) in an executor's `PhaseResult`
from reaching Claude un-tokenized. Every `execute_phase` / `continue_phase`
response is scrubbed at the MCP boundary, with the reversible mapping recorded in
the vault so the human can reconstitute locally.

## Why split from the original phase-06

The original phase-06 ("boundary enforcement") bundled two things: this
return-path scrub **and** the executor→DeepSeek outbound round-trip. Building it
surfaced a hard constraint: **NER does not scale to a boundary scrub.** Running
Qwen over an arbitrarily large diff is unbounded and context-limited, so it cannot
run synchronously on every `PhaseResult`. This phase therefore does a
**deterministic-only** scrub (regex/validators — linear, bounded, no model). The
NER-based scrub and the executor round-trip, which share that scaling problem, are
deferred to **phase-06b** with a prototype plan.

## Architecture references

- `mcp/src/server.rs` — `execute_phase_inner` / `continue_phase_inner`; the
  `cap::cap_phase_result` call is the existing "sanitize before Claude" chokepoint.
- `executor/src/privacy/tokenizer.rs` — `TokenMap::anonymize_text` (deterministic
  detection + tokenize) and `reconstitute`.
- `executor/src/privacy/vault.rs` — the encrypted store the mapping lands in.

## Current state

- `server.rs` returns `cap::cap_phase_result(result)` directly to Claude — no PII
  scrub. `mcp/src/privacy_cli.rs` already has `resolve_vault_dir` + the Vault API.

## Spec

1. **`scrub_phase_result`** — in `mcp/src/privacy_cli.rs`.
   `scrub_phase_result(result: PhaseResult, privacy: &PrivacyConfig, repo: &Path)
   -> Result<PhaseResult>`. Disabled gate → identity. Otherwise open the vault,
   `serde_json::to_string(&result)`, `vault.map_mut().anonymize_text(&json)`
   (deterministic-only), `vault.save()`, `serde_json::from_str` back. Scrubbing
   the serialized form catches **every** string field (diff, command outputs,
   update log, briefing, working-file content, diagnostics) with no hand-walking;
   structural fields (paths, status, line numbers) don't match the patterns, so
   they survive, and tokens (`Word_N`) are valid JSON so the value re-parses.

2. **Wire it** — in `server.rs`, before both `cap::cap_phase_result(result)`
   calls: `let result = crate::privacy_cli::scrub_phase_result(result,
   &cfg.privacy, &repo_path).map_err(|e| e.to_string())?;`. Scrub-before-cap so a
   PII value is never split by truncation (a truncated *token* is harmless).

## Acceptance criteria

- [ ] `cargo build` / `clippy -D warnings` / `fmt --check` / `test` all pass.
- [ ] A disabled `[privacy]` gate returns the `PhaseResult` unchanged.
- [ ] With the gate enabled, an email in `diff` is replaced by `Email_1` and the
      vault reconstitutes it back to the original.
- [ ] Both `server.rs` boundary sites apply the scrub before capping.

## Test plan

`privacy_cli.rs` tests: `scrub_disabled_returns_result_unchanged`,
`scrub_tokenizes_structured_pii_and_vault_reconstitutes`.

## End-to-end verification

Not applicable as a standalone binary invocation — the scrub runs inside the MCP
`execute_phase` handler, exercised by the round-trip unit test against a real
`TempDir` vault (serialize → deterministic scrub → re-parse → reconstitute). A
full live dispatch exercising it belongs to the M44 close dogfood.

## Authorizations

- No new dependencies. New symbol only (`scrub_phase_result`); edits to
  `server.rs`. No `docs/architecture.md` edit.

## Out of scope

- NER (names/addresses) at the boundary, and the executor→DeepSeek outbound
  anonymize + write-reconstitute round-trip — **phase-06b** (deferred; see its
  doc for the scaling problem and prototype plan).
- Scrubbing the CLI `run-phase` output (local human tool; owns the vault).

## Update Log

(Filled in by the executor. See WORKFLOW.md § "Update Log entries".)

<!-- entries appended below this line -->

### Update — 2026-08-07 15:47 (complete)

**Summary:** Added `scrub_phase_result` to `mcp/src/privacy_cli.rs` and wired it
into both `server.rs` return paths (`execute_phase` + `continue_phase`) ahead of
`cap_phase_result`. It serializes the `PhaseResult`, runs the deterministic
`anonymize_text` over the JSON (so every string field is covered without
hand-walking; structural fields don't match the patterns and survive), saves the
vault, and re-parses. Disabled `[privacy]` is a no-op. **Scope deviation, by
design:** the original phase-06 also covered NER-at-the-boundary and the executor
egress round-trip; both hit the same NER-doesn't-scale constraint and were split
out to phase-06b (drafted, deferred). No new dependencies.

**Acceptance criteria:** all ticked above.

**Commands:**

```
$ cargo fmt --all --check      # clean
$ cargo build                  # Finished, zero warnings
$ cargo clippy --all-targets --all-features -- -D warnings   # Finished, clean
$ cargo test 2>&1 | grep "^test result"
test result: ok. 690 passed; 0 failed; 0 ignored; ...     (mcp: +2 scrub)
test result: ok. 2 passed; 0 failed; 0 ignored; ...       (readme_config_reference)
test result: ok. 1113 passed; 0 failed; 3 ignored; ...    (executor lib)
```

Post-phase-05 baseline was 1803; now 1805 (+2 scrub tests).

**End-to-end verification:** Not applicable — no standalone binary entrypoint; the
scrub lives inside the MCP handler. `scrub_tokenizes_structured_pii_and_vault_reconstitutes`
drives the real serialize→scrub→re-parse→reconstitute path against a `TempDir`
vault: an email in `diff` becomes `Email_1`, and a freshly-opened `Vault`
reconstitutes it to the original.