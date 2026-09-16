# Phase 5: CLI — anonymize / reconstitute / vault

**Milestone:** M44 — PII Ingestion Gate
**Status:** review
**Depends on:** phase-01, phase-02, phase-03, phase-04
**Estimated diff:** ~180 lines (module + wiring + tests)
**Tags:** language=rust, kind=feature, size=m

## Goal

Make the gate runnable by hand: `rexymcp anonymize` scrubs PII from a file or
stdin (local Qwen + deterministic detectors) into the reversible encrypted vault
and prints the tokenized text; `rexymcp reconstitute` reverses it; `rexymcp vault`
reports counts without ever printing an original.

## Architecture references

Read before starting:

- `mcp/src/main.rs` — the clap `Commands` enum and its `match` dispatch; `Health`
  (config load) and `RunPhase` (async handler) are the patterns to mirror.
- `executor/src/privacy/{gateway,ner,vault}.rs` — the components this wraps.
- `docs/dev/STANDARDS.md` §3.2 (plumbing needs no test) / §3.1 (pure fns do).

## Pre-flight

1. Read `docs/dev/STANDARDS.md` top to bottom.
2. Read the `mcp` CLI dispatch and the phase-01..04 privacy API.
3. Read this entire phase doc.
4. Clean branch (`m44-pii-ingestion-gate`).

## Current state

- The `privacy` module exposes `Gateway::anonymize`, `NerEngine::from_config`,
  `Vault::{open,map,map_mut,save}`, `TokenMap::reconstitute`, and
  `VaultEntry.kind.token_prefix()`. Nothing invokes them from the binary.
- `mcp` refers to the executor as `rexymcp_executor::…`; `main` is `#[tokio::main]`
  so handlers may `await`. Config loads via `Config::load_with_env(&path)`.

## Spec

1. **Handler module** — new `mcp/src/privacy_cli.rs`.
   - `resolve_vault_dir(cfg: &PrivacyConfig, repo: &Path, override_dir:
     Option<&Path>) -> PathBuf`: `override_dir` > `cfg.vault_dir` >
     `repo/.rexymcp/vault`.
   - `read_input(input: Option<&str>) -> Result<String>`: `None`/`"-"` → stdin,
     else read the file.
   - `async fn anonymize(args)`: load config, read input, build
     `Gateway::new(NerEngine::from_config(&cfg.privacy)?)`, `Vault::open(dir)`,
     `gateway.anonymize(&text, vault.map_mut()).await`, `vault.save()`, `print!`
     the result (no added newline).
   - `fn reconstitute(args)`: load config, read input, `Vault::open`, `print!` the
     `vault.map().reconstitute(&text)`.
   - `fn vault_status(config, repo, vault)`: open the vault, print the dir, entry
     count, and per-kind counts (keyed by `token_prefix()`) — **never** an
     original.

2. **Commands** — add `Anonymize`, `Reconstitute`, `Vault` to `mcp/src/main.rs`'s
   `Commands` enum, each with `--config` (default `rexymcp.toml`), `--repo`
   (default `.`), `--vault` (optional), and (for the first two) a positional
   `input`. Add `mod privacy_cli;` and the three `match` arms.

## Acceptance criteria

- [ ] `cargo build` / `clippy -D warnings` / `fmt --check` / `test` all pass.
- [ ] `resolve_vault_dir` honors override > config > `repo/.rexymcp/vault`.
- [ ] End to end against the live Qwen engine: piping a sentence with a name and
      an email through `anonymize` prints tokens; piping that through
      `reconstitute` restores the original; `vault` reports the counts.

## Test plan

- Unit (`privacy_cli.rs`, hermetic): `vault_dir_prefers_explicit_override`,
  `vault_dir_falls_back_to_config`, `vault_dir_defaults_under_repo`. The command
  handlers are thin plumbing over already-tested components (STANDARDS §3.2), so
  their coverage is the end-to-end run below.

## End-to-end verification

Run the real binary against a scratch config pointed at the live Qwen engine
(`http://192.168.1.10:8080/v1`, `local-ner-model`) and a scratch vault dir; quote the
transcript in the completion Update Log — anonymize → reconstitute round-trips the
original, and `vault` shows the counts.

## Authorizations

- No new dependencies.
- New file: `mcp/src/privacy_cli.rs`.
- No `docs/architecture.md` edit; no `rexymcp.toml` edit (E2E uses a scratch
  config — configuring the live `[privacy]` engine is phase-07's init work).

## Out of scope

- A `--reveal` vault dump (reconstituting a token already reveals its original);
  batch/recursive directory scrubbing (uses the phase-04 registry — a later
  wiring); executor/`PhaseResult`/prompt-hook enforcement (phase-06/07).

## Update Log

(Filled in by the executor. See WORKFLOW.md § "Update Log entries".)

<!-- entries appended below this line -->

### Update — 2026-08-07 15:38 (complete)

**Summary:** Added `mcp/src/privacy_cli.rs` and three clap commands
(`anonymize`/`reconstitute`/`vault`) with `--config`/`--repo`/`--vault` +
positional `input`. `anonymize` builds `NerEngine::from_config` → `Gateway`, opens
the vault, scrubs, saves, and prints; `reconstitute` opens the vault and prints
`map().reconstitute`; `vault` prints per-kind counts via `token_prefix()` and
never an original. `resolve_vault_dir` (override > config > `repo/.rexymcp/vault`)
is unit-tested; the handlers are thin plumbing over already-tested components
(STANDARDS §3.2). No new dependencies, no deviations from the spec.

**Acceptance criteria:** all ticked above.

**Commands:**

```
$ cargo fmt --all --check      # clean
$ cargo build                  # Finished, zero warnings
$ cargo clippy --all-targets --all-features -- -D warnings   # Finished, clean
$ cargo test 2>&1 | grep "^test result"
test result: ok. 688 passed; 0 failed; 0 ignored; ...     (mcp: +3 privacy_cli)
test result: ok. 2 passed; 0 failed; 0 ignored; ...       (readme_config_reference)
test result: ok. 1113 passed; 0 failed; 3 ignored; ...    (executor lib)
```

Post-phase-04 baseline was 1800; now 1803 (+3 `privacy_cli` unit tests).

**End-to-end verification:** Ran the real binary against a scratch config pointed
at the live Qwen engine and a scratch vault dir:

```
input:  John Smith emailed jane@acme.com about renting 42 Baker Street to Maria Gonzalez

$ printf '%s' "$input" | rexymcp anonymize --config e2e.toml --vault ./m44-vault
Person_2 emailed Email_1 about renting 42 Baker Street to Person_1

$ rexymcp reconstitute --config e2e.toml --vault ./m44-vault < anon.txt
John Smith emailed jane@acme.com about renting 42 Baker Street to Maria Gonzalez

$ rexymcp vault --config e2e.toml --vault ./m44-vault
vault:   .../m44-vault
entries: 3
  Email: 1
  Person: 2
```

Round-trip is exact; the vault dir held `.gitignore` (`*`), `key` (mode `0600`),
and `vault.enc` (encrypted). **Honest note:** in this run Qwen's NER did **not**
flag "42 Baker Street" (it returned only the two names), so the address was not
tokenized — the documented best-effort limitation (an earlier spike on the same
sentence *did* catch it; llama.cpp at temp 0 is not perfectly deterministic).
Deterministic PII (the email) is caught reliably; unstructured PII via the model
is not guaranteed. This is a property of LLM NER, not a code defect; it is the
reason the design biases toward over-matching and pairs the model with
deterministic detectors — but it is not a leak-proof guarantee, and phase-06/07
must not present it as one.
