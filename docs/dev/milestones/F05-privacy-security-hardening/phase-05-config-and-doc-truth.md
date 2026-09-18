# Phase 5: remove the inert `privacy.kinds`, and make the privacy doc true

**Milestone:** F05 — Privacy and security hardening
**Status:** done
**Depends on:** phase 01 (done, `terms_file`), phase 02 (done, session-log
scrub). Both added behavior this phase documents.
**Estimated diff:** ~180 lines, about half of it tests
**Tags:** language=rust, kind=docs, size=s

> **Cloud executor allowed.** This phase touches `executor/src/config.rs` and
> two documentation files. It does **not** touch `executor/src/privacy/**` or
> `executor/src/security/**`, so it does not edit the redaction plumbing a
> cloud model is itself subject to.

## Goal

Two findings, both "the software says something that is not true".

**Finding 5 — `privacy.kinds` is inert.** `PrivacyConfig` declares
`pub kinds: Vec<String>` (`executor/src/config.rs:101`) and documents it as
"narrows which PII classes are masked (empty = all)". Nothing reads it. The
only reference is a test asserting it is empty. An operator who writes
`kinds = ["email"]` believes they scoped detection; they did not.

This phase **removes** it rather than implementing it. A knob that narrows
detection can only reduce protection, and the milestone's stated bias is the
other way ("over-redaction is safe; a miss is a leak"). Nobody has asked for
it.

Removing a key silently is its own lie — `#[serde(default)]` means an existing
`kinds = [...]` would simply be ignored. So a config that still sets it
**fails to load**, with an error saying the key was removed and why.

**Finding 6 — the privacy doc contradicts itself.** `docs/privacy.md:168-171`
still says executor egress "is not automated … a documented residual risk; use
a local executor or pre-scrub with the CLI". That has been false since M45, and
the same file describes the automated path a few lines earlier. A reader acts
on whichever they find first.

The same file's `## Configuration` block is also stale: it lists five keys and
omits `terms_file`, which phase 01 shipped.

## Gotcha: what the first run got wrong (read before editing)

The 2026-09-17 dispatch to `deepseek-flash` ended `hard_fail` (oscillation) at
91 turns with nothing usable. Two concrete mistakes, both avoidable:

1. **It rewrote a fact it was not asked to touch.** The `PrivacyConfig` doc
   comment reads "the local detection model (Qwen on the LAN, detection only)".
   The run changed `Qwen` to `llama.cpp`. That word was **not** redacted — the
   session log shows it reaching the model in the clear — so this was invention,
   not a redaction artifact. **`Qwen` is correct and must survive this phase
   unchanged.** The only clause to remove from that comment is the `kinds` one.
2. **It broke a line mid-word and could not repair it.** After a `patch_lines`
   call, line 101 read `/// executor endpoint is a clo` — truncated from
   "executor endpoint is a cloud host". The run then alternated `patch_lines`
   with `sed -n '101p' … | od -c` for roughly 40 turns until the governor
   stopped it.

   **If an edit leaves a line malformed, do not iterate on the bytes.** Run
   `git checkout -- <file>` to restore that file and redo the edit in one clean
   `write_file` or `patch`. The file is tracked and your work is not committed,
   so nothing else is lost.

Both lines this phase touches live in the same 10-line region, so read
`executor/src/config.rs` lines 88-112 before the first edit and verify the
region after the last one with a single `sed -n '88,112p'`.

## Architecture references

- `executor/src/config.rs:92-112` — the `PrivacyConfig` doc comment and struct.
  `load` is at line 496; it parses the whole file with `toml::from_str` into
  `Config`. `toml` is already a dependency (`executor/Cargo.toml:12`).
- `executor/src/config.rs:688-698` — `privacy_defaults_when_absent`, the test
  that asserts `kinds` is empty. It is the only other reference.
- `docs/privacy.md:146-159` — the `## Configuration` block.
- `docs/privacy.md:161-173` — `## Limitations (do not skip)`.
- `rexymcp.toml.example:69-83` — the `[privacy]` block, which is already
  correct and lists `terms_file`. It never listed `kinds`; leave it alone.

## Pre-flight

1. `git status --short` is clean. If it is not, stop and file a blocker.
2. `cargo test -p rexymcp-executor config` passes. Record the count.
3. `grep -rn 'kinds' executor/src mcp/src` — confirm for yourself that the only
   `privacy.kinds` references are the struct field, its doc comment, the test at
   line 697, and the `kinds: vec![]` literal in
   `executor/src/privacy/egress.rs:573`. Everything else named `kinds` belongs
   to other modules (`PiiKind`, symbols, telemetry) and is unrelated.

