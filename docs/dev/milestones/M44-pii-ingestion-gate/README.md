# M44 — PII Ingestion Gate

**Goal:** No PII artifact reaches a cloud model — Claude (architect) or DeepSeek
(executor) — un-anonymized: every input (prompts, documents, data) is processed
by a local PII engine before it enters the pipeline, with a reversible local
vault to reconstitute originals on demand.

**Status:** planning

**Depends on:** none (new subsystem). Sequence *after* M43 lands — see Notes.

**Exit criteria:**

- [ ] A configured local PII engine (Qwen3.5 on the LAN) plus deterministic
      detectors replace every detected PII artifact in ingested content with a
      stable, reversible pseudonym token before that content reaches Claude or
      DeepSeek.
- [ ] Originals are recoverable only from a **local, encrypted-at-rest,
      git-ignored** vault; the same original always maps to the same token, and
      distinct originals never collide.
- [ ] Ingestion is incremental: a source document or data element is (re)scrubbed
      only when it is new or its content hash changed; unchanged PII reuses its
      existing vault token.
- [ ] The executor's outbound prompts to DeepSeek carry only anonymized content,
      and token→original is reconstituted on `write_file`/`patch` before edits
      land on disk (no pseudonyms leak into the repo).
- [ ] The `PhaseResult` (diff, command outputs, update log, briefing) is scrubbed
      before it crosses the MCP boundary back to Claude.
- [ ] A `UserPromptSubmit` hook scrubs the human's typed prompt before it reaches
      the architect.
- [ ] Each mechanism has a test that fails when the mechanism is reverted; all
      four gates pass.

## Architecture references

- `docs/architecture.md#error-model` — model-visible outcomes vs. `Error`.
- `docs/architecture.md` § "The three layers" — where the engine, the boundary
  enforcement, and the `UserPromptSubmit` hook sit (Layer 1 / 2 / 3).
- `executor/src/security/redact.rs` — the existing *irreversible* secret
  redactor. M44's vault is deliberately the opposite (reversible); the two
  coexist. Reversibility is the added risk this milestone chooses.

## The threat model

Three cloud egress points; one local engine sits in front of all of them.

```
your prompt ─▶ Claude architect (CLOUD ①) ─▶ execute_phase ─▶ DeepSeek executor (CLOUD ②)
                    ▲                                              │ reads target repo
                    └──── PhaseResult / diff / briefing ◀──────────┘  (return path, CLOUD ①)
        Qwen3.5 @ 192.168.50.138:8080 = local PII engine, never on any cloud path
```

- **① Architect (Claude).** Protected two ways: the `UserPromptSubmit` hook
  scrubs the human's prompt on the way in; the `PhaseResult` scrub protects the
  way out.
- **② Executor (DeepSeek, cloud).** Its outbound prompts (repo file contents,
  phase doc) are anonymized before the AI client sends them; token→original is
  reconstituted on writes so the on-disk repo is never pseudonymized.

"Processed on any new or changed source document or data" is the **ingestion
registry**: a content-hash manifest that makes scrubbing incremental and keeps
pseudonyms stable across edits.

## Detection is best-effort — design consequences

The PII engine reduces leak risk; it does not eliminate it. Two rules follow:

1. **Deterministic-first.** Structured PII (email, phone, SSN, credit card via
   Luhn, IP, MAC) is caught by regex/validators, which are reliable. The LLM is
   used **only** for unstructured PII (person names, street addresses, orgs),
   where it *will* have false negatives. Deterministic spans win on overlap.
2. **The vault is a honeypot.** It concentrates every original in one place, so
   it is encrypted at rest (XChaCha20-Poly1305, key in a `0600` local key file),
   git-ignored, local-only, and reconstitution is explicit/gated.

## Phases

| #  | Phase                                                        | Build      | Status |
|----|-------------------------------------------------------------|------------|--------|
| 01 | privacy foundation: `[privacy]` config + deterministic detectors + stable tokenizer | architect | review ← active |
| 02 | encrypted vault: persist/load TokenMap, XChaCha20-Poly1305, local key | architect | review |
| 03 | Qwen NER engine + gateway (deterministic ∪ NER → tokenize → vault) | architect | review |
| 04 | ingestion registry: content-hash change tracking, incremental scrub | architect | review |
| 05 | CLI: `anonymize` / `reconstitute` / `vault`                 | architect | review |
| 06a | `PhaseResult` boundary scrub (deterministic) before it returns to Claude | architect | review |
| 06b | executor→DeepSeek egress round-trip | architect | **deferred** (NER doesn't scale per-turn; see doc) |
| 07 | `UserPromptSubmit` hook + `rexymcp init` `[privacy]` defaults + docs | architect | review |

**Hybrid build (why this split):** the security-critical core — the reversible
tokenizer (01), vault encryption (02), and the boundary round-trip (06) — is
authored by the architect, because a false negative or a botched reconstitution
is a data leak or a corrupted repo. The mechanical, well-bounded phases (CLI glue
05, hook + init defaults + docs 07) dispatch to the Qwen executor. Phases expand
on demand; 06 in particular may split once prototyped against live DeepSeek (see
Notes). Phase-01 is deliberately dependency-free (pure detectors + tokenizer);
crypto lands in 02, the first model call in 03, the first hash dep in 04.

## Notes

- **PII engine config (not the executor).** Qwen3.5 is the *detection engine
  only*; the executor stays `deepseek-v4-flash`. The engine endpoint is
  `http://192.168.50.138:8080/v1`, model `qwen3.5-9b`, served by llama.cpp.
  Phase-01 adds a `[privacy]` config section carrying this endpoint separately
  from `[executor]`.
- **Qwen is a reasoning model — thinking must be off.** Verified 2026-08-07:
  with thinking on it spends the entire token budget on `reasoning_content` and
  returns empty `content` (`finish_reason: length`). With
  `chat_template_kwargs.enable_thinking = false` it returned clean NER JSON in 90
  tokens and correctly found all five PII artifacts in a test sentence (two
  names, an email, a phone, a street address). The `[privacy]` engine call must
  send thinking-disabled.
- **Sequence after M43.** M43 phase-01 adds `deny_unknown_fields` to config
  override tables. A new top-level `[privacy]` section must be added to the
  config *type* (phase-01 here), not just the file, or a stricter parser will
  reject it. Land M43 first, then M44.
- **The executor round-trip (05) is the hard part.** Pseudonym tokens must be
  robust to being echoed back by DeepSeek unmangled, or reconstitution corrupts
  a write. Prototype token format against live DeepSeek before committing 05;
  split if needed.
- **Pseudonymized executor view.** The executor reasons over tokenized content.
  Source *code* rarely contains PII, so this is usually a no-op; PII lives mostly
  in data/fixture files, which anonymize cleanly. Load-bearing-name edge cases
  are noted per-phase.
- **This milestone cannot retro-protect an existing chat.** Claude is the cloud
  architect; PII already pasted into a session is already in the cloud. The gate
  protects *future* ingestion. Do not test with real PII until phase-04 ships the
  CLI and phase-06 ships the hook.
- **New dependencies** (authorized per-phase): an AEAD crate
  (`chacha20poly1305`) and a hash crate (`sha2` or `blake3`) — neither is in the
  workspace today.
