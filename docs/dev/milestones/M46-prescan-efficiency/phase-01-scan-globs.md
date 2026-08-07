# Phase 1: `[privacy] scan_globs`

**Milestone:** M46 — Pre-scan Efficiency
**Status:** review
**Depends on:** M45 (`scan_repo_files` / `build_egress_index`)
**Estimated diff:** ~60 lines
**Tags:** language=rust, kind=feature, size=s

## Goal

Let a repo bound the executor-egress pre-scan to a few paths, so a large tree
isn't NER-scanned wholesale. `[privacy] scan_globs` — repo-relative glob patterns;
empty = scan everything (the current behavior).

## Spec

1. `PrivacyConfig.scan_globs: Vec<String>` (default empty) — `config.rs`.
2. `scan_repo_files(root, globs)` builds a `globset::GlobSet` (skip when empty) and
   keeps only files whose repo-relative path matches; `build_egress_index` passes
   `&privacy.scan_globs`. Invalid patterns are skipped.
3. `rexymcp init` documents the key.

## Acceptance criteria

- [ ] Build / clippy `-D warnings` / fmt / test pass.
- [ ] With `scan_globs = ["data/**"]`, a `data/` file is scanned and a top-level
      `main.rs` is not.
- [ ] Empty `scan_globs` scans everything (unchanged behavior).

## Test plan

`egress.rs`: `scan_globs_limit_the_walk`; the existing
`scan_reads_text_files_and_skips_ignored` covers the empty case.

## Authorizations

No new dependencies (`globset` already a dep). No `docs/architecture.md` edit.

## Out of scope

- Encrypted index persistence — phase-02.

## Update Log

<!-- entries appended below this line -->

### Update — 2026-08-07 17:55 (complete)

**Summary:** Added `PrivacyConfig.scan_globs`; `scan_repo_files` now takes the
globs and filters by repo-relative path via a `globset::GlobSet` (built only when
non-empty; invalid patterns skipped). `build_egress_index` passes the config
value; `rexymcp init` documents it. Converted the `ner.rs` live-test
`PrivacyConfig` literal to `..Default::default()` so future fields don't break it.

**Commands:** `cargo fmt --all --check` clean; `clippy -D warnings` clean;
`cargo test` → 691 (mcp) + 2 (readme) + 1136 (executor, +scan_globs) = 1829, 0
failed.

**End-to-end verification:** Not applicable — library filter; the
`scan_globs_limit_the_walk` test drives a real `TempDir` walk with a `data/**`
pattern and asserts a top-level file is excluded.