## Current state

```rust
// executor/src/config.rs:92-102
/// PII ingestion gate (M44). `enabled` is opt-in (default false) until boundary
/// enforcement lands; `engine_*` point at the local detection model (Qwen on the
/// LAN, detection only); `vault_dir` locates the reversible token store; `kinds`
/// narrows which PII classes are masked (empty = all).
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct PrivacyConfig {
    pub enabled: bool,
    pub engine_base_url: Option<String>,
    pub engine_model: Option<String>,
    pub vault_dir: Option<PathBuf>,
    pub kinds: Vec<String>,
```

```rust
// executor/src/config.rs:496-505 (Config::load, head)
    pub fn load(path: &Path) -> Result<Self> {
        let mut config = Config::default();

        if path.exists() {
            let content = std::fs::read_to_string(path)?;
            let loaded: Config =
                toml::from_str(&content).map_err(|e| Error::Config(e.to_string()))?;
            config = loaded;
        }
```

```markdown
<!-- docs/privacy.md:168-171 -->
- **Executor egress (②) is not automated** — a prototype proved the intended
  round-trip corrupts files (the model replaces tokens with fabricated values), so
  it was abandoned. A cloud executor over a PII-bearing repo is a documented
  residual risk; use a local executor or pre-scrub with the CLI.
```

## Spec

### 1. Remove the field (config.rs)

- Delete `pub kinds: Vec<String>,` from `PrivacyConfig`.
- In the struct's doc comment, delete the clause "`kinds` narrows which PII
  classes are masked (empty = all)" and end the sentence at `vault_dir`'s
  description.
- In `executor/src/privacy/egress.rs`, delete the `kinds: vec![],` line from the
  `PrivacyConfig` literal at line 573. That is the one edit this phase makes
  outside `config.rs` and the two docs; without it the crate will not compile.
  Make no other change to that file.

### 2. A config that still sets it fails to load (config.rs)

In `Config::load`, after `let content = std::fs::read_to_string(path)?;` and
**before** `toml::from_str::<Config>`, reject the removed key:

```rust
            if removed_privacy_kinds(&content) {
                return Err(Error::Config(format!(
                    "{}: `[privacy] kinds` was removed in F05 — it never narrowed \
                     detection, it only looked like it did. Delete the line; every \
                     detected PII class is masked.",
                    path.display()
                )));
            }
```

and add, next to `Config::load`:

```rust
/// True when `content` still sets the removed `[privacy] kinds` key. Parsed as
/// TOML rather than grepped, so a `kinds` key in another table — or the word in
/// a comment or string — does not trip it.
fn removed_privacy_kinds(content: &str) -> bool {
    toml::from_str::<toml::Value>(content)
        .ok()
        .and_then(|v| {
            v.get("privacy")
                .and_then(|p| p.get("kinds"))
                .map(|_| true)
        })
        .unwrap_or(false)
}
```

A file that is not valid TOML returns `false` here and falls through to
`toml::from_str::<Config>`, which reports the real parse error — the existing
behavior, unchanged.

**Must NOT reject:** a `kinds` key under any other table (`[executor] kinds`,
`[models."x"] kinds`), the word `kinds` in a comment, or a string value
containing `kinds`. The Test plan pins each.

### 3. Fix the stale limitation (docs/privacy.md)

Replace the bullet quoted in Current state with one that is true today:

```markdown
- **Executor egress (②) is best-effort, not a guarantee.** Redaction is
  one-way: a detected artifact is replaced with `[REDACTED:kind]` or a literal
  term's `[CODE]`, and nothing restores it — the reversible round-trip was
  abandoned after a prototype showed the model replaces tokens with fabricated
  values. What reaches a cloud executor is only as complete as detection: the
  NER pass is best-effort on names and addresses, so a name it misses is a
  leak. Structured PII and listed literal terms are reliable.
```

Change no other bullet. The three around it (best-effort NER, vault honeypot,
cannot retro-protect a chat) are still true.

### 4. Complete the configuration block (docs/privacy.md)

In the `## Configuration ([privacy])` TOML block, add `terms_file` after
`scan_globs`, matching the shape of the lines around it and the wording in
`rexymcp.toml.example:83`:

```toml
# terms_file = "terms.json"                     # F05: JSON file of literal terms that must never reach a cloud model; path relative to the repo root
```

