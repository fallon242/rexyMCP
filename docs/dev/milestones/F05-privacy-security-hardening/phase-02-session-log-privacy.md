# Phase 2: redact the session log, and make it owner-only

**Milestone:** F05 — Privacy and security hardening
**Status:** todo
**Depends on:** phase 01 (done) — `LiteralTerms` and `redact_pii` are what this
phase reuses.
**Estimated diff:** ~300 lines, about half of it tests
**Tags:** language=rust, kind=security, size=m

> **Dispatch on a LOCAL executor only.** This phase changes privacy plumbing.

## Goal

`.rexymcp/sessions/*.jsonl` records every prompt, completion and tool result.
Redaction today protects only the **wire** to a cloud model: `RedactingAiClient`
masks literal terms and PII on the way out, while the session log gets the same
content in the clear. In the deployment, five previews from the turns that read
a private extract were stored raw (F05 README, finding 2). The directory is
`0755` and the files `0644`, so every account on the host can read them.

After this phase, **when executor-egress redaction is engaged for a dispatch**:

1. Every string in every session record goes through the same two steps as the
   wire: literal terms first (`LiteralTerms::mask`), then `redact_pii`.
2. The secret `Redactor` keeps running exactly as it does today, in addition.
3. `.rexymcp/sessions/` is created `0700` and each log file `0600`, on unix. An
   existing directory or file is tightened on open.

With redaction off — a local executor, or `[privacy]` disabled — the log
content is byte-identical to today's. Only the permissions change.

### Why the scrub walks parsed JSON, not the serialized string

`redact_event` currently redacts the **serialized JSON**, which is right for
secret patterns but wrong for literal terms: an alias that spans a line break is
`"at Plant\nNine"` in the record, and in serialized JSON that newline is the two
characters `\` and `n`, so the wrapped-alias pattern phase 01 built will not
match it. Walking the parsed value and scrubbing each string leaf sees the real
newline and masks it.

## Architecture references

- `executor/src/agent/log.rs` — the whole file (50 lines): `log_event`,
  `log_session_end`, `redact_event`. No test module.
- `executor/src/agent/mod.rs:238-244` — where the loop builds its `Redactor`
  and opens the log. `LoopDeps` is at line 100, with `pii_files` at line 151 as
  the precedent for a privacy field.
- `executor/src/store/sessions/jsonl.rs:31-52` — `SessionLogger::open` and
  `log`.
- `executor/src/privacy/redact.rs` — `redact_pii` (line 18) and
  `RedactingAiClient`, whose `scrub` step this phase mirrors:
  `redact_pii(&self.literal.mask(&msg.content), &self.terms)` (line 77).
- `mcp/src/runner.rs:428-460` — where `egress_terms` and `literal` are built
  and handed to `RedactingAiClient`.

## Pre-flight

1. `git status --short` is clean. If it is not, stop and file a blocker.
2. `cargo test -p rexymcp-executor` passes. Record the count (expect 1188).
3. Read `executor/src/agent/log.rs` in full, plus
   `executor/src/store/sessions/jsonl.rs` lines 1-55.

## Current state

```rust
// executor/src/agent/log.rs:5-16 and 44-50
pub(super) fn log_event(
    handle: &Option<SessionLogHandle>,
    redactor: &Redactor,
    clock: &dyn Fn() -> u64,
    turn: usize,
    event: SessionEvent,
) {
    let Some(handle) = handle else {
        return;
    };
    session_log(handle, clock(), turn, redact_event(redactor, event));
}

