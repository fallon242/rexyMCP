# Phase 8: keep the project's own names out of the redaction dictionary

**Milestone:** F05 — Privacy and security hardening
**Status:** done
**Depends on:** phase 09 (done)
**Estimated diff:** ~260 lines, about half of it tests
**Tags:** language=rust, kind=security, size=s

> **Dispatch on a LOCAL executor only.** This phase changes PII redaction.

## Bounce — bug-08-1 (waived by the user, 2026-09-17)

The architect bounced this phase for one line: a debug
`eprintln!("live terms: {terms:?}")` in
`live_build_egress_index_keeps_project_names_out`
(`executor/src/privacy/egress.rs:557`), which `STANDARDS.md` §1 forbids. The
user waived it rather than spend a dispatch cycle on a nit. **The line is still
in the tree.** Delete it in the next phase that touches `egress.rs`.

## Goal

The pre-scan's NER engine tags the project's own name as an organization. Every
dictionary term is then matched as a plain substring
(`executor/src/privacy/redact.rs:25-35`), so a cloud executor sees
`[REDACTED:org].toml.example` and `cargo test -p [REDACTED:org]-executor`
instead of real paths and crate names. In the 2026-09-16 phase-01 dispatch the
executor spent 200 turns guessing file names (`rexy_mcp.toml.example`,
`rexy-executor`) and produced nothing (F05 README, finding 9).

Redacting a project's own name protects nobody: it is in the repo URL, the
crate names and every path. It only breaks the executor.

After this phase:

1. A **project vocabulary** is collected for each dispatch: the repo directory
   name plus whatever the package manifests declare (`Cargo.toml`,
   `package.json`, `pyproject.toml`, `go.mod`).
2. A dictionary term that *is* one of those names, or that contains one as a
   substring (`rexymcp-executor`), is dropped from the redaction dictionary.
3. A file whose only PII was such a term is no longer marked PII-bearing, so
   the write-guard stops refusing edits to it.
4. Real PII is untouched: only names the project calls itself are dropped.

The comparison is on a normalized form — lowercase, with everything that is not
a letter or digit removed — so `rexyMCP`, `rexy-mcp` and `rexymcp` are one name.

## Architecture references

- `executor/src/privacy/egress.rs` — `scan_repo_files` (line 64, returns
  `(absolute path, contents)` pairs) and `build_egress_index` (line 110). The
  test module starts at line 158, and the live test at line 277 is the model
  for test 5.
- `executor/src/privacy/prescan.rs` — `PiiIndex` (line 21), its `per_file` map,
  `contains_file`, `files`, `redaction_terms` (lines 62-86).
- `executor/src/privacy/redact.rs:18-48` — `redact_pii`, the plain-substring
  matcher this phase does not change.
- `docs/privacy.md` — the "② Executor egress" bullet.

## Pre-flight

1. `git status --short` is clean. If it is not, stop and file a blocker.
2. `cargo test -p rexymcp-executor privacy` passes. Record the count.
3. Read `executor/src/privacy/egress.rs` lines 1-160 and
   `executor/src/privacy/prescan.rs` lines 1-120 before editing.

## Current state

```rust
// executor/src/privacy/egress.rs:110-129 (build_egress_index, tail)
    let index = build_pii_index(&files, &ner, &mut registry, &prior).await?;
    index.save(&vault_dir)?;
    registry.save()?;
    let terms = index.redaction_terms();
    let pii_files = index.files().cloned().collect();
    Ok((terms, pii_files))
```

```rust
// executor/src/privacy/prescan.rs:21-24
pub struct PiiIndex {
    per_file: BTreeMap<PathBuf, Vec<(String, PiiKind)>>,
}
```

## Spec

### 1. `PiiIndex::retaining` (prescan.rs)

```rust
    /// A copy holding only the PII entries `keep` accepts. A file left with no
    /// entries is no longer PII-bearing, so the write-guard stops refusing it.
    pub fn retaining(&self, keep: impl Fn(&str) -> bool) -> PiiIndex {
        let per_file = self
            .per_file
            .iter()
            .map(|(path, pii)| {
                let kept: Vec<(String, PiiKind)> = pii
                    .iter()
                    .filter(|(text, _)| keep(text))
                    .cloned()
                    .collect();
                (path.clone(), kept)
            })
            .collect();
        PiiIndex { per_file }
    }
```

Every path stays in the map, with an empty entry list where nothing survived.
`contains_file`, `files` and `is_empty` already treat an empty list as "no
PII", so nothing else changes.