The block must list every key `PrivacyConfig` declares after Spec §1, and no
key it does not. After your edit that is: `enabled`, `engine_base_url`,
`engine_model`, `vault_dir`, `redact_executor_egress`, `scan_globs`,
`terms_file`.

## Acceptance criteria

- [ ] The `PrivacyConfig` doc comment still says `Qwen on the LAN` —
      `grep -c 'Qwen on the' executor/src/config.rs` is `1`.
- [ ] `grep -c 'is a cloud host' executor/src/config.rs` is `1`: the
      `redact_executor_egress` comment is intact.
- [ ] `PrivacyConfig` has no `kinds` field, and `grep -rn 'privacy.kinds\|kinds:' executor/src/config.rs`
      finds nothing.
- [ ] A config file containing `[privacy]` with `kinds = ["email"]` makes
      `Config::load` return `Err(Error::Config(m))` where `m` names the file
      path and `kinds`.
- [ ] A config with a `kinds` key in a different table, or the word in a
      comment, still loads.
- [ ] A config with no `[privacy]` section still loads with
      `privacy.enabled == false`.
- [ ] `docs/privacy.md` no longer claims executor egress is not automated, and
      contains no occurrence of "is not automated".
- [ ] The `## Configuration` block lists exactly the seven live keys, including
      `terms_file`.
- [ ] Each new non-ignored test fails when its fix is reverted (show one in the
      Update Log).
- [ ] `cargo fmt --all --check`, `cargo build` (zero warnings),
      `cargo clippy --all-targets --all-features -- -D warnings` and
      `cargo test` all pass.

## Test plan

All in `executor/src/config.rs`'s existing test module, which already writes
TOML to a `tempfile::TempDir` — follow `privacy_defaults_when_absent`
(line 688) for the shape.

1. **`removed_privacy_kinds_fails_the_load`** — write a config with:

   ```toml
   [project]
   id = "x"

   [privacy]
   enabled = true
   kinds = ["email"]
   ```

   `Config::load` is `Err(Error::Config(m))`; `m` contains `kinds` and the
   config file's path.
2. **`kinds_in_another_table_still_loads`** — **must NOT reject.** Three cases,
   each loading `Ok`:
   - `[executor]` with `kinds = ["email"]` added alongside the normal keys.
     (The architect checked this on 2026-09-17: unknown keys in other tables are
     ignored, so such a config loads today and must keep loading.);
   - a config whose `[privacy]` section has a comment line
     `# kinds = ["email"]`;
   - a config with `terms_file = "kinds.json"` — the word inside a string value.
3. **`privacy_defaults_when_absent`** — the existing test at line 688. **Delete
   only its `assert!(cfg.privacy.kinds.is_empty());` line**; keep the rest. That
   is the one permitted change to an existing test.
4. **`malformed_toml_still_reports_a_parse_error`** — a config whose body is
   `not = [valid` returns `Err(Error::Config(_))` whose message does **not**
   mention `kinds`. This pins that the new check does not swallow the real parse
   error.

## End-to-end verification

The real artifact here is the binary refusing a config, and the two docs. Run
this as **one** bash command and paste the output:

```bash
T=$(mktemp -d)
mkdir -p "$T/repo"
cargo build -q -p rexymcp
BIN="$PWD/target/debug/rexymcp"
printf '[project]\nid = "x"\n\n[executor]\nprovider = "openai"\nmodel = "m"\nbase_url = "http://localhost:9/v1"\n\n[privacy]\nenabled = true\nkinds = ["email"]\n' > "$T/repo/rexymcp.toml"
"$BIN" health --config "$T/repo/rexymcp.toml" > "$T/with_kinds.txt" 2>&1; echo "with_kinds_exit=$?"
sed -i '/^kinds = /d' "$T/repo/rexymcp.toml"
"$BIN" health --config "$T/repo/rexymcp.toml" > "$T/without_kinds.txt" 2>&1; echo "without_kinds_exit=$?"
echo "--- with kinds:"; cat "$T/with_kinds.txt"
echo "--- without kinds (positive control):"; head -2 "$T/without_kinds.txt"
echo "doc_stale_claim=$(grep -c 'is not automated' docs/privacy.md)"
echo "doc_terms_file=$(grep -c 'terms_file' docs/privacy.md)"
```