fn redact_event(redactor: &Redactor, event: SessionEvent) -> SessionEvent {
    let Ok(json) = serde_json::to_string(&event) else {
        return event;
    };
    let redacted = redactor.redact(&json);
    serde_json::from_str(&redacted).unwrap_or(event)
}
```

```rust
// executor/src/store/sessions/jsonl.rs:32-40
    pub fn open(log_dir: &Path, session_id: &str) -> std::io::Result<Self> {
        std::fs::create_dir_all(log_dir)?;
        let path = log_dir.join(format!("session-{session_id}.jsonl"));
        let file = OpenOptions::new().create(true).append(true).open(&path)?;
```

```rust
// executor/src/agent/mod.rs:238-241
    let redactor = Redactor::new();
    let log_dir = deps.project_root.join(".rexymcp").join("sessions");
    let log_handle: Option<SessionLogHandle> =
        open_session_log(&log_dir, &format!("{}-{}", input.phase, deps.session_id)).ok();
```

## Spec

### 1. `EgressScrub` (privacy/redact.rs)

```rust
/// The outbound scrub, as one value: literal terms first, then the PII
/// dictionary. This is what `RedactingAiClient` applies to the wire; the
/// session log applies the same thing (F05 finding 2).
#[derive(Debug, Clone, Default)]
pub struct EgressScrub {
    literal: LiteralTerms,
    terms: Vec<(String, PiiKind)>,
}

impl EgressScrub {
    pub fn new(literal: LiteralTerms, terms: Vec<(String, PiiKind)>) -> Self;

    /// `text` with every literal alias masked as `[CODE]` and every dictionary
    /// term as `[REDACTED:kind]`.
    pub fn scrub(&self, text: &str) -> String {
        redact_pii(&self.literal.mask(text), &self.terms)
    }
}
```

`LiteralTerms` (`executor/src/privacy/terms.rs:33`) derives `Debug, Default`
today and holds `Option<regex::Regex>` plus `Vec<String>`, both `Clone`. Add
`Clone` to that derive — it is the one change this phase makes to `terms.rs`.

**Do not change `RedactingAiClient`.** It keeps its own fields and behavior; the
one duplicated line is deliberate, so a just-approved wire path is not disturbed.

### 2. `LogScrub` and the value walk (agent/log.rs)

```rust
/// What the session log scrubs each record with: the secret `Redactor` always,
/// plus the egress scrub when redaction is engaged for this dispatch.
pub(super) struct LogScrub {
    redactor: Redactor,
    egress: Option<EgressScrub>,
}

impl LogScrub {
    pub(super) fn new(redactor: Redactor, egress: Option<EgressScrub>) -> Self;
}

/// Apply `f` to every string leaf of `value`, in place.
fn scrub_strings(value: &mut serde_json::Value, f: &dyn Fn(&str) -> String) {
    match value {
        serde_json::Value::String(s) => *s = f(s),
        serde_json::Value::Array(items) => {
            for item in items {
                scrub_strings(item, f);
            }
        }
        serde_json::Value::Object(map) => {
            for (_, v) in map.iter_mut() {
                scrub_strings(v, f);
            }
        }
        _ => {}
    }
}
```

Change `log_event` and `log_session_end` to take `scrub: &LogScrub` instead of
`redactor: &Redactor`. **Do not rename the parameter at the call sites**: the
local in `mod.rs` is called `redactor` and the 24 `log_event` / 17
`log_session_end` calls all pass `&redactor`, so keeping that variable's name
means none of those 41 call sites change.

`redact_event` becomes:

```rust
fn redact_event(scrub: &LogScrub, event: SessionEvent) -> SessionEvent {
    let Ok(json) = serde_json::to_string(&event) else {
        return event;
    };
    let redacted = scrub.redactor.redact(&json);
    let Some(egress) = &scrub.egress else {
        return serde_json::from_str(&redacted).unwrap_or(event);
    };
    let Ok(mut value) = serde_json::from_str::<serde_json::Value>(&redacted) else {
        return event;
    };
    scrub_strings(&mut value, &|s| egress.scrub(s));
    serde_json::from_value(value).unwrap_or(event)
}
```

Keep the existing doc comment's meaning and extend it: the secret pass runs on
the serialized form, the egress pass on the parsed values.

### 3. Carry the scrub into the loop (agent/mod.rs, mcp/src/runner.rs)

- Add to `LoopDeps`, next to `pii_files`:

  ```rust
      /// F05 finding 2: the egress scrub for session-log records, `Some` exactly
      /// when executor-egress redaction is engaged for this dispatch. `None`
      /// leaves the log byte-identical to an unredacted run.
      pub egress_scrub: Option<crate::privacy::redact::EgressScrub>,
  ```

- At `mod.rs:238`, keep the variable name:

  ```rust
      let redactor = LogScrub::new(Redactor::new(), deps.egress_scrub.clone());
  ```

- In `mcp/src/runner.rs`, where `egress_terms` and `literal` are built
  (lines 428-441), build the scrub from the same two values **before** they are
  moved into `RedactingAiClient`, and put it in the `LoopDeps` literal:

  ```rust
      let egress_scrub = if redact {
          Some(rexymcp_executor::privacy::redact::EgressScrub::new(
              literal.clone(),
              egress_terms.clone(),
          ))
      } else {
          None
      };
  ```

  `redact` is the existing boolean at `mcp/src/runner.rs:402` that decides
  whether the client is wrapped. Use the same condition, so the log and the wire
  are never out of step.

- **Every `LoopDeps { .. }` literal in the test modules gets
  `egress_scrub: None,`.** There are **21** in `executor/src/agent/tests.rs` and
  **1** in `mcp/src/runner.rs`. `cargo build` names each one. That is the only
  change allowed to existing tests.

### 4. Owner-only permissions (store/sessions/jsonl.rs)

In `SessionLogger::open`, after `create_dir_all` and after the file is opened,
tighten both. Unix only:

```rust
#[cfg(unix)]
fn set_mode(path: &Path, mode: u32) {
    use std::os::unix::fs::PermissionsExt;
    let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode));
}

