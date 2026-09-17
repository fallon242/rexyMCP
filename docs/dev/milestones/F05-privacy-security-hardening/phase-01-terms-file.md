# Phase 1: `[privacy] terms_file`

**Milestone:** F05 — Privacy and security hardening
**Status:** in-progress (bounced: [bug-01-1](bugs/bug-01-1.md))
**Depends on:** phase 06 (done). A failed pre-scan now stops the dispatch, and
a term file that fails to load goes the same way.
**Estimated diff:** ~380 lines, about half of it tests
**Tags:** language=rust, kind=feature, size=m

## Bounce — bug-01-1 (read this first)

**The gates are green and the tree is clean. That is expected here and is NOT
evidence that the phase is done.** The whole feature is implemented and
approved: the config key, `LiteralTerms` loading and masking, the
`RedactingAiClient` wiring, the `build_egress_index` plumbing, the refusal
message and the docs. All 14 tests the Test plan names are present, and the
architect verified they bite (dropping the left `\b` fails three of them).
The end-to-end capture is pasted in the Update Log and shows the right
before/after. Do not change any of that.

There is **one line** left to fix: `executor/src/privacy/terms.rs:150`, in
`LiteralTerms::mask`.

```rust
            let m = caps.get(0).expect("the whole regex matched");
```

`STANDARDS.md` §1 and §2 forbid a new `expect()` in a production path, and
`mask` runs on every outbound message. Replace it with any panic-free form —
`let Some(m) = caps.get(0) else { continue };` keeps the current shape, and
`regex.find_iter(text)` is the other obvious route. Keep the behavior
identical.

**Finish conditions. Check each one yourself before reporting:**

- `awk 'NR<174 && /\.expect\(|\.unwrap\(|panic!\(/' executor/src/privacy/terms.rs`
  prints nothing. (Adjust the bound if your edit moves `mod tests`; the rule is
  no panic path in the production part of the file.)
- `cargo test` still reports 716 / 2 / 1188 (10 ignored): this fix adds no test
  and removes none.
- All four gates pass.

Do **not** re-run the End-to-end harness: it is already pasted in the Update
Log and this fix cannot change it. Say so in your completion entry.

## Goal

Let a repository list literal terms that must never reach a cloud model. The
executor-egress chokepoint replaces each one with a code such as `[SITE_1]`,
next to the NER pre-scan's `[REDACTED:…]` replacements. These are terms a name
detector cannot find: short site codes, system acronyms, and product names
that are ordinary words.

With `terms_file` unset, nothing changes. Not one byte.

## Architecture references

- `executor/src/privacy/redact.rs`: `redact_pii` (line 18) and
  `RedactingAiClient` (lines 50–92). Every outbound message passes through this
  one chokepoint.
- `executor/src/privacy/egress.rs`: `build_egress_index` (line 110),
  `scan_repo_files` (line 64), and the ignored live test at line 277.
- `executor/src/config.rs`: `PrivacyConfig` (line 96).
- `mcp/src/runner.rs`: `run_phase` (the `build_egress_index` match near line
  395, and the `RedactingAiClient::new` call near line 414) and
  `prescan_refusal` (near line 473).
- `docs/dev/milestones/F03-executor-egress-protection/README.md` §"Why
  irreversible redaction". The executor is never asked to put a term back.

## Pre-flight

1. `cargo test -p rexymcp-executor privacy` passes. Record the count.
2. Read `redact.rs` and `build_egress_index` in full before editing.

## Gotcha: do not write real-looking names

This phase runs on a cloud executor with egress redaction on. A person or place
name you write in a source file may be caught by the NER pre-scan on the next
dispatch. That file then joins the write-guard's PII set, and you can no longer
edit it. Use only the made-up terms this doc uses: `Plant Nine`, `PLN`,
`Quillsys`, `Ledgers`. Do not invent town or person names for tests.

## Current state