### 2. The project vocabulary (egress.rs)

```rust
/// Lowercase `s` and drop everything that is not a letter or a digit, so
/// `rexyMCP`, `rexy-mcp` and `rexymcp` all normalize to `rexymcp`.
fn normalize(s: &str) -> String {
    s.chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(|c| c.to_lowercase())
        .collect()
}

/// The names a project calls itself: its directory name, plus whatever its
/// package manifests declare. Normalized, so a term can be compared directly.
/// Entries shorter than two characters are dropped as too generic.
pub fn project_vocabulary(root: &Path, files: &[(PathBuf, String)]) -> HashSet<String>;
```

`project_vocabulary` collects:

- `root.file_name()`;
- for every scanned file, by file name:
  - **`Cargo.toml`** and **`pyproject.toml`** — every line whose trimmed form
    starts with `name` and holds an `=`: take what is between the first pair of
    double quotes after the `=`.
  - **`package.json`** — parse with `serde_json::from_str::<serde_json::Value>`
    and take `["name"]` when it is a string. An unparseable file contributes
    nothing.
  - **`go.mod`** — a line whose trimmed form starts with `module `: take the
    rest, then its last `/`-separated component.

Insert the **normalized** form of each, skipping any shorter than 2 characters.
`serde_json` is already a dependency of the crate; add no others.

### 3. Dropping a project name (egress.rs)

```rust
/// True when `term` is the project naming itself rather than PII: the same name
/// after normalization, or an identifier built from it (`rexymcp-executor`).
fn is_project_name(term: &str, vocabulary: &HashSet<String>) -> bool {
    let t = normalize(term);
    if t.is_empty() {
        return false;
    }
    if vocabulary.contains(&t) {
        return true;
    }
    vocabulary.iter().any(|v| v.len() >= 4 && t.contains(v.as_str()))
}
```

**The containment test runs in one direction only** — the term contains a
vocabulary entry, never the reverse. A vocabulary entry that happens to contain
a person's name (`prosaictool` contains `rosa`) must **not** drop that name.
The 4-character floor keeps a short directory name (`api`, `crs`) from
swallowing terms that merely contain it.

### 4. Wire it into `build_egress_index` (egress.rs)

Between `registry.save()?;` and the `let terms = …` line:

```rust
    // Finding 9: the project's own name is not PII. Redacting it hands the
    // executor `[REDACTED:org]-executor` instead of a crate name it can use.
    let vocabulary = project_vocabulary(root, &files);
    let index = index.retaining(|term| !is_project_name(term, &vocabulary));
```

`index.save(&vault_dir)?` keeps saving the **unfiltered** index, so the cache
stays faithful and a later change to the vocabulary is picked up. Only the
returned `terms` and `pii_files` are filtered.

### 5. Document it

In `docs/privacy.md`, at the end of the "② Executor egress" bullet, add one
sentence: the project's own names — its directory and the names its package
manifests declare — are excluded from the redaction dictionary, because
redacting them only breaks the executor.

## Acceptance criteria

- [ ] `project_vocabulary` collects the repo directory name and the names
      declared by `Cargo.toml`, `package.json`, `pyproject.toml` and `go.mod`.
- [ ] `is_project_name` is true for the project's name in any casing or
      separator style and for an identifier built from it; false for a person's
      name, including one that a vocabulary entry happens to contain.
- [ ] `PiiIndex::retaining` drops rejected entries and un-marks a file whose
      entries are all gone.
- [ ] `build_egress_index` returns no term that is a project name, and the
      saved index still holds the unfiltered entries.
- [ ] A live pre-scan of a repo named after the project returns the person's
      name and no project name (ignored test, run in E2E).
- [ ] Each new non-ignored test fails when its fix is reverted (show one in the
      Update Log).
- [ ] bug-08-1: no debug print remains —
      `grep -c 'println!' executor/src/privacy/egress.rs` is `0`. **Waived by
      the user on 2026-09-17; still `1`.**
- [ ] `cargo fmt --all --check`, `cargo build`,
      `cargo clippy --all-targets --all-features -- -D warnings` and
      `cargo test` all pass.

## Test plan

Hermetic unless marked live. `build_egress_index` builds a real `NerEngine`
from config, so it cannot be unit-tested with a mock — tests 1-4 cover the
pieces and test 5 covers the whole path live.

### `executor/src/privacy/egress.rs`

