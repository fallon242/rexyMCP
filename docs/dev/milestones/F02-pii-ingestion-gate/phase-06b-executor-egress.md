# Phase 6b: Executor egress round-trip (DEFERRED — design + prototype plan)

**Milestone:** M44 — PII Ingestion Gate
**Status:** **won't-implement-as-designed** — prototype executed 2026-08-07 and
disproved the design (confirmed data corruption). See "Prototype results" below.
**Depends on:** phase-01, phase-02, phase-03, phase-04, phase-06a
**Estimated diff:** unknown until prototyped
**Tags:** language=rust, kind=feature, size=l

## Prototype results (2026-08-07) — decisive: do NOT build the naive round-trip

Ran against the live DeepSeek executor (`deepseek-v4-flash`), feeding content
containing pseudonym tokens (`Person_1`, `Email_1`, `Phone_1`, `Address_1`):

| Scenario | Token behavior |
|---|---|
| A. Structured edit (add a JSON field) | all tokens **survived verbatim** ✓ |
| B. Prose rewrite ("be concise") | all tokens **survived verbatim** ✓ |
| C. Code generation ("write a function using these") | survived, but the model was visibly **confused** — it spent its whole output trying to interpret `Email_1` as a fill-in placeholder |
| D. Validate/normalize ("fix the invalid email/phone") | **CORRUPTION**: `Email_1` → `person1@example.com`, `Phone_1` → `+15551234567` — tokens **replaced with fabricated values** |

**Verdict.** Verbatim survival holds only for *faithful* edits. The moment the
model is asked to validate, normalize, complete, or "fix" a tokenized value — a
routine executor action — it **replaces the token with a plausible fabricated
value**, which silently (a) loses the real PII (reconstitution has nothing to
reverse) and (b) writes invented data into the repo. Tokenizing the executor's
working content is **unsafe as a general mechanism**. The prototype earned its
keep by proving the design would corrupt files.

**Decision: do not implement the tokenize-the-executor's-view round-trip.**

### Safe alternatives (what to do instead)

1. **Local executor for PII-bearing repos** — a LAN Qwen executor has no cloud
   egress ② at all; no round-trip needed. The clean answer.
2. **Pre-scrub + faithful-edit discipline** — scrub data files via the CLI /
   ingestion registry before dispatch and keep the executor on faithful edits;
   still exposed to scenario D, so pair with (3).
3. **Token-integrity guard (future, if ever attempted)** — wrap writes so that a
   token present in the pre-image but absent from the write **blocks the write and
   surfaces it**, turning silent corruption into a loud failure. A guard, not a
   fix.

The shipped M44 protections stand: the deterministic `PhaseResult` boundary scrub
(06a) protects egress ① to Claude, and the CLI protects hand-typed input. Egress
② — a *cloud* executor over a PII-bearing repo — is a **documented residual
risk**, not something this milestone can safely automate.

## Why this was deferred, not skipped

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

<!-- entries appended below this line -->

### Update — 2026-08-07 16:10 (prototype — won't implement)

**Ran the prototype against live DeepSeek** (`deepseek-v4-flash`, key from
`REXYMCP_API_KEY`). Four scenarios; results in "Prototype results" above. Scenario
D is decisive: asked to "fix invalid" values, the model **replaced** `Email_1` →
`person1@example.com` and `Phone_1` → `+15551234567` — losing the real PII and
writing fabricated data. Tokenizing the executor's view is unsafe as a general
mechanism.

**Outcome:** phase **not implemented, by design**. The corrupting round-trip is
abandoned; safe alternatives (local executor / pre-scrub / a future
token-integrity write-guard) are recorded above. M44's shipped protections (06a
boundary scrub + the CLI) are unaffected; a cloud executor over a PII-bearing repo
is a documented residual risk. No code changed in this step — findings only.