#[cfg(not(unix))]
fn set_mode(_path: &Path, _mode: u32) {}
```

`open` calls `set_mode(log_dir, 0o700)` and `set_mode(&path, 0o600)`. Setting
them on every open (not only on create) tightens a directory or file left over
from an earlier version. Failures are ignored on purpose: the existing contract
says session logging must never affect loop behavior.

### 5. Document it

In `docs/privacy.md`, at the end of the "② Executor egress" bullet, add one
sentence: when egress redaction is engaged, the session log under
`.rexymcp/sessions/` gets the same masking as the wire, and the directory and
its files are created owner-only (`0700`/`0600`).

## Acceptance criteria

- [ ] With an `EgressScrub`, a record's strings are masked the same way as the
      wire: literal terms as `[CODE]`, dictionary terms as `[REDACTED:kind]`.
- [ ] A literal alias wrapped across a newline is masked in the log. (This is
      what the value walk buys; a serialized-string pass misses it.)
- [ ] Without an `EgressScrub`, `redact_event` output is unchanged from today's,
      including the secret redaction.
- [ ] `.rexymcp/sessions/` is `0700` and each session file `0600` after
      `SessionLogger::open`, including when they already existed with looser
      modes.
- [ ] The loop passes the scrub through, so a run with redaction engaged writes
      a masked log.
- [ ] End-to-end (below): the session file holds `[SITE_1]`, no `Plant Nine`,
      and modes `700` / `600`.
- [ ] Each new non-ignored test fails when its fix is reverted (show one in the
      Update Log).
- [ ] `cargo fmt --all --check`, `cargo build` (zero warnings),
      `cargo clippy --all-targets --all-features -- -D warnings` and
      `cargo test` all pass.

## Test plan

Hermetic: `tempfile::TempDir` for every path, no network, no real model.

### `executor/src/agent/log.rs` (create a `#[cfg(test)] mod tests` — the file has none)

1. **`scrub_strings_reaches_every_leaf`** — a `serde_json::json!` value with a
   nested object, an array of strings and a number. After
   `scrub_strings(&mut v, &|s| s.replace("x", "y"))`, every string leaf is
   replaced and the number is untouched.
2. **`redact_event_masks_terms_and_literals`** — build
   `SessionEvent::ToolResult { name: "read_file", succeeded: true,
   output_preview: "owner Alice at Plant Nine".into(), output_bytes: 25 }` and a
   `LogScrub` whose `EgressScrub` has one literal entry (`SITE_1` ⇄
   `Plant Nine`) and terms `vec![("Alice".to_string(), PiiKind::PersonName)]`.
   The returned event's `output_preview` contains `[SITE_1]` and
   `[REDACTED:person]`, and contains neither `Alice` nor `Plant Nine`.
   Build the `LiteralTerms` with `LiteralTerms::load` on a `TempDir` file, the
   way `executor/src/privacy/terms.rs`'s tests do.
3. **`redact_event_masks_an_alias_across_a_newline`** — same scrub, with
   `SessionEvent::Prompt { rendered: "at Plant\nNine today".into() }`. The
   result contains `[SITE_1]` and not `Plant`. **This is the test that fails if
   the scrub is applied to the serialized JSON instead of the parsed value.**
4. **`redact_event_without_egress_is_unchanged`** — a `LogScrub` with
   `egress: None`. A `Completion { raw: "owner Alice" }` comes back with
   `raw == "owner Alice"`, and a raw string holding an obvious secret
   (`security::redact` matches `sk-[A-Za-z0-9_-]{20,}`, so
   `format!("sk-{}", "a".repeat(32))` is redacted) still comes back redacted,
   proving the secret pass still runs.

### `executor/src/store/sessions/jsonl.rs`

5. **`session_dir_and_file_are_owner_only`** — `#[cfg(unix)]`. Open a logger in
   a fresh `TempDir` subdirectory; assert `mode & 0o777` is `0o700` for the
   directory and `0o600` for the file.
6. **`existing_loose_modes_are_tightened`** — `#[cfg(unix)]`. Create the
   directory `0o755` and a `session-<id>.jsonl` file `0o644` first, then open
   the logger on the same id. Both are `0o700` / `0o600` afterwards, and a
   record written after the reopen still appears in `read_session_log`.

### `executor/src/agent/tests.rs`

