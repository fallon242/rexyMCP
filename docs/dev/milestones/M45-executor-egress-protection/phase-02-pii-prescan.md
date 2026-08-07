# Phase 2: Repo pre-scan → PII index

**Milestone:** M45 — Executor Egress Protection
**Status:** review
**Depends on:** phase-01; M44 detector / ner / registry
**Estimated diff:** ~200 lines (module + tests)
**Tags:** language=rust, kind=feature, size=m

## Goal

Scan the repo's files once into a `PiiIndex` — the two things later phases need:
the **PII term dictionary** (every distinct PII string + kind, for outbound
redaction) and the **PII-file set** (which files contain PII, for the write-refuse
guard). Incremental via the M44 registry: a file whose content is unchanged reuses
its cached result, so NER runs only on new/changed files.

## Architecture references

- `docs/dev/milestones/M45-executor-egress-protection/README.md` — the design.
- `executor/src/privacy/{detector,ner,registry}.rs` — reused wholesale.
- `docs/dev/STANDARDS.md` §3 (tests; `MockAiClient` for NER).

## Current state

- `detector::detect_deterministic`, `ner::NerEngine::detect`, and
  `registry::Registry::{is_changed,mark}` all exist (M44). Nothing aggregates them
  into a repo-wide PII index.

## Spec

1. **`executor/src/privacy/prescan.rs`** (new; `pub mod prescan;` in
   `privacy/mod.rs`):
   - `PiiIndex` holding `per_file: BTreeMap<PathBuf, Vec<(String, PiiKind)>>`.
     Methods: `empty()`, `contains_file(&Path) -> bool`,
     `files() -> impl Iterator<Item=&PathBuf>` (PII files only),
     `redaction_terms() -> Vec<(String, PiiKind)>` (distinct, **longest-first** so
     overlapping matches prefer the longer term), `is_empty()`.
   - `async fn build_pii_index(files: &[(PathBuf, String)], ner: &NerEngine,
     registry: &mut Registry, prior: &PiiIndex) -> Result<PiiIndex>`: for each
     `(path, content)`, if `!registry.is_changed(path, content)` and `prior` has
     the file, reuse its cached entry (no model call); else run
     `detect_deterministic` + `ner.detect`, `registry.mark`, store. Aggregation is
     per-file so the reuse is per-file.

## Acceptance criteria

- [ ] `cargo build` / `clippy -D warnings` / `fmt --check` / `test` all pass.
- [ ] First pass runs NER on every non-empty file; a data file with PII is in
      `files()` and a clean code file is not.
- [ ] `redaction_terms` contains both a deterministic hit (email) and an NER hit
      (name) from the scanned content.
- [ ] A second pass over **unchanged** content reuses the cache — NER is **not**
      re-run (mock call count unchanged).
- [ ] A file whose content changed **is** re-scanned.

## Test plan

`prescan.rs` `#[tokio::test]` with `MockAiClient` (cloned handle for `calls()`) and
a `TempDir` registry: `scans_all_files_first_pass_and_aggregates`,
`unchanged_file_reuses_cache_without_ner`, `changed_file_is_rescanned`.

## End-to-end verification

Not applicable — library aggregation over injected `(path, content)` pairs and a
`MockAiClient`; no runtime-loadable artifact. The real file-walking that produces
the `(path, content)` list is phase-05 wiring.

## Authorizations

- No new dependencies. New file `executor/src/privacy/prescan.rs`.
- No `docs/architecture.md` edit.

## Out of scope

- Walking the repo tree / choosing which files to scan — phase-05 wiring.
- Persisting the index across dispatches (encrypted) — phase-05 decides storage;
  this phase keeps the index in memory and takes `prior` as a parameter.
- The redaction itself and the write-refuse — phases 03/04.

## Update Log

(Filled in by the executor. See WORKFLOW.md § "Update Log entries".)

<!-- entries appended below this line -->

### Update — 2026-08-07 16:44 (complete)

**Summary:** Added `executor/src/privacy/prescan.rs`. `PiiIndex` stores PII
per-file (`BTreeMap<PathBuf, Vec<(String, PiiKind)>>`) and exposes `contains_file`
(write-refuse), `files`, `redaction_terms` (distinct, longest-first), and
`is_empty`. `build_pii_index` runs `detect_deterministic` + `NerEngine::detect`
per file, but reuses a file's `prior` entry when `Registry::is_changed` reports it
unchanged — so NER runs only on new/changed files. `pub mod prescan;`. No new
dependencies.

**Acceptance criteria:** all ticked above.

**Commands:**

```
$ cargo fmt --all --check      # clean
$ cargo clippy --all-targets --all-features -- -D warnings   # Finished, clean
$ cargo test 2>&1 | grep "^test result"
test result: ok. 691 passed; 0 failed; 0 ignored; ...     (mcp)
test result: ok. 2 passed; 0 failed; 0 ignored; ...       (readme_config_reference)
test result: ok. 1124 passed; 0 failed; 3 ignored; ...    (executor lib: +3 prescan)
```

Post-phase-01 baseline was 1814; now 1817 (+3 prescan tests).

**End-to-end verification:** Not applicable — library aggregation over injected
`(path, content)` pairs + `MockAiClient`. `unchanged_file_reuses_cache_without_ner`
proves the incrementality: a second pass over identical content leaves the mock's
call count at 1; `changed_file_is_rescanned` shows it rises to 2 on edited content.
