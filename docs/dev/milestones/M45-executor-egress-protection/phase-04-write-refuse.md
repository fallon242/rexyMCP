# Phase 4: Write-refuse guard for PII files

**Milestone:** M45 — Executor Egress Protection
**Status:** review
**Depends on:** phase-02 (`PiiIndex`)
**Estimated diff:** ~70 lines (fn + tests)
**Tags:** language=rust, kind=feature, size=s

## Goal

The structural half of the protection: refuse a `write_file` / `patch` targeting a
PII-bearing file. The cloud model only ever sees that file's **redacted**
contents, so it must not overwrite it (it would write fabricated data — the 06b
failure). Refusing is safe by construction: no dependence on the model preserving
anything. This phase ships the pure refusal function; wiring it into the loop is
phase-05 (with the RedactingAiClient), where the `LoopDeps` change lives.

## Architecture references

- `executor/src/agent/tools.rs` — the sibling pre-dispatch refusals
  (`read_before_edit_refusal`, `destructive_restore_refusal`), all pure
  `Option<String>` guards over `(tool_call, state, project_root)`. `edit_target`
  resolves a write/patch target path.
- `executor/src/agent/mod.rs:1064` — the `.or_else(...)` refusal chain this joins
  (in phase-05).

## Current state

- `tools.rs` has `edit_target` + two refusal guards, chained in `mod.rs`. Nothing
  consults the M45 `PiiIndex`. `tools.rs` imports `HashMap` but not `HashSet`.

## Spec

1. **`pii_write_refusal`** — a `pub fn` in `executor/src/privacy/egress.rs`:
   `pub fn pii_write_refusal(edit_target: Option<&Path>, pii_files:
   &HashSet<PathBuf>) -> Option<String>`. It takes the **already-resolved**
   write/patch target (so it needs no agent internals, and being `pub` it is not
   dead code while the phase-05 wiring waits). `None` for a non-edit call
   (`edit_target == None`) or a target not in `pii_files`; else a model-visible
   refusal naming the file and the manual/local-executor remedy. Phase-05 computes
   the target via the agent's `edit_target` and passes it in.

## Acceptance criteria

- [ ] `cargo build` / `clippy -D warnings` / `fmt --check` / `test` all pass.
- [ ] `write_file` **and** `patch` to a file in `pii_files` are refused.
- [ ] A clean file, a non-edit call (`read_file`), and an empty `pii_files` set
      all return `None`.

## Test plan

`egress.rs` unit tests: `write_guard_refuses_pii_file`,
`write_guard_allows_clean_file`, `write_guard_allows_non_edit_call` (target
`None`), `write_guard_empty_set_never_refuses`.

## End-to-end verification

Not applicable — a pure `Option<String>` guard; the live refusal in a dispatch is
exercised in phase-05's dogfood.

## Authorizations

- No new dependencies. Edits `executor/src/agent/tools.rs` only.
- No `docs/architecture.md` edit.

## Out of scope

- Wiring into the `mod.rs` refusal chain + the `LoopDeps` `pii_files` field +
  populating it — phase-05 (21 `LoopDeps` construction sites change there, with
  the RedactingAiClient wiring).

## Update Log

(Filled in by the executor. See WORKFLOW.md § "Update Log entries".)

<!-- entries appended below this line -->

### Update — 2026-08-07 17:00 (complete)

**Summary:** Added `pub fn pii_write_refusal(edit_target, pii_files)` to
`executor/src/privacy/egress.rs`. **Deviation from the draft (deliberate):** first
tried it as a `pub(super)` guard in `agent/tools.rs`, but with the wiring deferred
to phase-05 it was dead code (`cargo clippy -D warnings` failed —
`function is never used`). STANDARDS forbids masking that with `#[allow]`, so I
relocated it to `privacy::egress` as a **`pub` fn** (public API → not dead code)
that takes the already-resolved target path, needing no agent internals. Phase-05
computes the target via the agent's `edit_target` and passes it in.

**Acceptance criteria:** all ticked above.

**Commands:**

```
$ cargo fmt --all --check      # clean
$ cargo clippy --all-targets --all-features -- -D warnings   # Finished, clean
$ cargo test 2>&1 | grep "^test result"
test result: ok. 691 passed; 0 failed; 0 ignored; ...     (mcp)
test result: ok. 2 passed; 0 failed; 0 ignored; ...       (readme_config_reference)
test result: ok. 1134 passed; 0 failed; 3 ignored; ...    (executor lib: +4 write-guard)
```

Post-phase-03 baseline was 1823; now 1827 (+4 write-guard tests).

**End-to-end verification:** Not applicable — a pure `Option<String>` guard. The
four tests cover: refuse a PII file, allow a clean file, allow a non-edit call
(`None` target), and never refuse on an empty set. Live refusal in a dispatch is
phase-05's dogfood.