Expected: `with_kinds.txt` names `kinds` and the config path.
`without_kinds.txt` instead reads `unreachable: http://localhost:9/v1` — the
health check ran, which is the positive control that the refusal came from the
key and not from the config being broken in general. `doc_stale_claim=0`;
`doc_terms_file` at least 2 (the mention at line 50 plus the new config line).

**Do not read the exit codes as the signal.** The architect checked on
2026-09-17: `health` exits 1 both for a config error and for an unreachable
endpoint, so both runs exit non-zero. The message text is what distinguishes
them, which is why both files are printed.

**Paste that output into the Update Log.** A green `cargo test` does not show
that the shipped binary refuses the config.

## Authorizations

- [x] May edit `executor/src/config.rs`, including its test module.
- [x] May delete the single `kinds: vec![],` line in
      `executor/src/privacy/egress.rs:573`. No other change to that file.
- [x] May delete one assertion from `privacy_defaults_when_absent`.
- [x] May edit `docs/privacy.md` as described in Spec §3 and §4.
- [ ] May implement `kinds` instead of removing it — **no.** That decision is
      made; see Goal.
- [ ] May add a dependency — **no.** `toml` is already there.
- [ ] May edit `rexymcp.toml.example`, `executor/src/privacy/**` (beyond the one
      line above), or `executor/src/security/**` — **no.**

## Out of scope

- **Implementing per-kind scoping.** If an operator ever needs it, it should be
  a widening control (mask *more*), not a narrowing one, and it needs its own
  phase with a threat model.
- **Other stale statements in `docs/privacy.md`.** Only the egress bullet and
  the configuration block are in scope. If you spot another, note it in "Notes
  for review" — do not fix it.
- **`README.md`'s privacy section.** Not in scope; file it if it is stale.
- **Rejecting other removed keys.** There is only one.

## Update Log

(Filled in by the executor. See WORKFLOW.md § "Update Log entries".)

<!-- entries appended below this line -->

### Update — 2026-09-18 11:24 (progress)

**Executor:** local rexyMCP run (RedHatAI/Qwen3.8-27B-INT4). Pre-flight clean:
`git status --short` empty, `cargo test -p rexymcp-executor config` → 77 passed,
1 ignored. All four spec tasks implemented and verified.

### Update — 2026-09-18 11:24 (end-to-end verification)

Removed-key refusal vs. the same config with the key gone (one session, same
binary):

```
with_kinds_exit=1
without_kinds_exit=1
--- with kinds:
Error: config: /tmp/tmp.sqoebCuNzW/repo/rexymcp.toml: `[privacy] kinds` was removed in F05 — it never narrowed detection, it only looked like it did. Delete the line; every detected PII class is masked.
--- without kinds (positive control):
unreachable: http://localhost:9/v1
doc_stale_claim=0
doc_terms_file=2
```

Both runs exit 1 (health exits 1 on both a config error and an unreachable
endpoint — the message text is the signal): with `kinds = ["email"]` the config
is refused naming both the path and the key; with the line removed, the health
check runs and reports `unreachable: http://localhost:9/v1`, proving the refusal
came from the key, not from a broken config.

Spec-pinned literal grep: `grep -c 'Qwen on the' executor/src/config.rs` = 1 (the
model word survived the phase unchanged); `grep -c 'is a cloud host'
executor/src/config.rs` = 1 (the `redact_executor_egress` comment intact).

Test-revert proof: with the `Config::load` guard bypassed (call replaced by a
no-op call that still compiles), `removed_privacy_kinds_fails_the_load` fails
with `expected Err(Error::Config), got Ok(Config { … privacy: PrivacyConfig {
enabled: true, … } })`; the guard restored, it passes.

### Update — 2026-09-17 (escalation)

**Chosen lever:** refined re-dispatch, on the **local** executor
**Rationale:** the spec was not the problem — it carried the exact code for
every edit. `deepseek-flash` invented an unrelated change (`Qwen` →
`llama.cpp`), truncated a comment mid-word, and then oscillated between
`patch_lines` and an `od -c` byte dump for ~40 turns until the governor stopped
it. The architect reverted the corrupted `config.rs`; the tree is clean and
builds. A spec gap would justify re-dispatching to the same model, but precise
in-place edits in existing code are where this model failed, and the local
`RedHatAI/Qwen3.8-27B-INT4` has done that reliably across eight phases today.
The Gotcha section above pins both mistakes as must-not-repeat, with a recovery
instruction for a malformed edit.

