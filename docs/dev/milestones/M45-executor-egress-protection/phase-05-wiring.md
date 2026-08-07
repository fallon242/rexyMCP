# Phase 5: Wiring (DEFERRED — focused follow-up)

**Milestone:** M45 — Executor Egress Protection
**Status:** review (05a plumbing + 05b dispatch integration done; live pre-scan verified)
**Depends on:** phase-01, phase-02, phase-03, phase-04 (all built + committed)
**Estimated diff:** large — core dispatch path + ~23 `LoopDeps` sites + file-walk

## Why deferred

Phases 01–04 built and tested every component: `should_redact_egress` +
`pii_write_refusal` (egress), `build_pii_index`/`PiiIndex` (prescan),
`RedactingAiClient` (redact). Phase-05 only *wires* them into the dispatch path —
but that touches the load-bearing agent loop and ~23 `LoopDeps` construction sites
(21 in tests), and adds repo file-walking. It was checkpointed to land as its own
PR with full attention rather than be rushed. The four components are green on
branch `m45-executor-egress-protection`.

## Wiring plan (the follow-up)

1. **Engagement, at dispatch** (where the executor client + loop are assembled —
   `executor/src/phase/` / the `run_phase` path that calls `ai::make_client`):
   if `egress::should_redact_egress(&cfg.privacy, &cfg.executor.base_url)`:
   a. **File-walk** the repo root into `(PathBuf, String)` pairs — reuse the
      `ignore` crate (already a dep) honoring `.gitignore`, scoped to the target
      root (`security::scope`); skip binary/oversized files.
   b. Build the engine (`NerEngine::from_config(&cfg.privacy)`) and a **separate**
      pre-scan `Registry` (its own manifest under the vault dir, so it does not
      collide with the M44 ingestion registry), then `build_pii_index(files, &ner,
      &mut registry, &prior)`.
   c. Wrap the client: `RedactingAiClient::new(client, index.redaction_terms())`.
   d. Resolve `index.files()` to absolute paths → the `pii_files` set.
2. **`LoopDeps`** — add `pub pii_files: HashSet<PathBuf>` (owned; default
   `HashSet::new()` at each of the ~23 construction sites, incl. tests). An empty
   set = protection off, so all existing tests keep passing unchanged.
3. **Refusal chain** (`agent/mod.rs:1064`) — add
   `.or_else(|| privacy::egress::pii_write_refusal(edit_path.as_deref(),
   &deps.pii_files))` (the `edit_path` is already resolved on the line above).
4. **Config template** — document `[privacy] redact_executor_egress` in
   `mcp/src/init.rs`.
5. **Docs** — update `docs/privacy.md` egress ② section from "not automated" to
   "redact-on-read + write-guard for cloud executors".
6. **Dogfood** — a live run: point the executor at DeepSeek, a fixture repo with a
   PII data file; confirm (a) the outbound prompts carry `[REDACTED:…]` (session
   log), and (b) a write to the PII file is refused.

## Caveats to carry (unchanged from the README)

- Best-effort: names the pre-scan NER misses still reach the cloud executor.
- File-walk cost: the pre-scan runs NER once per new/changed file at dispatch;
  the registry keeps it incremental. Very large repos should scope which paths are
  scanned (a `[privacy] scan_globs`-style knob is a candidate refinement).

## Update Log

<!-- entries appended below this line -->

### Update — 2026-08-07 17:31 (complete)

**Summary:** Wired the four components into the dispatch path, in two safe steps.
**05a (dormant plumbing):** added `pii_files: HashSet<PathBuf>` to `LoopDeps` and
`crate::privacy::egress::pii_write_refusal` to the pre-dispatch refusal chain;
every construction site (1 prod + 21 tests) passes an empty set, so all 1827 tests
passed unchanged. **05b (engagement):** added `privacy::egress::scan_repo_files`
(gitignore-honoring walk) + `build_egress_index` (scan → NER pre-scan → terms +
PII-file set); in `runner::run_phase`, on a real dispatch with the gate on and a
cloud endpoint, it builds the index, wraps the client in `RedactingAiClient`, and
threads the PII-file set through `Seams` → `LoopDeps`. A failed pre-scan degrades
to deterministic-only live redaction with a `PhaseResult` warning. All existing
tests bypass the new path (they inject a `test_client`), so behavior is unchanged
for them.

**Deviations:** (1) `run_phase` uses a local `ExecClient` enum to own the wrapped
vs. plain client (avoids a conditional-move borrow error); (2) index persistence
across dispatches is **not** done — each dispatch full-scans (registry marks
hashes but the prior index is not persisted); a `scan_globs` knob + index
persistence are the obvious follow-ups for large repos.

**Acceptance criteria:** met (see below); the full live DeepSeek dispatch is the
one manual check left (heavy / token cost).

**Commands:**

```
$ cargo fmt --all --check      # clean
$ cargo clippy --all-targets --all-features -- -D warnings   # Finished, clean
$ cargo test 2>&1 | grep "^test result"
test result: ok. 691 passed; ...    (mcp)
test result: ok. 2 passed; ...      (readme_config_reference)
test result: ok. 1135 passed; 0 failed; 3 ignored; ...  (executor lib)
```

Baseline (M45 branch) 1827 → 1828 (+1 `scan_reads_text_files_and_skips_ignored`;
plus the `#[ignore]` live pre-scan test).

**End-to-end verification:** Ran the live pre-scan against the real Qwen engine:

```
$ cargo test -p rexymcp-executor privacy::egress::tests::live_build_egress_index -- --ignored
running 1 test
test privacy::egress::tests::live_build_egress_index_finds_pii ... ok
```

`build_egress_index` scanned a fixture repo, Qwen's NER found "John Smith", the
deterministic detector found "jane@acme.com", both landed in the redaction terms,
and `data.json` was flagged PII-bearing. The remaining manual check is a full
DeepSeek dispatch confirming (a) `[REDACTED:…]` in the session log and (b) a
refused write to the PII file.

### Update — 2026-08-07 17:49 (dogfood — live DeepSeek dispatch)

Ran the full live dispatch against a fixture repo (DeepSeek executor, Qwen privacy
engine, a `data/users.json` with a name + email + phone), `max_turns = 6`. All
three properties were observed live:

- **Write-guard fires.** The model's `write_file`/`patch` on the PII file was
  refused — *"refusing to edit …/data/users.json: it contains PII, and the
  executor is a cloud model that only sees its redacted contents."* Nothing was
  written (`files_changed: []`).
- **Deterministic redaction reaches DeepSeek.** A read-only follow-up phase had
  the model quote the email/phone; its completion returned `email:
  [REDACTED:email]`, `phone: [REDACTED:phone]` — direct proof the wrapper redacts
  structured PII on the wire.
- **NER (names) is best-effort — a leak observed.** The model reported the owner
  as the real `"John Smith"`. Cause verified directly: Qwen's NER returned `[]`
  for that exact file content (it *had* found the name in a near-identical earlier
  run — pure model variance), so the name never entered the redaction dictionary.
  This is the documented limitation caught in the act, not a defect. NOTE: the
  session log stores **pre-redaction** content (the loop logs tool results before
  the client-boundary redaction), so real PII in the log is expected and is not
  what DeepSeek received — verify egress from the *model's own output*, as above.

**Net:** the mechanism is correct — structured PII is reliably redacted and the
write-guard reliably fires; unstructured PII (names/addresses) depends on the NER
catching it and is **not guaranteed**, matching `docs/privacy.md`'s stated limits.