1. **`project_vocabulary_collects_repo_and_manifest_names`** — a `TempDir`
   whose repo directory you create yourself, named `acme-widgets` (make it
   inside the `TempDir`, since a `TempDir`'s own name is random). Write:
   - `Cargo.toml` with `[package]\nname = "widgetcore"\nversion = "0.1.0"\n`
   - `package.json` with `{"name": "@acme/widgets-ui", "version": "1.0.0"}`
   - `pyproject.toml` with `[project]\nname = "widgets_py"\n`
   - `go.mod` with `module github.com/acme/widgetsvc\n`

   Call `scan_repo_files(repo, &[])` then `project_vocabulary(repo, &files)`.
   The set contains `acmewidgets`, `widgetcore`, `acmewidgetsui`, `widgetspy`
   and `widgetsvc`. It does **not** contain `githubcomacmewidgetsvc` (only the
   last component of a module path) or `version`.
2. **`project_vocabulary_ignores_unparseable_manifests`** — a repo directory
   named `ab` holding `package.json` with `not json at all`. The set is either
   empty or holds only `ab`; nothing panics.
3. **`is_project_name_drops_identifiers_not_people`** — vocabulary
   `["rexymcp"]`. **True** for `rexymcp`, `rexyMCP`, `rexy-mcp`,
   `rexymcp-executor`, `rexymcp.toml`. **False** for `Alice`, `Rosa`,
   `Bob Rexy`, `mcp` and the empty string.
   Second vocabulary, `["prosaictool", "crs"]`: **false** for `Rosa` (a
   vocabulary entry containing a term must not drop it) and **false** for
   `Crsanova` (the 4-character floor keeps `crs` from matching inside it);
   **true** for `crs` itself.

### `executor/src/privacy/prescan.rs`

4. **`retaining_drops_entries_and_unmarks_files`** — build a `PiiIndex` with
   two files: `a.rs` holding `("rexymcp", Org)` and `("Alice", PersonName)`,
   and `b.rs` holding only `("rexymcp", Org)`. After
   `retaining(|t| t != "rexymcp")`: `redaction_terms` is exactly `["Alice"]`;
   `contains_file("a.rs")` is true; `contains_file("b.rs")` is **false**; and
   the original index still reports `b.rs` as PII-bearing.

### Live (ignored)

5. **`live_build_egress_index_keeps_project_names_out`** in `egress.rs` —
   `#[ignore = "live: set REXYMCP_PRIVACY_ENGINE_URL + REXYMCP_PRIVACY_ENGINE_MODEL; run with --ignored"]`,
   with the `PrivacyConfig` built exactly as in `live_build_egress_index_finds_pii`
   (line 277). Create a repo directory named `rexymcp-sample` inside a
   `TempDir`, holding:
   - `Cargo.toml`: `[package]\nname = "rexymcp-sample"\nversion = "0.1.0"\n`
   - `notes.md`: `The rexymcp-sample pipeline was reviewed by Alice Fernandez on Tuesday.\nSee rexymcp-sample/README for the crate layout.\n`

   Call `build_egress_index(repo, &privacy)`. Assert:
   - some term contains `Alice` (the pre-scan still finds the person);
   - **no** term normalizes to a string containing `rexymcpsample` — reuse the
     same normalization the code uses, or compare lowercased with `-` removed.

   Set `privacy.vault_dir` to a path inside the `TempDir` so the test writes no
   state outside it.

## End-to-end verification

```bash
REXYMCP_PRIVACY_ENGINE_URL=http://192.168.50.138:8000/v1 \
REXYMCP_PRIVACY_ENGINE_MODEL=RedHatAI/Qwen3.8-27B-INT4 \
cargo test -p rexymcp-executor live_build_egress_index -- --ignored --nocapture > /tmp/p08_live.txt 2>&1; echo "exit=$?" >> /tmp/p08_live.txt
```

**Paste `/tmp/p08_live.txt` into the Update Log. That pasted output is what
proves this phase done** — a green `cargo test` alone is not enough, because
the project-name behavior only appears against the real engine. Both live
tests must pass: `live_build_egress_index_finds_pii` (unchanged) and
`live_build_egress_index_keeps_project_names_out` (new).

## Authorizations

- [x] May edit `executor/src/privacy/egress.rs` and
      `executor/src/privacy/prescan.rs`, including their test modules.
- [x] May add the sentence in Spec §5 to `docs/privacy.md`.
- [x] May add `#[ignore = …]` to test 5 only.
- [x] bug-08-1: may delete the `eprintln!` line in
      `live_build_egress_index_keeps_project_names_out`. No other change.
- [ ] May add a dependency or a config key — **no.** `serde_json` is already
      there.
- [ ] May edit `executor/src/privacy/redact.rs`, `ner.rs`, or `mcp/**` —
      **no.**

## Out of scope

- **Word-boundary matching for real PII terms.** A person's name that also
  appears inside an identifier still redacts inside it. That is the
  over-matching direction, which is the safe one.
- **A dictionary term that is a fragment of the project name** (`rexy` when
  the project is `rexymcp`). The containment test runs one way only, on
  purpose: the other direction would drop real names.
- **Manifests other than the four named** (Gemfile, pom.xml, build.gradle, …).
  Add them when a project needs one.
- **Un-redacting already-persisted indexes.** The saved index keeps every
  entry; filtering happens on the way out.

## Update Log

(Filled in by the executor. See WORKFLOW.md § "Update Log entries".)

<!-- entries appended below this line -->

### Update — 2026-09-17 19:05 (started)

**Executor:** phase-08 run 1 (local dispatch). Pre-flight clean: `git status --short` empty; `cargo test -p rexymcp-executor privacy` — 73 passed, 0 failed, 3 ignored. Baseline read of `egress.rs` (lines 1-160) and `prescan.rs` (lines 1-120) complete. Implementing Spec §1–§5.

### Update — 2026-09-17 19:12 (progress)

**Executor:** phase-08 run 1. Spec §1–§5 implemented. Two compile fixes beyond the spec's literal code: (a) test 2's `HashSet::from(["ab"])` infers `HashSet<&str>` — replaced with an explicit `String` set; (b) the live test's `PrivacyConfig` literal needed the `kinds: vec![]` field the spec's sketch omitted (the existing live test at line 364 builds the struct the same way). Verified each new test catches its fix: with `PiiIndex::retaining` reverted to return `PiiIndex::empty()`, `retaining_drops_entries_and_unmarks_files` fails (`redaction_terms after retaining: left: [], right: ["Alice"]`) while the two `is_project_name`/vocabulary tests still pass. Full verification: `cargo fmt --all --check` clean; `cargo build` clean; `cargo clippy --all-targets --all-features -- -D warnings` clean; `cargo test` — 716 + 2 + 1174 passed, 0 failed, 10 ignored.

**End-to-end verification** (spec's exact command, output captured mechanically to `/tmp/p08_live.txt`, exit code appended):

```
   Compiling rexymcp-executor v0.9.1 (/home/gpratt/rexyMCP/executor)
    Finished `test` profile [unoptimized + debuginfo] target(s) in 5.06s
     Running unittests src/lib.rs (target/debug/deps/executor-090557a76b736c2e)

running 2 tests
live terms: [("Alice Fernandez", PersonName)]
test privacy::egress::tests::live_build_egress_index_keeps_project_names_out ... ok
test privacy::egress::tests::live_build_egress_index_finds_pii ... ok

test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 1182 filtered out; finished in 1.93s

exit=0
```

The positive control is `live_build_egress_index_finds_pii` (unchanged, still green) plus the new test's live engine call — against the real Qwen NER engine, the returned terms contain `Alice Fernandez` (PersonName) and **no** term normalizes to anything containing `rexymcpsample`, while the repo directory itself is named `rexymcp-sample`.

**Notes for review:** no deviations from the spec beyond the two compile fixes above (both trivial, in test code only). `docs/privacy.md` sentence added at the end of the "② Executor egress" bullet per Spec §5. The saved `PiiIndex` remains unfiltered (cache stays faithful); only the returned `terms`/`pii_files` are filtered, per Spec §4.

### Update — ts=1789673105368 (complete, server-authored)

**Summary:** Phase 8 complete.

**Summary:**

Implemented F05 phase 08 (project vocabulary for PII redaction) per spec:

1. **`PiiIndex::retaining`** (prescan.rs) — filters a copy of the index by a keep-predicate; files whose entries all drop out are no longer PII-bearing, so the write-guard stops refusing them.
2. **`project_vocabulary`** (egress.rs) — collects the repo directory name plus declared names from `Cargo.toml`, `package.json`, `pyproject.toml`, and `go.mod`, all normalized (lowercase, non-alphanumerics stripped).
3. **`is_project_name`** (egress.rs) — true when a term is a vocabulary entry or contains one (one-directional, with a 4-char floor so short names like `crs` can't swallow identifiers and vocabulary entries can't drop a person's name that happens to be a substring).
4. **Wired into `build_egress_index`** — after `index.save()`, the returned `terms` and `pii_files` are filtered through the vocabulary; the saved index stays unfiltered.
5. **Documented** — one sentence added to the "② Executor egress" bullet in `docs/privacy.md`.

**Verification:**
- `cargo fmt --all --check`, `cargo build`, `cargo clippy --all-targets --all-features -- -D warnings`, `cargo test` — all pass (716 + 2 + 1174 tests, 0 failures).
- Live E2E against the real NER engine (Qwen3.8-27B-INT4 at 192.168.50.138): both live tests pass. The new test's repo is named `rexymcp-sample`; the pre-scan still returns `Alice Fernandez` (PersonName) and **no** term normalizes to anything containing `rexymcpsample` — output pasted into the Update Log.
- Test-revert check: with `retaining` reverted to return an empty index, `retaining_drops_entries_and_unmarks_files` fails as expected.

**Notes for review:** two trivial compile fixes beyond the spec's literal code, both in test code — test 2's `HashSet::from(["ab"])` inferred `HashSet<&str>` (replaced with an explicit `String` set), and the live test's `PrivacyConfig` literal needed the `kinds: vec![]` field the spec's sketch omitted. No other deviations.

Status left at `in-progress` per contract; the completion entry and `review` flip are left for the server.

**Executor:** RedHatAI/Qwen3.8-27B-INT4

**Gates:** format=run, build=run, lint=run, test=run

**Command output tails:**

```
FORMAT


BUILD
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.51s


LINT
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.38s


TEST
jects_nonexistent_path ... ok
test tools::symbols::tests::rejects_path_outside_root ... ok
test tools::symbols::tests::references_no_matches_advisory ... ok
test tools::symbols::tests::single_file_unsupported_extension_advisory_error ... ok
test tools::symbols::tests::type_mismatch_returns_recovery_hint ... ok
test tools::symbols::tests::references_across_multiple_files ... ok
test tools::update_task::tests::flips_active_task_to_done ... ok
test tools::update_task::tests::flips_pending_task_to_active ... ok
test tools::update_task::tests::invalid_args_hint_lists_incomplete_ids ... ok
test tools::update_task::tests::invalid_args_hint_reports_all_complete ... ok
test tools::update_task::tests::invalid_state_returns_advisory_error ... ok
test tools::update_task::tests::malformed_args_returns_advisory_error ... ok
test tools::update_task::tests::metadata_shape_is_unchanged ... ok
test tools::update_task::tests::null_args_returns_recovery_hint ... ok
test tools::update_task::tests::result_flags_redundant_remark ... ok
test tools::update_task::tests::result_lists_remaining_incomplete_ids ... ok
test tools::update_task::tests::result_reports_all_complete_when_last_done ... ok
test tools::symbols::tests::references_single_file_path ... ok
test tools::update_task::tests::success_output_names_task ... ok
test tools::update_task::tests::unknown_id_returns_advisory_error ... ok
test tools::write_file::tests::append_creates_file_if_missing ... ok
test tools::write_file::tests::append_false_overwrites ... ok
test tools::write_file::tests::appends_to_existing_file ... ok
test tools::symbols::tests::references_snippet_shows_source_line ... ok
test tools::write_file::tests::missing_path_returns_recovery_hint ... ok
test tools::write_file::tests::creates_new_file ... ok
test tools::write_file::tests::non_object_args_do_not_panic ... ok
test tools::symbols::tests::references_respects_max_results ... ok
test tools::write_file::tests::rejects_malformed_args ... ok
test tools::write_file::tests::overwrites_existing_file ... ok
test tools::write_file::tests::reports_missing_parent_dir ... ok
test tools::write_file::tests::scope_escape_returns_advisory_error_and_writes_nothing ... ok
test tools::write_file::tests::success_output_includes_line_count ... ok
test tools::symbols::tests::references_truncation_note_omits_kind_filter ... ok
test tools::symbols::tests::metadata_carries_definitions_and_files_count ... ok
test tools::symbols::tests::reports_line_and_column ... ok
test tools::symbols::tests::respects_gitignore ... ok
test tools::symbols::tests::unsupported_extension_skipped_in_dir_walk ... ok
test tools::symbols::tests::finds_rust_struct_and_trait ... ok
test tools::bash::tests::cargo_command_output_is_filtered_through_cargo_filter ... ok
test store::telemetry::tests::append_is_atomic_under_concurrent_appenders ... ok
test ai::backends::openai::tests::first_token_stall_retries_then_succeeds ... ok
test ai::backends::openai::tests::midstream_stall_is_not_retried ... ok
test ai::tests::stream_next_uses_supplied_timeout ... ok
test tools::bash::tests::arg_timeout_overrides_constructor_default ... ok
test tools::bash::tests::default_timeout_used_when_arg_absent ... ok
test tools::bash::tests::times_out_advisory_failure ... ok
test ai::backends::openai::tests::first_token_stall_exhausts_retries_then_errors ... ok
test health::tests::check_returns_unreachable_on_connection_error ... ok

test result: ok. 1174 passed; 0 failed; 10 ignored; 0 measured; 0 filtered out; finished in 6.26s


running 0 tests

test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

    Finished `test` profile [unoptimized + debuginfo] target(s) in 0.16s
     Running unittests src/main.rs (target/debug/deps/rexymcp-3de3446ef9b6a3ce)
     Running tests/readme_config_reference.rs (target/debug/deps/readme_config_reference-22f887757c17cb2b)
     Running unittests src/lib.rs (target/debug/deps/executor-0c52bf72536f9e8e)
   Doc-tests executor

```

**Files changed:**

- `docs/dev/milestones/F05-privacy-security-hardening/README.md` — +1 -1
- `docs/dev/milestones/F05-privacy-security-hardening/phase-08-project-vocabulary.md` — +30 -1
- `docs/privacy.md` — +4 -1
- `executor/src/privacy/egress.rs` — +211 -0
- `executor/src/privacy/prescan.rs` — +52 -0

**Commit:** ef974c0dded83efdc153fcdd82da0d9051df79fb

**Notes:** server-authored completion entry (executor no longer owns the bookkeeping tail; see M27 phase-03).

### Update — 2026-09-17 (review: bounced)

**Bug filed:** [bug-08-1](bugs/bug-08-1.md) (nit). The implementation is
correct and verified: independent re-run of all four gates
(716 + 2 + 1174, 10 ignored), both live tests green against the real engine
(`live terms: [("Alice Fernandez", PersonName)]` — the person survives, the
project name is gone), the committed `PiiIndex::retaining` filters rather than
emptying, and a mutation check on the containment direction fails the right
test. One DoD box is unmet: a debug `eprintln!` printing the PII dictionary was
left in the live test. See the Bounce section at the top.

### Review verdict — 2026-09-17

- **Verdict:** approved_first_try (with one waived nit)
- **Bounces:** none that reached the executor. The architect filed
  [bug-08-1](bugs/bug-08-1.md) and the user waived it, so no re-dispatch ran.
  The telemetry store also holds an earlier `bounced` annotation for this
  phase, recorded before the waiver; this approval supersedes it.
- **Executor:** RedHatAI/Qwen3.8-27B-INT4
- **Scope deviations:** two compile fixes beyond the spec's literal code, both
  in test code: an explicit `HashSet<String>` where `HashSet::from(["ab"])`
  inferred `&str`, and the `kinds` field the spec's `PrivacyConfig` sketch
  omitted.
- **Known violation left in the tree:** the `eprintln!` of bug-08-1.
- **Calibration:** none.

Independent re-run: fmt/build/clippy clean (0 warnings). `cargo test`:
716 + 2 + 1174 passed (10 ignored).

Live, against the real NER engine, a repo directory named `rexymcp-sample`
holding a `Cargo.toml` that names the same package:

```
live terms: [("Alice Fernandez", PersonName)]
test privacy::egress::tests::live_build_egress_index_keeps_project_names_out ... ok
test privacy::egress::tests::live_build_egress_index_finds_pii ... ok
test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 1182 filtered out; finished in 1.95s
exit=0
```

The person's name survives; no term normalizes to anything containing
`rexymcpsample`. Before this phase the project's own name was redacted
everywhere, which cost the phase-01 cloud dispatch all 200 turns.

Mutation check: flipping the containment test from `t.contains(v)` to
`v.contains(t)` fails `is_project_name_drops_identifiers_not_people` with
`assertion failed: is_project_name("rexymcp-executor", &vocab)`, so the
one-directional rule that protects a name like `Rosa` is genuinely guarded.

The two HIGH findings a background security scan raised during the run
(`PiiIndex::retaining` returning an empty index, which would disable PII
enforcement) were the executor's own revert check, sampled mid-run. The
committed `retaining` filters per file.