**First real cloud dispatch — what held.** The privacy and sandbox work all
behaved: 2,239 `[REDACTED:…]` markers on the wire, the session log written
`0600` inside a `0700` directory, `bash` confined to bwrap, and `.git` and
`rexymcp.toml` never successfully touched. The pre-scan took about 90 minutes
on its first uncached run and wrote `egress-index.enc`, so later cloud
dispatches skip unchanged files. The failure was executor quality, not
protection.

### Update — ts=1789731891105 (complete, server-authored)

**Summary:** **Summary.** Phase 5 complete: removed the inert `PrivacyConfig.kinds` field (plus its doc-comment clause and the `kinds: vec![]` literal in `executor/src/privacy/egress.rs:573`), added `Config::load` refusal — `removed_privacy_kinds` parses the file as `toml::Value` so only a `kinds` key under `[privacy]` trips it, with an error naming the config path — and fixed both stale parts of `docs/privacy.md` (the false "executor egress is not automated" bullet, and the missing `terms_file` line in the configuration block). Four new tests plus the one permitted deletion from `privacy_defaults_when_absent`; all gates pass (`fmt --check`, `build`, `clippy -D warnings`, `test` 1200 passed). E2E: the shipped binary refuses the `kinds` config with the removal message, and the same config with the key gone runs the health check (`unreachable: http://localhost:9/v1`) — the positive control the spec required.

**Notes for review.**
- `Qwen on the LAN` in the `PrivacyConfig` doc comment is preserved exactly as the spec demanded (`grep -c 'Qwen on the'` = 1); the prior dispatch's `llama.cpp` invention did not recur.
- One deviation from the spec's sketch: the call site reads `Self::removed_privacy_kinds(...)` rather than bare `removed_privacy_kinds(...)` — the latter doesn't resolve inside `impl Config` without a `use`; behavior identical.
- Test-revert proof (in the Update Log): bypassing the guard makes `removed_privacy_kinds_fails_the_load` fail with `expected Err(Error::Config), got Ok(...)`; restoring it passes.
- `rexymcp.toml.example` untouched as authorized; no other files touched.

**Executor:** RedHatAI/Qwen3.8-27B-INT4

**Gates:** format=run, build=run, lint=run, test=run

**Command output tails:**

```
FORMAT


BUILD
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.55s


LINT
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.55s


TEST
ymbols::tests::rejects_invalid_kind ... ok
test tools::symbols::tests::references_finds_call_sites ... ok
test tools::symbols::tests::rejects_max_results_zero ... ok
test tools::symbols::tests::rejects_nonexistent_path ... ok
test tools::symbols::tests::rejects_path_outside_root ... ok
test tools::symbols::tests::single_file_unsupported_extension_advisory_error ... ok
test tools::symbols::tests::type_mismatch_returns_recovery_hint ... ok
test tools::update_task::tests::flips_active_task_to_done ... ok
test tools::update_task::tests::flips_pending_task_to_active ... ok
test tools::update_task::tests::invalid_args_hint_reports_all_complete ... ok
test tools::update_task::tests::invalid_args_hint_lists_incomplete_ids ... ok
test tools::update_task::tests::invalid_state_returns_advisory_error ... ok
test tools::update_task::tests::malformed_args_returns_advisory_error ... ok
test tools::update_task::tests::metadata_shape_is_unchanged ... ok
test tools::update_task::tests::null_args_returns_recovery_hint ... ok
test tools::update_task::tests::result_flags_redundant_remark ... ok
test tools::update_task::tests::result_lists_remaining_incomplete_ids ... ok
test tools::update_task::tests::result_reports_all_complete_when_last_done ... ok
test tools::update_task::tests::success_output_names_task ... ok
test tools::update_task::tests::unknown_id_returns_advisory_error ... ok
test tools::write_file::tests::append_creates_file_if_missing ... ok
test tools::write_file::tests::append_false_overwrites ... ok
test tools::write_file::tests::appends_to_existing_file ... ok
test tools::write_file::tests::creates_new_file ... ok
test tools::write_file::tests::missing_path_returns_recovery_hint ... ok
test tools::write_file::tests::non_object_args_do_not_panic ... ok
test tools::symbols::tests::references_across_multiple_files ... ok
test tools::write_file::tests::overwrites_existing_file ... ok
test tools::write_file::tests::rejects_malformed_args ... ok
test tools::write_file::tests::reports_missing_parent_dir ... ok
test tools::write_file::tests::scope_escape_returns_advisory_error_and_writes_nothing ... ok
test tools::write_file::tests::success_output_includes_line_count ... ok
test tools::symbols::tests::metadata_carries_definitions_and_files_count ... ok
test tools::symbols::tests::references_snippet_shows_source_line ... ok
test tools::symbols::tests::references_truncation_note_omits_kind_filter ... ok
test tools::symbols::tests::unsupported_extension_skipped_in_dir_walk ... ok
test tools::symbols::tests::respects_gitignore ... ok
test tools::symbols::tests::finds_rust_struct_and_trait ... ok
test tools::symbols::tests::reports_line_and_column ... ok
test tools::bash::tests::cargo_command_output_is_filtered_through_cargo_filter ... ok
test store::telemetry::tests::append_is_atomic_under_concurrent_appenders ... ok
test ai::tests::stream_next_uses_supplied_timeout ... ok
test ai::backends::openai::tests::first_token_stall_retries_then_succeeds ... ok
test ai::backends::openai::tests::midstream_stall_is_not_retried ... ok
test tools::bash::tests::arg_timeout_overrides_constructor_default ... ok
test tools::bash::tests::default_timeout_used_when_arg_absent ... ok
test tools::bash::tests::times_out_advisory_failure ... ok
test ai::backends::openai::tests::first_token_stall_exhausts_retries_then_errors ... ok
test health::tests::check_returns_unreachable_on_connection_error ... ok

test result: ok. 1200 passed; 0 failed; 10 ignored; 0 measured; 0 filtered out; finished in 6.21s


running 0 tests

test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

    Finished `test` profile [unoptimized + debuginfo] target(s) in 0.14s
     Running unittests src/main.rs (target/debug/deps/rexymcp-3de3446ef9b6a3ce)
     Running tests/readme_config_reference.rs (target/debug/deps/readme_config_reference-22f887757c17cb2b)
     Running unittests src/lib.rs (target/debug/deps/executor-0c52bf72536f9e8e)
   Doc-tests executor

```