```rust
// executor/src/privacy/egress.rs:110-131
pub async fn build_egress_index(
    root: &Path,
    privacy: &PrivacyConfig,
) -> Result<(Vec<(String, PiiKind)>, HashSet<PathBuf>)> {
    let ner = NerEngine::from_config(privacy)?;
    let files = scan_repo_files(root, &privacy.scan_globs);
    // Persist the index + registry under the vault dir (M46) so an unchanged file
    // reuses its prior entry — no NER call — on the next dispatch.
    let vault_dir = privacy
        .vault_dir
        .clone()
        .unwrap_or_else(|| root.join(".rexymcp/vault"));
    let prior = PiiIndex::load(&vault_dir)?;
    let mut registry = Registry::load(&vault_dir.join("egress-registry.json"))?;
    let index = build_pii_index(&files, &ner, &mut registry, &prior).await?;
    index.save(&vault_dir)?;
    registry.save()?;
    let terms = index.redaction_terms();
    let pii_files = index.files().cloned().collect();
    Ok((terms, pii_files))
}
```

```rust
// executor/src/privacy/redact.rs:50-72
pub struct RedactingAiClient {
    inner: Box<dyn AiClient>,
    terms: Vec<(String, PiiKind)>,
}

impl RedactingAiClient {
    pub fn new(inner: Box<dyn AiClient>, terms: Vec<(String, PiiKind)>) -> Self {
        Self { inner, terms }
    }

    fn redact_message(&self, mut msg: Message) -> Message {
        msg.content = redact_pii(&msg.content, &self.terms);
        if let Some(results) = msg.tool_results.as_mut() {
            for result in results.iter_mut() {
                result.content = redact_pii(&result.content, &self.terms);
            }
        }
        msg
    }
}
// chat() does: let system = redact_pii(system_prompt, &self.terms);
```

`redact_pii` matches each dictionary term as a **plain substring**, with no
word boundaries and no case handling. It replaces each hit with
`[REDACTED:<tag>]`.

```rust
// mcp/src/runner.rs, inside run_phase
        match rexymcp_executor::privacy::egress::build_egress_index(inp.repo_path, &inp.cfg.privacy)
            .await
        {
            Ok((terms, files)) => {
                egress_terms = terms;
                pii_files = files;
            }
            Err(e) => return Err(prescan_refusal(&e)),
        }
    // ...
            rexymcp_executor::privacy::redact::RedactingAiClient::new(
                Box::new(prod_client),
                egress_terms,
            ),
```

`PrivacyConfig` (`executor/src/config.rs:94-108`) has
`#[derive(Debug, Clone, Serialize, Deserialize, Default)]` and
`#[serde(default)]`. It is documented field by field, like this:

```rust
    /// M46: glob patterns (relative to the repo root) limiting which files the
    /// executor-egress pre-scan walks. Empty = scan everything (gitignore-honored).
    pub scan_globs: Vec<String>,
```

`rexymcp.toml.example` line 82 documents `scan_globs` this way:

```toml
# scan_globs = ["data/**", "fixtures/**"]        # limit the egress pre-scan to these repo-relative globs (M46)
```

The error type is `rexymcp_executor::error::Error`. Use `Error::Privacy(String)`
for every failure in this phase.

`PrivacyConfig.kinds` is dead code: only a test that checks it is empty uses it.
Leave it alone.

## Spec

### 1. Config key

Add `pub terms_file: Option<PathBuf>` to `PrivacyConfig`, with a doc comment in
the `scan_globs` style. A relative path is resolved against the **repo root**.
Add one commented line to `rexymcp.toml.example` under `scan_globs`, in the
same style. There is no default: when the key is absent, the feature is off.

### 2. Loader: new module `executor/src/privacy/terms.rs`

Register it in `executor/src/privacy/mod.rs` (`pub mod terms;`). The file
format is:

```json
{ "entries": [
  { "code": "SITE_1", "aliases": ["Plant Nine", "PLN"] },
  { "code": "SYS_1",  "aliases": ["Quillsys"], "embed": true },
  { "code": "SYS_2",  "aliases": ["Ledgers"],  "match": "case-sensitive" }
] }
```

- `code` and `aliases` are required. `aliases` must be non-empty, and no alias
  may be empty or whitespace-only.
- `embed` is optional and defaults to false. When true, the alias may match at
  the start of a longer identifier.
- `match` is optional. If it is present, with any value, the entry matches
  case-sensitively. With serde:
  `#[serde(default, rename = "match")] case_sensitive: Option<String>`.
