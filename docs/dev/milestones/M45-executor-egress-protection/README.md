# M45 — Executor Egress Protection

**Goal:** Stop repo PII from reaching a **cloud executor** (e.g. DeepSeek) when a
local executor isn't an option — by redacting PII out of everything the executor
sends to the model (irreversibly) and refusing model writes to PII-bearing files,
so no round-trip and no corruption.

**Status:** planning

**Depends on:** M44 (detectors, registry, `[privacy]` config, vault). This closes
the residual gap M44 documented and phase-06b proved could not be closed with a
reversible round-trip.

**Exit criteria:**

- [ ] When `[privacy].enabled` and the executor endpoint is a **cloud** host, no
      structured PII (email/phone/SSN/card/IP/MAC) appears in any message sent to
      the executor model — verified across `read_file`, `bash`, `search`, and
      verifier output.
- [ ] Names/addresses found by a one-time repo pre-scan are redacted from
      outbound messages by dictionary match (no per-turn model call).
- [ ] A `write_file` / `patch` targeting a PII-bearing file is refused with a
      model-visible error (not a crash), leaving the file untouched.
- [ ] A **local** executor endpoint bypasses all of this (no redaction, no
      write-refusal).
- [ ] Redaction is **irreversible** (`[REDACTED:kind]`) — no vault, no token the
      model can "correct" into fabricated data (the phase-06b failure mode).
- [ ] All four gates pass; each mechanism has a test that fails when reverted.

## Why irreversible redaction (not the M44 reversible round-trip)

Phase-06b's prototype proved a *reversible* executor round-trip corrupts files:
asked to validate/normalize a tokenized value, the model **replaces** the token
with fabricated data (`Email_1` → `person1@example.com`), losing the original.
M45 avoids this entirely by making redaction **one-way** (nothing to reverse, so
nothing to corrupt) and refusing writes to PII files **structurally** (no reliance
on the model preserving anything). Reversibility at the executor boundary is not
needed — the executor edits *code*; PII-bearing data files stay real on disk,
protected by the write-refusal.

## Design

Engaged iff `privacy.enabled && !endpoint_is_local(executor.base_url)`.

1. **Endpoint classification** — `endpoint_is_local(url)`: localhost / `127.0.0.1`
   / `::1` / RFC-1918 (`10.`, `172.16–31.`, `192.168.`) → local; else cloud.
2. **Pre-scan → PII dictionary** — before dispatch, scan the repo's files once
   (deterministic + NER), producing (a) a set of PII strings + kinds and (b) the
   set of files containing PII. Incremental via the M44 registry (content-hash),
   so NER runs only on new/changed files.
3. **Outbound redaction chokepoint** — a `RedactingAiClient` decorator wraps the
   cloud client; before every `chat`, each outgoing message is redacted:
   deterministic detectors (live) + dictionary substring-match (names/addresses)
   → `[REDACTED:kind]`. One chokepoint covers every content source. Cheap (regex +
   substring; no per-turn model call).
4. **Write-refuse guard** — `write_file` / `patch` to a file in the PII-file set
   returns a model-visible advisory error; the file is not written.

## Phases

| #  | Phase                                                         | Build     | Status |
|----|--------------------------------------------------------------|-----------|--------|
| 01 | `endpoint_is_local` classification + `[privacy]` egress config | architect | review ← active |
| 02 | repo pre-scan → PII dictionary + PII-file set (registry-cached) | architect | review |
| 03 | `RedactingAiClient` outbound chokepoint (deterministic + dictionary) | architect | review |
| 04 | write-refuse guard for PII-bearing files (`pii_write_refusal` in `egress`) | architect | review |
| 05 | wiring (engage for cloud endpoints) + config + docs + dogfood | architect | **deferred** — focused follow-up PR (invasive: core loop + ~23 LoopDeps sites); see doc |

## Notes

- **Best-effort remains best-effort.** Structured PII is redacted reliably; names
  the pre-scan NER misses still reach the cloud executor. M45 is a large reduction
  in egress risk, not an airtight guarantee — same honest limit as all of M44.
- **Executor quality on data files.** The executor reads `[REDACTED:...]` where PII
  was, for PII-bearing files it cannot edit anyway. Code files (no PII) are
  untouched, so normal coding work is unaffected.
- **Reuse, don't rebuild.** Extend `security/redact.rs` (already does irreversible
  `[REDACTED:...]`) with the M44 detector kinds; reuse `privacy::registry` for the
  incremental pre-scan and `privacy::detector` for the live pass.
- **Marker-in-code edge case.** If the model copies a `[REDACTED:...]` marker into
  a *non*-PII file it writes, the literal marker lands there — visible garbage the
  review catches, not a PII leak. A marker-guard on writes is a possible later
  refinement.
