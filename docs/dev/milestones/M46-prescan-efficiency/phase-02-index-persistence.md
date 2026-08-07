# Phase 2: Encrypted PiiIndex persistence

**Milestone:** M46 — Pre-scan Efficiency
**Status:** review
**Depends on:** phase-01; M45 pre-scan; M44 vault crypto
**Estimated diff:** ~180 lines (seal module + refactor + persistence + tests)
**Tags:** language=rust, kind=feature, size=m

## Goal

Make the executor-egress pre-scan **incremental across dispatches**: persist the
`PiiIndex` so an unchanged file reuses its prior entry (no NER call) — turning the
current full-scan-every-dispatch into a scan of only new/changed files. Because
the index holds PII strings, it is persisted **encrypted**.

## Spec

1. **`privacy::seal`** (new module) — reusable XChaCha20-Poly1305:
   `load_or_create_key(dir)` (`0600` key file), `seal(key, bytes)`,
   `unseal(key, blob)` (`[24-byte nonce ‖ ciphertext]`). Extracted from the vault.
2. **Refactor `vault.rs`** to use `seal` (was inline crypto) — behavior-preserving;
   the five existing vault tests are the guard.
3. **`PiiIndex`**: derive `Serialize`/`Deserialize`; `load(dir)` (sealed
   `dir/egress-index.enc`, empty if absent) / `save(dir)` (seal + atomic write, and
   a `*` `.gitignore`).
4. **`build_egress_index`** resolves the vault dir (`privacy.vault_dir` or
   `<repo>/.rexymcp/vault`), loads the prior index + a registry
   (`egress-registry.json`), builds with them, and saves both — so
   `build_pii_index`'s registry-skip is finally effective.

## Acceptance criteria

- [ ] Build / clippy `-D warnings` / fmt / test pass; the five vault tests still
      pass (refactor is behavior-preserving).
- [ ] A `PiiIndex` round-trips through `save`/`load`, and the on-disk
      `egress-index.enc` does not contain a plaintext PII string.
- [ ] A missing index loads as empty.
- [ ] `build_egress_index` still works live against Qwen (`#[ignore]`).

## Test plan

`seal.rs`: `seal_unseal_roundtrips`, `key_is_owner_only`. `prescan.rs`:
`index_persists_encrypted_across_reload`, `load_missing_index_is_empty`; the
existing `unchanged_file_reuses_cache_without_ner` / `changed_file_is_rescanned`
cover the reuse logic the persistence now feeds across dispatches.

## Authorizations

No new dependencies (`chacha20poly1305` already present). No `docs/architecture.md`
edit. New files: `executor/src/privacy/seal.rs`.

## Out of scope

- A cache-eviction / TTL policy for stale index entries (removed files linger
  until the vault dir is cleared) — a later refinement.

## Update Log

<!-- entries appended below this line -->

### Update — 2026-08-07 18:05 (complete)

**Summary:** Extracted `privacy::seal` (XChaCha20-Poly1305 key + seal/unseal) and
refactored `vault.rs` onto it (five vault tests still green → behavior-preserving).
`PiiIndex` now derives serde and has sealed `load`/`save`;
`build_egress_index` loads the prior index + a registry from the vault dir, builds
with them, and saves both — so an unchanged file reuses its entry and skips NER on
the next dispatch. Verified live against Qwen.

**Commands:** `cargo fmt --all --check` clean; `clippy -D warnings` clean;
`cargo test` → 691 + 2 + 1140 = **1833**, 0 failed (+4: 2 `seal`, 2 index
persistence).

**End-to-end verification:** `index_persists_encrypted_across_reload` writes a
`PiiIndex`, confirms the on-disk blob has no plaintext "Alice", and reloads it;
the live `build_egress_index` test (against Qwen) still passes with the new
load/save I/O in place.