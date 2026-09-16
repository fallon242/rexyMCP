# M46 — Pre-scan Efficiency

**Goal:** Make the M45 executor-egress pre-scan practical on real repos — bound
what it walks, and stop re-running NER on unchanged files every dispatch.

**Status:** done (both phases approved_first_try, 2026-08-21)

**Depends on:** M45 (the pre-scan + `build_egress_index`).

**Why:** M45's dogfood confirmed the mechanism, but `build_egress_index` currently
full-scans every dispatch (the registry marks content hashes, but the prior
`PiiIndex` is not persisted, so nothing is reused). On a large repo that is one
NER call per file, per dispatch — impractical. Two follow-ups fix it.

## Phases

| #  | Phase                                                          | Status |
|----|----------------------------------------------------------------|--------|
| 01 | `[privacy] scan_globs` — limit which files the pre-scan walks   | done |
| 02 | encrypted `PiiIndex` persistence — reuse unchanged files (skip NER) | done |

## Notes

- **Index persistence must be encrypted.** The `PiiIndex` holds PII strings
  (names/emails), so it is a honeypot like the vault — persisted sealed
  (XChaCha20-Poly1305 under the vault key), never plaintext.
- `scan_globs` uses `globset` (already a dep); patterns match repo-relative paths.