- Public API:

  ```rust
  #[derive(Debug, Default)]
  pub struct LiteralTerms { /* private */ }

  impl LiteralTerms {
      /// Load and compile a term file. Any failure is `Error::Privacy` naming the path.
      pub fn load(path: &Path) -> Result<Self>;
      /// Replace every listed alias in `text` with `[CODE]`.
      /// `LiteralTerms::default()` returns `text` unchanged.
      pub fn mask(&self, text: &str) -> String;
  }
  ```

- Each of these is an `Err(Error::Privacy(..))`, and the message contains
  `path.display()`: a missing file, an unreadable file, malformed JSON, an entry
  without `code` or `aliases`, and an empty `aliases` list or empty alias. Do not
  warn and carry on. A term file that silently fails to load is worse than
  having none, because the operator believes they are protected.

### 3. Matcher

Compile every alias of every entry into **one** `regex::Regex`, one capture
group per alias, with the alternatives sorted **longest alias first**. Rust's
`regex` crate uses leftmost-first alternation, so at a given start position the
longest alias wins. Keep a `Vec` that maps group index to the entry's code.

Build each alternative like this:

```rust
// words of the alias, each regex::escape'd, joined by \s+
let body = alias.split_whitespace().map(regex::escape).collect::<Vec<_>>().join(r"\s+");
let first_is_word = alias.chars().next().is_some_and(|c| c.is_alphanumeric() || c == '_');
let last_is_word  = alias.chars().last().is_some_and(|c| c.is_alphanumeric() || c == '_');
let left  = if first_is_word { r"\b" } else { "" };
let right = if last_is_word && !embed { r"\b" } else { "" };
let flags = if case_sensitive { "" } else { "(?i)" };
format!("({flags}{left}{body}{right})")   // (?i) inside the group applies only to that group
```

- A default entry needs a word boundary on **both** sides. `PLN` matches in
  `site PLN today` but not in `PLNX` or `XPLN`.
- An `embed` entry needs a boundary only on the **left**. `Quillsys` matches in
  `QuillsysExport`, which becomes `[SYS_1]Export`, but not in `myQuillsys`.
- `mask` runs `captures_iter`. For each match, it finds the one capture group
  that is `Some`, looks up that group's code, and writes `[CODE]`, followed by
  one `\n` for each `\n` inside the matched text. The line count stays the same.
  All text outside matches is copied unchanged.
- A case-sensitive entry masks `Ledgers` and leaves `ledgers` alone.

### 4. Wiring

1. **`build_egress_index`** returns a struct instead of the tuple:

   ```rust
   pub struct EgressIndex {
       pub terms: Vec<(String, PiiKind)>,
       pub pii_files: HashSet<PathBuf>,
       pub literal: LiteralTerms,
   }
   ```

   Load the term file **first**, before `NerEngine::from_config`, so that a bad
   term file is reported even when the engine is misconfigured. When
   `privacy.terms_file` is `None`, `literal` is `LiteralTerms::default()`. A
   relative path is joined onto `root`.
2. **`RedactingAiClient`**: add a `literal: LiteralTerms` field and a builder.
   **Keep `new`'s signature**, so the existing tests do not change:

   ```rust
   pub fn with_literal_terms(mut self, literal: LiteralTerms) -> Self { self.literal = literal; self }
   ```

   Everywhere the client calls `redact_pii(x, &self.terms)` today, it now calls
   `redact_pii(&self.literal.mask(x), &self.terms)`. That covers the system
   prompt, `msg.content` and each `tool_results[].content`. Literal masking
   runs **first**, so each entry's own rules decide its matches.
   `redact_pii` itself does not change.
3. **`mcp/src/runner.rs`**: destructure `EgressIndex`, keep `literal` in a
   local (default `LiteralTerms::default()` on the no-redaction path), and chain
   `.with_literal_terms(literal)` onto `RedactingAiClient::new(..)`.
4. **`prescan_refusal`** (runner.rs): the message must also name
   `privacy.terms_file` as a setting to check, next to
   `privacy.engine_base_url`. Change nothing else in that function.
5. Update the ignored live test `live_build_egress_index_finds_pii`
   (egress.rs:277) to the new return type: `let idx = …; idx.terms`,
   `idx.pii_files`. Change only that line and the uses of the two bindings.

### 5. Documentation

