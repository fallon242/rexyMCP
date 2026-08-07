# Phase 7: init defaults, UserPromptSubmit hook, docs

**Milestone:** M44 — PII Ingestion Gate
**Status:** review
**Depends on:** phase-01..06a
**Estimated diff:** ~120 lines (template + hook + docs + test)
**Tags:** language=rust, kind=feature, size=s

## Goal

Ship the last user-facing pieces: `rexymcp init` documents the `[privacy]`
section, an opt-in `UserPromptSubmit` hook guards against pasting structured PII
into Claude, and `docs/privacy.md` explains the whole gate honestly.

## Architecture references

- `mcp/src/init.rs` — the `rexymcp.toml` template + its key-coverage tests.
- Claude Code hook contract (confirmed): `UserPromptSubmit` receives `user_input`
  on stdin and **cannot rewrite** the prompt — only allow / add-context / block.
- `plugin/` — the Claude Code plugin package (skills, templates, `.mcp.json`).

## Pre-flight

1. Read `docs/dev/STANDARDS.md`.
2. Read the phase-01..06a privacy code and `docs/privacy.md` intent.
3. Clean branch (`m44-pii-ingestion-gate`).

## Spec

1. **`[privacy]` in the init template** — `mcp/src/init.rs`: append a `[privacy]`
   block (`enabled = false`; commented `engine_base_url` / `engine_model` /
   `vault_dir`) with a best-effort caveat comment. Add a test asserting the
   generated config documents `[privacy]` and loads with the gate disabled.

2. **UserPromptSubmit hook** — `plugin/hooks/pii-guard.sh`. Because the hook
   contract cannot rewrite a prompt, the only leak-preventing action is to
   **block**. The script extracts `.user_input` (via `jq`), matches fast local
   regex for structured PII (email / SSN / phone / card — no model, no network, so
   it never fails open), and on a match exits `2` with guidance to run
   `rexymcp anonymize`. Opt-in (documented settings.json registration); not
   auto-registered, since a blocking hook should be a deliberate choice.

3. **`docs/privacy.md`** — the honest feature doc: threat model, components, CLI,
   the automatic boundary scrub, the hook + its hard constraint, config, and the
   limitations (best-effort NER, vault-as-honeypot, deferred executor egress).

## Acceptance criteria

- [ ] `cargo build` / `clippy -D warnings` / `fmt --check` / `test` all pass.
- [ ] A freshly `init`-ed config documents `[privacy]` and loads with
      `privacy.enabled == false`.
- [ ] `pii-guard.sh` blocks (exit 2) a prompt containing an email / SSN / phone /
      card and allows (exit 0) a clean one.
- [ ] `docs/privacy.md` states plainly that detection is best-effort and that the
      hook cannot rewrite prompts.

## Test plan

- `init.rs`: `template_documents_privacy_section`.
- The hook is a shell script (not cargo-testable); verified by direct invocation
  (quoted in the Update Log).

## End-to-end verification

Run `pii-guard.sh` against PII and clean JSON payloads; quote the exit codes.

## Authorizations

- No new dependencies. New files: `plugin/hooks/pii-guard.sh`, `docs/privacy.md`.
  Edit: `mcp/src/init.rs`. No `docs/architecture.md` edit (M44's Status entry is
  added at milestone close).

## Out of scope

- Auto-registering the hook (opt-in only).
- Any prompt *transformation* — impossible via the hook contract; the CLI is the
  transform path.
- Executor egress enforcement — phase-06b (deferred).

## Update Log

(Filled in by the executor. See WORKFLOW.md § "Update Log entries".)

<!-- entries appended below this line -->

### Update — 2026-08-07 15:56 (complete)

**Summary:** Added a `[privacy]` block to the `rexymcp init` template
(`enabled = false`, commented engine/vault keys, best-effort caveat) with
`template_documents_privacy_section`. Added `plugin/hooks/pii-guard.sh` — an
opt-in `UserPromptSubmit` guard that blocks (exit 2) a prompt containing
structured PII (email/SSN/phone/card via local regex, engine-free so it never
fails open) with guidance to `rexymcp anonymize`; it cannot rewrite the prompt
because the hook contract forbids it (confirmed), so blocking is the honest
mechanism. Added `docs/privacy.md` documenting the whole gate, the CLI, the
boundary scrub, the hook's hard constraint, and the limitations. No new deps.

**Acceptance criteria:** all ticked above.

**Commands:**

```
$ cargo fmt --all --check      # clean
$ cargo build                  # Finished, zero warnings
$ cargo clippy --all-targets --all-features -- -D warnings   # Finished, clean
$ cargo test 2>&1 | grep "^test result"
test result: ok. 691 passed; 0 failed; 0 ignored; ...     (mcp: +1 privacy template)
test result: ok. 2 passed; 0 failed; 0 ignored; ...       (readme_config_reference)
test result: ok. 1113 passed; 0 failed; 3 ignored; ...    (executor lib)
```

Post-phase-06a baseline was 1805; now 1806 (+1 init test).

**End-to-end verification:** Direct hook invocation:

```
$ printf '%s' '{"user_input":"my email is jane@acme.com"}' | plugin/hooks/pii-guard.sh; echo $?
rexyMCP PII guard: your prompt appears to contain structured PII ...
2
$ printf '%s' '{"user_input":"refactor the parser module"}' | plugin/hooks/pii-guard.sh; echo $?
0
$ printf '%s' '{"user_input":"ssn 123-45-6789"}' | plugin/hooks/pii-guard.sh; echo $?
2
```

Blocks structured PII, allows clean prompts.