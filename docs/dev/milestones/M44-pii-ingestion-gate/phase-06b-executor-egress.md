# Phase 6b: Executor egress round-trip (DEFERRED — design + prototype plan)

**Milestone:** M44 — PII Ingestion Gate
**Status:** todo (deferred — do not implement without the prototype below)
**Depends on:** phase-01, phase-02, phase-03, phase-04, phase-06a
**Estimated diff:** unknown until prototyped
**Tags:** language=rust, kind=feature, size=l

## Why this is deferred, not skipped

When the executor endpoint is a **cloud** provider (this repo runs
`deepseek-v4-flash`), the executor's outbound prompts are a PII egress: the model
reads repo files and they go to the cloud. Full enforcement would (a) anonymize
outbound prompts and (b) reconstitute token→original on `write_file`/`patch`
before edits hit disk. Two hard problems block a naïve implementation, so this is
written down and deferred rather than rushed:

### Problem 1 — NER does not scale to per-turn interception

Anonymizing every outbound message with the Qwen NER engine adds a **Qwen
round-trip to every DeepSeek turn** — slow, and the growing context can exceed the
engine's window. Deterministic detection scales (phase-06a uses it), but it only
covers structured PII; names/addresses need the model. Per-turn NER is not viable.

**Likely answer:** don't scrub per-message. Pre-scrub the repo's PII-bearing
source files **once** via the phase-04 ingestion registry (incremental, model runs
only on new/changed files), and have the executor read the *tokenized* view — then
only reconstitution-on-write happens per turn (cheap, deterministic vault lookup).
This turns an unbounded per-turn cost into a bounded per-ingest one.

### Problem 2 — token round-trip robustness

Reconstitution on write assumes DeepSeek echoes tokens back **verbatim**. If the
model reformats `Person_1` (`person 1`, `Person1`, a translation, a rename), the
reverse lookup misses and either a token leaks into the repo or real content is
lost. This must be measured against the live model before any write path trusts
it.

## Prototype plan (do this first, gate the phase on the result)

1. Point the executor at DeepSeek. Feed a phase whose input contains tokens
   (`Person_1`, `Email_2`) and measure how often they survive a full turn
   verbatim in `write_file`/`patch` arguments. If survival is not ~100%, the
   write-reconstitution design needs a more robust token format (e.g. a rare
   sentinel wrapper the model won't touch) or a different approach entirely.
2. Prototype the read/write interception (Problem 1's "answer") on one
   PII-bearing fixture file and confirm the on-disk file ends up with **real**
   content (no leaked tokens) after an edit.

## Sketch of the intended design (subject to the prototype)

- **Read/ingest:** before dispatch, run the ingestion registry (phase-04) over
  the repo's configured data/fixture paths; the executor's file reads return the
  tokenized content, sharing the phase's vault.
- **Write:** wrap `write_file`/`patch` so tool arguments are
  `vault.reconstitute`-d (deterministic token→original) before the write lands.
- **Endpoint gate:** only engage when the executor endpoint is non-local (a LAN
  Qwen executor needs none of this); classify by host.
- **Shared vault:** the same vault phase-06a uses, so Claude's tokens and the
  executor's tokens agree.

## Out of scope for M44 (candidate follow-up milestone)

Given the two unsolved problems, full executor-egress enforcement is a **separate
milestone candidate**, not an M44 close blocker. M44 ships: the engine (01–03),
the incremental registry (04), the CLI (05), and the deterministic PhaseResult
boundary scrub (06a) — a usable, honest gate. This doc is the spec seed for the
follow-up.

## Update Log

(None — not implemented. This phase is a deferred design record.)

<!-- entries appended below this line -->