In `docs/privacy.md`, under the executor-egress description, add a short
paragraph or a bullet list covering:
- the `terms_file` key and that relative paths are repo-relative;
- the file format;
- the `embed` and `match` flags, and the default boundary rule;
- that a missing or bad file stops the dispatch;
- one sentence: the executor is never asked to restore a term.

Change nothing else in that file.

## Acceptance criteria

- [ ] With `terms_file` unset, `build_egress_index` returns an empty `literal`,
      and `RedactingAiClient` output is byte-identical to today's. A test proves
      this.
- [ ] A listed term reaches the inner client as `[CODE]`. This is verified
      through `RedactingAiClient` with `MockAiClient`, not only in the matcher.
- [ ] A missing or malformed term file makes `build_egress_index` return
      `Error::Privacy` with the file path in the message.
- [ ] An `embed` entry matches inside an identifier. A default entry does not.
- [ ] A `match` entry masks `Ledgers` and leaves `ledgers` alone.
- [ ] An alias wrapped across a newline is masked, and the newline count is
      unchanged.
- [ ] `prescan_refusal`'s message names `privacy.terms_file`.
- [ ] End-to-end capture (below) shows `[SITE_1]` on the wire and no
      `Plant Nine`.
- [ ] bug-01-1: no new panic path in production —
      `awk 'NR<174 && /\.expect\(|\.unwrap\(|panic!\(/' executor/src/privacy/terms.rs`
      prints nothing.
- [ ] `cargo fmt --all --check`, `cargo build` (zero warnings),
      `cargo clippy --all-targets --all-features -- -D warnings` and
      `cargo test` all pass.

## Test plan

Unit tests in `executor/src/privacy/terms.rs`, using `tempfile::TempDir` for
files:

- `loads_entries_and_flags`: the three example shapes parse, and flags default
  off.
- `missing_file_is_an_error`, `malformed_json_is_an_error`,
  `entry_without_aliases_is_an_error`, `empty_alias_is_an_error`: each returns
  `Err(Error::Privacy(m))` where `m` contains the path.
- `default_entry_requires_both_boundaries`: masks `PLN` in `go to PLN now`,
  and leaves `PLNX` and `XPLN` unchanged.
- `embed_entry_matches_inside_an_identifier`: `QuillsysExport` becomes
  `[SYS_1]Export`, and `myQuillsys` is unchanged.
- `case_sensitive_entry_spares_the_common_noun`: `Ledgers` is masked, and
  `the ledgers` is unchanged.
- `wrapped_alias_matches_and_keeps_the_break`: `"at Plant\nNine today"`
  becomes `"at [SITE_1]\n today"`, with the same `\n` count.
- `longest_alias_wins`: with aliases `Plant` (code A) and `Plant Nine`
  (code B), `Plant Nine` becomes `[B]`.
- `default_terms_leave_text_unchanged`: `LiteralTerms::default().mask(s) == s`.

In `redact.rs`:
- `literal_terms_redact_through_the_client`: follow the existing
  `redacts_outbound_messages_and_system_prompt` test (same file, line 135),
  which reads what the mock received with `let call = &mock.calls()[0];` and
  then checks `call.system_prompt` and `call.messages[0].content`.
  Build `RedactingAiClient::new(Box::new(mock.clone()), vec![])
  .with_literal_terms(LiteralTerms::load(&file)?)` and send a message containing
  `Plant Nine`. Assert that the mock's recorded message contains `[SITE_1]` and
  does not contain `Plant Nine`. Check the system prompt too.

In `egress.rs`:
- `unset_terms_file_changes_nothing` and `missing_terms_file_fails_the_index`.
  Both run hermetically, with no NER call: `engine_base_url =
  Some("http://localhost:9/v1")`, `engine_model = Some("m")`,
  `scan_globs = vec!["no-such-dir/**".into()]` (so no file is scanned and the
  engine is never called), and `vault_dir` inside the `TempDir`. The first
  asserts `idx.literal.mask(s) == s` and that `idx.terms` is empty. The second
  sets `terms_file` to a missing path and asserts `Error::Privacy` naming it.

In `mcp/src/runner.rs`: add `privacy.terms_file` to the assertions in the
existing `prescan_refusal_redacts_address_and_names_remedies` test. That is
the one existing-test change this phase allows.

