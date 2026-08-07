# Phase 4: Ingestion registry — content-hash change tracking

**Milestone:** M44 — PII Ingestion Gate
**Status:** review
**Depends on:** phase-01, phase-02, phase-03
**Estimated diff:** ~230 lines (module + tests)
**Tags:** language=rust, kind=feature, size=m

## Goal

Make ingestion incremental: process a source document or data element through the
gateway **only when it is new or its content changed**, so the expensive NER pass
never re-runs on unchanged material. Because the vault map is shared, a re-scrub
of a changed doc reuses existing tokens — pseudonyms stay stable across edits.
This is the "done on any new or changed source document or data" requirement.

## Architecture references

Read before starting:

- `docs/dev/milestones/M44-pii-ingestion-gate/README.md` — the ingestion-registry
  role.
- `executor/src/privacy/gateway.rs` — `Gateway::anonymize` (the scrub this gates).
- `docs/dev/STANDARDS.md` §2.6 (deps), §3 (tests).

## Pre-flight

1. Read `docs/dev/STANDARDS.md` top to bottom.
2. Read the phase-01/02/03 privacy code.
3. Read this entire phase doc.
4. Clean branch (`m44-pii-ingestion-gate`).

## Current state

- `privacy/` has deterministic + NER detection, the reversible `TokenMap`, the
  encrypted `Vault`, and `Gateway::anonymize`. Nothing tracks *what has already
  been ingested*, so every call re-runs full detection (including the model).
- No hash dependency is in the workspace.

## Spec

1. **Add the dependency** — `sha2 = "0.10"` to `[workspace.dependencies]` (root
   `Cargo.toml`) and `sha2.workspace = true` to `executor/Cargo.toml`.

2. **`Registry`** — new `executor/src/privacy/registry.rs`. A persisted manifest
   of `source key → SHA-256 hex of its last-ingested content`.
   - `Registry::load(path: &Path) -> Result<Self>` (missing file → empty manifest;
     parse failure → `Error::Privacy`).
   - `is_changed(&self, key: &str, content: &str) -> bool` — true when `key` is
     unknown or its stored hash differs from `hash(content)`.
   - `mark(&mut self, key: &str, content: &str)` — record the current hash.
   - `save(&self) -> Result<()>` — atomic write (temp + rename) of the manifest as
     pretty JSON.
   - The manifest holds only opaque SHA-256 hashes (no PII, not reversible), so it
     is plaintext; it still lives in the git-ignored vault dir. The reversible PII
     lives only in the encrypted vault.
   - `fn hash(content: &str) -> String` — lowercase SHA-256 hex.

3. **`Ingestor` + `Ingested`** — in the same module. `Ingestor { gateway,
   registry }`; `Ingested::{ Scrubbed(String), Unchanged }`.
   - `async fn ingest(&mut self, key: &str, content: &str, map: &mut TokenMap) ->
     Result<Ingested>`: if `!registry.is_changed(key, content)` return
     `Unchanged` **without calling the model**; otherwise
     `gateway.anonymize(content, map)`, `registry.mark(key, content)`, return
     `Scrubbed(anonymized)`.
   - `registry(&self) -> &Registry` / `registry_mut(&mut self) -> &mut Registry`.

4. **Module declaration** — `pub mod registry;` in `privacy/mod.rs`.

## Acceptance criteria

- [ ] `cargo build` / `clippy -D warnings` / `fmt --check` / `test` all pass.
- [ ] `is_changed` is true for an unknown key, false after `mark`, and true again
      after the content changes.
- [ ] A saved manifest reloads and reports unchanged content as unchanged.
- [ ] `Ingestor::ingest` on the **same** content twice calls the model **once**
      (second call returns `Unchanged`); changed content re-scrubs (model called
      again).
- [ ] A re-scrub of changed content reuses the same token for PII already in the
      map (pseudonym stability).

## Test plan

Hermetic (`TempDir`, `MockAiClient`):

- `registry`: `is_changed_true_for_unknown_key`, `is_changed_false_after_mark`,
  `is_changed_true_after_content_edit`, `manifest_persists_across_reload`.
- `ingestor` (`#[tokio::test]`): `unchanged_content_is_not_rescrubbed` (assert
  `mock.calls().len() == 1` via a cloned handle), `changed_content_is_rescrubbed`
  (model called twice), `rescrub_reuses_stable_token` (same person → same token
  after an edit).

## End-to-end verification

Ships no CLI/binary yet (phase-05); the manifest touches the real filesystem, so
`manifest_persists_across_reload` exercises the real write→read→parse path against
a real file. Quote the `cargo test` summary in the completion Update Log.

## Authorizations

- May add dependency: `sha2` (root `Cargo.toml` + `executor/Cargo.toml`).
- New file: `executor/src/privacy/registry.rs`.
- No `docs/architecture.md` edit.

## Out of scope

- Walking a directory tree / discovering source files — that's the CLI's job
  (phase-05); this phase takes an explicit `(key, content)`.
- Wiring `Ingestor` to a `Vault` for map persistence — phase-06.
- Deleting manifest entries for removed sources / GC — not required now.

## Update Log

(Filled in by the executor. See WORKFLOW.md § "Update Log entries".)

<!-- entries appended below this line -->

### Update — 2026-08-07 15:23 (complete)

**Summary:** Added `executor/src/privacy/registry.rs`. `Registry` persists a
`source key → SHA-256 hex` manifest (`load`/`is_changed`/`mark`/`save`, atomic
temp+rename); the manifest holds only opaque hashes (no PII), so it is plaintext
though still git-ignored, while the reversible PII stays in the encrypted vault.
`Ingestor` composes a `Gateway` + `Registry`: `ingest(key, content, &mut map)`
returns `Unchanged` without a model call when the hash matches, else
`Scrubbed(anonymized)` and records the new hash. Because the `TokenMap` is shared,
a re-scrub of edited content reuses existing tokens (pseudonym stability). Added
`sha2 = "0.10"` (workspace + executor). No deviations from the spec.

**Acceptance criteria:** all ticked above.

**Commands:**

```
$ cargo fmt --all --check      # clean
$ cargo build                  # Finished, zero warnings
$ cargo clippy --all-targets --all-features -- -D warnings   # Finished, clean
$ cargo test 2>&1 | grep "^test result"
test result: ok. 685 passed; 0 failed; 0 ignored; ...     (mcp)
test result: ok. 2 passed; 0 failed; 0 ignored; ...       (readme_config_reference)
test result: ok. 1113 passed; 0 failed; 3 ignored; ...    (executor lib)
```

Post-phase-03 baseline was 1793; now 1800 (+7: 4 `registry`, 3 `ingestor`).
`unchanged_content_is_not_rescrubbed` proves the model is called exactly once
across two ingests of identical content; `rescrub_reuses_stable_token` proves an
edit re-scrubs but keeps `Person_1`.

**End-to-end verification:** No CLI/binary yet (phase-05). `manifest_persists_across_reload`
exercises the real save→reload→parse path against an actual file in a `TempDir`
(mark "content", `save`, reopen, assert unchanged content reads unchanged and
edited content reads changed).
