# Phase 5: Wiring (DEFERRED — focused follow-up)

**Milestone:** M45 — Executor Egress Protection
**Status:** todo (deferred — the invasive integration; do as its own focused PR)
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

(None — not implemented. Deferred wiring spec.)

<!-- entries appended below this line -->