## End-to-end verification

This harness was run by the architect on the tree before this phase. It
captured the email as `[REDACTED:email]` and `Plant Nine` in the clear. After
this phase, the same run must show `[SITE_1]`. It uses a local HTTP server that
records request bodies as a stand-in for the cloud executor
(`redact_executor_egress = true` forces redaction for a localhost endpoint), and
a glob that matches nothing, so the NER engine is never called. Run it as
**one** bash command, then paste the output:

```bash
T=$(mktemp -d)
cat > "$T/capture.py" <<'EOF'
import http.server, sys
out = sys.argv[2]
class H(http.server.BaseHTTPRequestHandler):
    def do_POST(self):
        body = self.rfile.read(int(self.headers.get("Content-Length", 0)))
        with open(out, "ab") as f:
            f.write(body + b"\n")
        self.send_response(400); self.end_headers()
    def do_GET(self):
        self.send_response(404); self.end_headers()
    def log_message(self, *a):
        pass
http.server.HTTPServer(("127.0.0.1", int(sys.argv[1])), H).serve_forever()
EOF
mkdir -p "$T/repo"
printf '# Phase 1: e2e\n\n**Status:** todo\n\n## Goal\n\nMail ops@example.com about Plant Nine.\n' > "$T/phase-01-e2e.md"
printf '{"entries":[{"code":"SITE_1","aliases":["Plant Nine"]}]}\n' > "$T/repo/terms.json"
cat > "$T/repo/rexymcp.toml" <<'EOF'
[executor]
provider = "openai"
model = "m"
base_url = "http://127.0.0.1:18765/v1"

[commands]
format = "true"
build = "true"
lint = "true"
test = "true"

[privacy]
enabled = true
redact_executor_egress = true
engine_base_url = "http://localhost:9/v1"
engine_model = "m"
scan_globs = ["no-such-dir/**"]
terms_file = "terms.json"
EOF
python3 "$T/capture.py" 18765 "$T/body.txt" & P=$!
timeout 120 cargo run -q -p rexymcp -- run-phase --no-telemetry \
  --config "$T/repo/rexymcp.toml" --phase-doc "$T/phase-01-e2e.md" --repo "$T/repo" \
  > "$T/run.txt" 2>&1; echo "exit=$?"
kill $P
echo "site_code=$(grep -o 'about \[SITE_1\]' "$T/body.txt" | head -1)"
echo "email_marker=$(grep -o 'REDACTED:email' "$T/body.txt" | head -1)"
echo "plaintext_hits=$(grep -c 'Plant Nine' "$T/body.txt")"
```

Expected: `site_code=about [SITE_1]`, `email_marker=REDACTED:email`,
`plaintext_hits=0`. A non-zero `exit=` is expected, because the capture server
answers 400. `email_marker` is the positive control: it proves that
`body.txt` holds the redacted request. Also run it once with the
`terms_file = …` line removed, and paste that output: `plaintext_hits` must be
non-zero. If `python3` or background processes are unavailable in your
environment, stop and file a blocker. Do not mark the phase done without this
capture.

## Authorizations

- [x] May add `executor/src/privacy/terms.rs` and register it in
      `executor/src/privacy/mod.rs`.
- [x] May edit `executor/src/config.rs` (the `PrivacyConfig` struct and its
      doc comments only), `executor/src/privacy/egress.rs`,
      `executor/src/privacy/redact.rs`, `rexymcp.toml.example` and
      `docs/privacy.md`.
- [x] May edit `mcp/src/runner.rs`: the `build_egress_index` destructure, the
      `RedactingAiClient` construction, and `prescan_refusal`'s message.
- [x] May change existing tests only as named above: the live test's
      destructure, and one added assertion in
      `prescan_refusal_redacts_address_and_names_remedies`.
- [ ] May add a dependency — **no.** `regex` and `serde_json` are already
      executor dependencies.
- [ ] May change `redact_pii` — **no.**
- [ ] May touch `docs/architecture.md` or `privacy.kinds` — **no.**

## Out of scope

- **Reversibility at the executor boundary.** Nothing asks the model to
  restore a term (F03 decision).