**Files changed:**

- `docs/dev/milestones/F05-privacy-security-hardening/README.md` — +1 -1
- `docs/dev/milestones/F05-privacy-security-hardening/phase-05-config-and-doc-truth.md` — +38 -1
- `docs/privacy.md` — +8 -4
- `executor/src/config.rs` — +92 -4
- `executor/src/privacy/egress.rs` — +0 -1

**Commit:** 063ff8e7432904bafb3f618c39c3e3bcda240b81

**Notes:** server-authored completion entry (executor no longer owns the bookkeeping tail; see M27 phase-03).

### Review verdict — 2026-09-18

- **Verdict:** approved_after_1
- **Bounces:** 1 escalation, not a bounce: the 2026-09-17 `deepseek-flash`
  dispatch ended `hard_fail` (oscillation) at 91 turns and was re-dispatched to
  the local executor with the Gotcha section added. The local run finished in
  89 turns.
- **Executor:** RedHatAI/Qwen3.8-27B-INT4 (after `deepseek-flash` hard-failed)
- **Scope deviations:** none. The `egress.rs` edit is exactly the one
  authorized line (`kinds: vec![],`).
- **Calibration:** `deepseek-flash` scored 30/30 on a greenfield benchmark file
  but could not make precise edits inside dense existing code here — it invented
  an unrelated change and then oscillated repairing a line it had broken. Route
  cloud work to greenfield or additive phases; keep in-place edits in existing
  modules on the local executor.

Independent re-run: fmt/build/clippy clean (0 warnings). `cargo test`:
716 + 2 + 1200 passed (10 ignored), up from 1197.

Acceptance greps, all as specified:

```
kinds_field=0  qwen_intact=1  cloud_host_intact=1  doc_stale=0  doc_terms_file=2
```

The middle two exist because the cloud run destroyed both strings; they are the
guard against that recurring.

End-to-end against the built binary, by the architect:

```
with kinds:     Error: config: <path>/rexymcp.toml: `[privacy] kinds` was removed in F05 — it never
                narrowed detection, it only looked like it did. Delete the line; every detected PII
                class is masked.
without kinds:  unreachable: http://localhost:9/v1      (control: the health check ran)
kinds under [executor]: unreachable: http://localhost:9/v1   (must-not-reject case)
```

Mutation check: forcing `removed_privacy_kinds` to return `false` fails
`removed_privacy_kinds_fails_the_load`. No `unwrap`/`expect`/`panic!`,
`println!`, `#[allow]` or `unsafe` in the production part of `config.rs`.
