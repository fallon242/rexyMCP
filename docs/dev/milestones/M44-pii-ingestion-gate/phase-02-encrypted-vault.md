# Phase 2: Encrypted vault — durable, reversible secure dictionary

**Milestone:** M44 — PII Ingestion Gate
**Status:** review
**Depends on:** phase-01
**Estimated diff:** ~280 lines (module + tests)
**Tags:** language=rust, kind=feature, size=m

## Goal

Make the `TokenMap` durable and secure: persist it to disk **encrypted at rest**
(XChaCha20-Poly1305 under a local key file) in a git-ignored directory, and
reload it so pseudonyms stay stable across process restarts. This is the durable
half of the "secure dictionary" the milestone requires — the honeypot, contained.

## Architecture references

Read before starting:

- `docs/dev/milestones/M44-pii-ingestion-gate/README.md` § "Detection is
  best-effort" — the vault is a PII honeypot; encryption + git-ignore + `0600`
  key are how that risk is contained.
- `executor/src/privacy/tokenizer.rs` — the `TokenMap` this persists.
- `docs/dev/STANDARDS.md` §2.1 (error model), §2.6 (dependencies), §3.3 (tests).

## Pre-flight

1. Read `docs/dev/STANDARDS.md` top to bottom.
2. Read the phase-01 code (`privacy/{mod,tokenizer}.rs`).
3. Read this entire phase doc.
4. Clean branch (`m44-pii-ingestion-gate`).

## Current state

- `TokenMap` (phase-01) holds `forward`/`reverse`/`counters` privately, with no
  serialization and no persistence. `PiiKind` derives no serde traits.
- `error::Error` has `Config`/`Io`/`Backend`/`Internal`. No crypto dependency is
  in the workspace.

## Spec

1. **Add the dependency** — `chacha20poly1305 = "0.10"` to
   `[workspace.dependencies]` in the root `Cargo.toml` and
   `chacha20poly1305.workspace = true` to `executor/Cargo.toml`. (Authorized
   below.)

2. **Serializable `PiiKind`** — in `privacy/mod.rs`, add `Serialize, Deserialize`
   to `PiiKind`'s derives (unit variants serialize as strings).

3. **`VaultEntry` + snapshot/restore on `TokenMap`** — in `privacy/tokenizer.rs`:
   `pub struct VaultEntry { token, original, kind: PiiKind }` (`Serialize,
   Deserialize`). `TokenMap::entries(&self) -> Vec<VaultEntry>` flattens `reverse`;
   `TokenMap::from_entries(Vec<VaultEntry>) -> Self` rebuilds `forward`/`reverse`
   and sets each per-kind counter to the **max numeric suffix** seen for that kind
   (so a restored map never re-mints a token that collides with a persisted one).

4. **`Error::Privacy(String)`** — add one variant to `error::Error`
   (`#[error("privacy: {0}")]`) for crypto/vault failures a caller should
   distinguish from generic `Internal`.

5. **`Vault`** — new `executor/src/privacy/vault.rs`, `pub mod vault;` in
   `privacy/mod.rs`. Holds the vault `dir`, an `XChaCha20Poly1305` cipher, and a
   `TokenMap`.
   - `Vault::open(dir: &Path) -> Result<Self>`: `create_dir_all(dir)`; write a
     `.gitignore` containing `*` into `dir` (the vault must never be committed);
     load-or-create the 32-byte key at `dir/key` (create via `OsRng`, write with
     `0600` perms on unix); build the cipher; load `dir/vault.enc` into the map if
     present (else an empty map).
   - `map(&self) -> &TokenMap` and `map_mut(&mut self) -> &mut TokenMap`.
   - `save(&self) -> Result<()>`: serialize `map.entries()` to JSON, encrypt with
     a fresh random 24-byte XNonce, write `[nonce ‖ ciphertext]` to `dir/vault.enc`
     atomically (temp file + rename). Decrypt/parse failures map to
     `Error::Privacy`.

## Acceptance criteria

- [ ] `cargo build` / `clippy -D warnings` / `fmt --check` / `test` all pass.
- [ ] A `Vault` opened on a `TempDir`, populated via `map_mut().intern(...)`, and
      `save()`d, then **reopened** on the same dir, reconstitutes the same
      token↔original mapping.
- [ ] After reopen, `intern` of a persisted original returns its **same** token,
      and `intern` of a new original of the same kind gets the **next** counter
      (no collision with restored tokens).