- **A CLI that masks a file on demand.** A later phase, if wanted.
- **Masking the architect's return path.** `scrub_phase_result` handles
  structured PII only.
- **Masking terms in the session log.** That is phase 02.
- **`\r\n` handling in wrapped aliases.** Only `\n` is re-emitted.

## Update Log

(Filled in by the executor. See WORKFLOW.md § "Update Log entries".)

<!-- entries appended below this line -->

### Update — 2026-09-16 (blocked, architect)

First dispatch (run `96d43940`) ended `budget_exceeded` at 200 turns with no
tests run. The pre-scan redacted the project name everywhere (F05 finding 9),
and the executor then searched outside the repo, deleted rexymcp logs and vault
keys (finding 10). Partial work was left uncommitted in the tree:
`executor/src/privacy/terms.rs` (untracked), plus +4 lines in
`executor/src/config.rs` and +1 line in `executor/src/privacy/mod.rs`. Blocked
until findings 9 and 10 are fixed; no cloud re-dispatch before then.

### Update — 2026-09-17 (unblocked, architect)

Findings 9 and 10 are fixed, so this phase is dispatchable again. Since the
failed first run: the project's own name is no longer redacted (phase 08, so
the executor sees real paths and crate names this time), the executor's `bash`
runs inside bwrap on a cloud endpoint, `.git/` and `rexymcp.toml` are
protected, and a cut-off PII scan reply can no longer pass as "no PII".

The first run's partial work is **not** in the tree: `terms.rs` does not exist
and `config.rs` / `privacy/mod.rs` carry none of it. This is a fresh start, not
a resume.

**Dispatch on a LOCAL executor.** Nothing here needs a cloud model, and the
first cloud run of any real phase should not be this one.

### Update — ts=1789679554315 (complete, server-authored)

**Summary:** All acceptance criteria are met and committed. Summary for review:

**Built:** `[privacy] terms_file` — a JSON file of literal terms (site codes, acronyms, product names) that must never reach a cloud model. `LiteralTerms` (`executor/src/privacy/terms.rs`) compiles every alias into one regex with leftmost-first, longest-alias-wins alternation and per-group `(?i)` flags; `build_egress_index` now returns `EgressIndex { terms, pii_files, literal }` and loads the term file before the NER engine so a bad file fails even when the engine is misconfigured; `RedactingAiClient` masks literals before `redact_pii` on the system prompt, messages, and tool results; `prescan_refusal` names `privacy.terms_file`.

**Deviations:** one — the phase doc's matcher sketch puts `(?i)` inside each group, but a case-sensitive entry (`match` present) needs the flag absent for just that group, so the flag rides per-group in the compiled alternation (exactly as the sketch's "applies only to that group" note intends). No other deviation; the spec's "no alias may be empty" was enforced for both `""` and whitespace-only.

**E2E:** ran the architect's harness with a localhost capture server. With `terms_file`: `site_code=about [SITE_1]`, `email_marker=REDACTED:email` (positive control — proves `body.txt` holds the redacted request), `plaintext_hits=0`. Without `terms_file`: `plaintext_hits=1`, no `[SITE_1]`. Non-zero `exit=1` as expected (capture server answers 400).

**Gates:** `cargo fmt --all --check`, `cargo build` (zero warnings), `cargo clippy --all-targets --all-features -- -D warnings`, and `cargo test` (1188 passed, 10 ignored) all clean. Working tree is clean; commit `5c7010c` includes all 8 files.

**Executor:** RedHatAI/Qwen3.8-27B-INT4

**Gates:** format=run, build=run, lint=run, test=run

**Command output tails:**