7. **`session_log_is_scrubbed_when_egress_is_engaged`** — drive the loop the way
   the existing tests do (find the helper that builds `LoopDeps` and runs a
   scripted `MockAiClient`; `run_full` in that file is the pattern). Give
   `egress_scrub: Some(EgressScrub::new(LiteralTerms::default(), vec![("Alice".into(), PiiKind::PersonName)]))`
   and a phase input whose text contains `Alice`. After the run, read the
   session log with
   `crate::store::sessions::jsonl::read_session_log(path)`, where `path` comes
   from `PhaseResult.log_path` (`executor/src/phase/result.rs:66`, an
   `Option<PathBuf>`) — and assert no record's JSON contains
   `Alice`, while at least one contains `[REDACTED:person]`.
   If threading the path is awkward, read the single `*.jsonl` file under
   `<project_root>/.rexymcp/sessions/` instead.

## End-to-end verification

This is the phase-01 harness with three extra checks. The capture server
answers 400, so the loop never gets a completion — but the `Prompt` record is
written at turn 0 and carries the phase-doc text, which is what these checks
read. Run it as **one** bash command and paste the output:

```bash
T=$(mktemp -d)
cat > "$T/capture.py" <<'PYEOF'
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
PYEOF
mkdir -p "$T/repo"
printf '# Phase 1: e2e\n\n**Status:** todo\n\n## Goal\n\nMail ops@example.com about Plant Nine.\n' > "$T/phase-01-e2e.md"
printf '{"entries":[{"code":"SITE_1","aliases":["Plant Nine"]}]}\n' > "$T/repo/terms.json"
cat > "$T/repo/rexymcp.toml" <<'EOF'
[executor]
provider = "openai"
model = "m"
base_url = "http://127.0.0.1:18766/v1"

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
python3 "$T/capture.py" 18766 "$T/body.txt" & P=$!
timeout 120 cargo run -q -p rexymcp -- run-phase --no-telemetry \
  --config "$T/repo/rexymcp.toml" --phase-doc "$T/phase-01-e2e.md" --repo "$T/repo" \
  > "$T/run.txt" 2>&1; echo "exit=$?"
kill $P
LOG=$(ls "$T/repo/.rexymcp/sessions/"*.jsonl | head -1)
echo "dir_mode=$(stat -c %a "$T/repo/.rexymcp/sessions")"
echo "file_mode=$(stat -c %a "$LOG")"
echo "log_site_code=$(grep -c 'SITE_1' "$LOG")"
echo "log_plaintext=$(grep -c 'Plant Nine' "$LOG")"
echo "wire_plaintext=$(grep -c 'Plant Nine' "$T/body.txt")"
```

Expected: `dir_mode=700`, `file_mode=600`, `log_site_code` non-zero,
`log_plaintext=0`, `wire_plaintext=0`. A non-zero `exit=` is expected (the
capture server answers 400).

**Positive control — run it a second time with the `terms_file = "terms.json"`
line removed**, and paste that output too. Then `log_plaintext` must be
**non-zero**: it proves the check reads a log that really does hold the text
when nothing masks it. Without that second run the first one proves nothing,
because an empty or missing log would also print `0`.

## Authorizations

- [x] May edit `executor/src/agent/log.rs`, `executor/src/agent/mod.rs` (the
      `LoopDeps` field and the `redactor` local), `executor/src/privacy/redact.rs`
      (the new `EgressScrub` only), and
      `executor/src/store/sessions/jsonl.rs`, including their test modules.
- [x] May add `#[derive(Clone)]` to `LiteralTerms` if it lacks one.
- [x] May edit `mcp/src/runner.rs` to build the scrub and pass it.
- [x] May add `egress_scrub: None,` to the 21 + 1 existing `LoopDeps` literals.
      No other change to existing tests.
- [x] May add the sentence in Spec §5 to `docs/privacy.md`.
- [ ] May add a dependency or a config key — **no.**
- [ ] May change `RedactingAiClient`, `redact_pii`, or `LiteralTerms::mask` —
      **no.** The wire path was approved in phase 01 and is not in scope.

## Out of scope

- **`.rexymcp/output/` recovery logs.** They hold full, unmasked tool output and
  are read back by the model, so masking them would corrupt the recovery read.
  Their permissions deserve the same treatment; file it as its own phase.
- **Logs written before this phase.** Existing files are tightened on open only
  if the same session id is reopened. Old logs keep their modes; the operator
  can `chmod` them.
- **Windows.** `set_mode` is a no-op there. rexymcp's sandbox is Linux-only
  already (phase 07).
- **The `Redactor`'s secret patterns.** Unchanged.

## Update Log

(Filled in by the executor. See WORKFLOW.md § "Update Log entries".)

<!-- entries appended below this line -->