- [ ] `dir/vault.enc` bytes do **not** contain a plaintext original that was
      interned (proof it is encrypted).
- [ ] On unix, `dir/key` mode is `0600`.
- [ ] `dir/.gitignore` exists and ignores the vault contents.

## Test plan

In `#[cfg(test)] mod tests` in `vault.rs` (hermetic, `TempDir`):

- `roundtrips_token_map_across_reopen` — save then reopen; a token reconstitutes
  to its original.
- `reopen_preserves_token_stability_and_advances_counter` — persisted original →
  same token; new same-kind original → next number.
- `vault_file_is_encrypted_not_plaintext` — raw `vault.enc` excludes the original.
- `key_file_is_owner_only` — `#[cfg(unix)]`, mode `& 0o777 == 0o600`.
- `gitignore_written_into_vault_dir` — `dir/.gitignore` contains `*`.

Randomness note: the key and nonce come from `OsRng` inside the code under test;
the assertions are round-trip / not-plaintext invariants that hold for **any**
key and nonce, so the tests are outcome-deterministic without a seeded RNG (per
STANDARDS §3.3 — deterministic outcome, no RNG-dependent assertion).

## End-to-end verification

Ships no CLI/binary entrypoint yet (phase-05), but the vault touches the real
filesystem. The `TempDir` tests exercise the real encrypt→write→read→decrypt path
against real files (not a fake), which is the end-to-end surface for this phase.
Quote the `cargo test` summary in the completion Update Log.

## Authorizations

- May add dependency: `chacha20poly1305` (root `Cargo.toml` +
  `executor/Cargo.toml`).
- May add one `error::Error` variant (`Privacy`).
- New file: `executor/src/privacy/vault.rs`.
- No `docs/architecture.md` edit.

## Out of scope

- The Qwen NER engine / any model call — phase-03.
- Resolving `PrivacyConfig.vault_dir`'s default against a repo root — phase-06
  wiring (this phase takes an explicit `dir`).
- Content-hash change tracking — phase-04.
- Key rotation, multi-vault, or a passphrase-derived key — not required; a local
  random key file is the contract.

## Update Log

(Filled in by the executor. See WORKFLOW.md § "Update Log entries".)

<!-- entries appended below this line -->

### Update — 2026-08-07 15:06 (complete)

**Summary:** Added `executor/src/privacy/vault.rs` — `Vault::open(dir)` /
`map`/`map_mut` / `save()`. The dictionary is serialized (`TokenMap::entries()`
→ `Vec<VaultEntry>` JSON), sealed with XChaCha20-Poly1305 under a per-vault key,
and written as `[24-byte XNonce ‖ ciphertext]` to `dir/vault.enc` (temp-file +
rename for atomicity). `open` creates the dir, writes a `*` `.gitignore` so the
vault can never be committed, and loads-or-creates the 32-byte key at `dir/key`
(`OsRng`, `0600` on unix). `TokenMap` gained `entries()` / `from_entries()`
(restoring each per-kind counter to the max token suffix so a reloaded map never
re-mints a colliding token), `PiiKind` gained `Serialize`/`Deserialize`, and
`error::Error` gained a `Privacy(String)` variant for crypto/vault failures.
Added `chacha20poly1305 = "0.10"` (workspace + executor). No deviations from the
spec.

**Acceptance criteria:** all ticked above.

**Commands:**

```
$ cargo fmt --all --check        # clean (no output)
$ cargo build                    # Finished, zero warnings
$ cargo clippy --all-targets --all-features -- -D warnings   # Finished, clean
$ cargo test 2>&1 | grep "^test result"
test result: ok. 685 passed; 0 failed; 0 ignored; ...    (mcp)
test result: ok. 2 passed; 0 failed; 0 ignored; ...      (readme_config_reference)
test result: ok. 1098 passed; 0 failed; 2 ignored; ...   (executor lib)
```

Post-phase-01 baseline was 1780; now 1785 (+5 vault tests: roundtrip across
reopen, counter continuity, encrypted-not-plaintext, `0600` key perms, and the
vault `.gitignore`). Zero failures.

**End-to-end verification:** No CLI/binary yet (phase-05), but the vault touches
the real filesystem. The `TempDir` tests exercise the real
encrypt→write→read→decrypt path against actual files: `roundtrips_token_map_across_reopen`
opens a vault, interns "Alice", saves, reopens on the same dir, and reconstitutes
the token back to "Alice"; `vault_file_is_encrypted_not_plaintext` confirms the
raw `vault.enc` bytes do not contain "Alice".
