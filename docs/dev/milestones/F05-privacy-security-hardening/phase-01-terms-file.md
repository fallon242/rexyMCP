# Phase 1: `[privacy] terms_file`

**Milestone:** F05 — Privacy and security hardening
**Status:** todo
**Depends on:** phase 06 (done). A failed pre-scan now stops the dispatch, and
a term file that fails to load goes the same way.
**Estimated diff:** ~380 lines, about half of it tests
**Tags:** language=rust, kind=feature, size=m

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