```
FORMAT


BUILD
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.55s


LINT
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.15s


TEST
onexistent_path ... ok
test tools::symbols::tests::rejects_path_outside_root ... ok
test tools::symbols::tests::references_exclude_strings_and_comments ... ok
test tools::symbols::tests::single_file_unsupported_extension_advisory_error ... ok
test tools::symbols::tests::type_mismatch_returns_recovery_hint ... ok
test tools::symbols::tests::references_single_file_path ... ok
test tools::update_task::tests::flips_active_task_to_done ... ok
test tools::update_task::tests::flips_pending_task_to_active ... ok
test tools::update_task::tests::invalid_args_hint_lists_incomplete_ids ... ok
test tools::update_task::tests::invalid_args_hint_reports_all_complete ... ok
test tools::update_task::tests::invalid_state_returns_advisory_error ... ok
test tools::update_task::tests::malformed_args_returns_advisory_error ... ok
test tools::update_task::tests::metadata_shape_is_unchanged ... ok
test tools::update_task::tests::null_args_returns_recovery_hint ... ok
test tools::symbols::tests::references_no_matches_advisory ... ok
test tools::update_task::tests::result_flags_redundant_remark ... ok
test tools::update_task::tests::result_lists_remaining_incomplete_ids ... ok
test tools::update_task::tests::result_reports_all_complete_when_last_done ... ok
test tools::update_task::tests::success_output_names_task ... ok
test tools::update_task::tests::unknown_id_returns_advisory_error ... ok
test tools::symbols::tests::metadata_carries_definitions_and_files_count ... ok
test tools::write_file::tests::append_creates_file_if_missing ... ok
test tools::write_file::tests::append_false_overwrites ... ok
test tools::write_file::tests::missing_path_returns_recovery_hint ... ok
test tools::write_file::tests::creates_new_file ... ok
test tools::write_file::tests::appends_to_existing_file ... ok
test tools::symbols::tests::references_respects_max_results ... ok
test tools::write_file::tests::non_object_args_do_not_panic ... ok
test tools::write_file::tests::overwrites_existing_file ... ok
test tools::write_file::tests::rejects_malformed_args ... ok
test tools::write_file::tests::reports_missing_parent_dir ... ok
test tools::write_file::tests::scope_escape_returns_advisory_error_and_writes_nothing ... ok
test tools::write_file::tests::success_output_includes_line_count ... ok
test tools::symbols::tests::references_snippet_shows_source_line ... ok
test tools::symbols::tests::finds_rust_struct_and_trait ... ok
test tools::symbols::tests::references_truncation_note_omits_kind_filter ... ok
test tools::symbols::tests::unsupported_extension_skipped_in_dir_walk ... ok
test tools::symbols::tests::respects_gitignore ... ok
test tools::symbols::tests::reports_line_and_column ... ok
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

test result: ok. 1188 passed; 0 failed; 10 ignored; 0 measured; 0 filtered out; finished in 6.18s


running 0 tests

test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

    Finished `test` profile [unoptimized + debuginfo] target(s) in 0.17s
     Running unittests src/main.rs (target/debug/deps/rexymcp-3de3446ef9b6a3ce)
     Running tests/readme_config_reference.rs (target/debug/deps/readme_config_reference-22f887757c17cb2b)
     Running unittests src/lib.rs (target/debug/deps/executor-0c52bf72536f9e8e)
   Doc-tests executor

```

**Files changed:**

- `docs/privacy.md` — +22 -0
- `executor/src/config.rs` — +4 -0
- `executor/src/privacy/egress.rs` — +83 -13
- `executor/src/privacy/mod.rs` — +1 -0
- `executor/src/privacy/redact.rs` — +59 -4
- `executor/src/privacy/terms.rs` — +329 -0
- `mcp/src/runner.rs` — +13 -5
- `rexymcp.toml.example` — +1 -0

**Commit:** 5c7010c2ebe91fb7aff29eaedc7a4601553ef6e8

**Notes:** server-authored completion entry (executor no longer owns the bookkeeping tail; see M27 phase-03).

### Update — 2026-09-17 (review: bounced)

**Bug filed:** [bug-01-1](bugs/bug-01-1.md) (minor). The feature is correct and
verified: independent re-run of all four gates (716 + 2 + 1188, 10 ignored,
zero warnings), all 14 named tests present, and a mutation check — removing the
left `\b` from the alias pattern fails `default_entry_requires_both_boundaries`,
`embed_entry_matches_inside_an_identifier` and `loads_entries_and_flags`. The
pasted E2E shows `[SITE_1]` on the wire with `plaintext_hits=0`, against
`plaintext_hits=1` and no `[SITE_1]` without the terms file. One DoD box is
unmet: a new `expect()` in `LiteralTerms::mask`, a production path. The
`expect` at `mcp/src/runner.rs:466` is pre-existing (`b9f1c97`) and not this
phase's. See the Bounce section at the top.

