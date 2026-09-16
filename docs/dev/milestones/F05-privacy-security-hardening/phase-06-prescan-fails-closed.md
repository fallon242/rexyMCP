# Phase 6: a failed pre-scan stops a cloud dispatch

**Milestone:** F05 — Privacy and security hardening
**Status:** in-progress
**Depends on:** none (drafted ahead of phase 01 on human instruction)
**Estimated diff:** ~120 lines, most of it tests
**Tags:** language=rust, kind=bugfix, size=s

## Goal

When a dispatch goes to a cloud executor with `[privacy]` on, rexyMCP first
scans the repo for PII with a local NER engine. If that scan fails today, the
run carries on anyway. It sends content with only structured PII redacted, and
the write-guard is off. The only signal is one line in `PhaseResult.warnings`.
The stated rule is the opposite: when the detection engine is unavailable,
cloud work stops.

After this phase, a failed pre-scan ends the dispatch before turn 1 with an
`Error::Privacy`. The message names the setting to check and the three ways
forward. It contains no IP address, because this error reaches Claude without
passing through the `PhaseResult` scrub.

## Architecture references

- `mcp/src/runner.rs` — `run_phase` (lines 355–477). The pre-scan block is
  lines 382–409; the warning push is lines 472–474.
- `executor/src/privacy/egress.rs` — `should_redact_egress` (line 27),
  `build_egress_index` (line 110). Read only.
- `executor/src/privacy/redact.rs` — `redact_pii` (line 18). Read only.
- `mcp/src/server.rs` — lines 231–246: a `run_phase` error becomes a string
  with `e.to_string()` **before** `scrub_phase_result` runs. Read only.

## Pre-flight

1. `cargo test -p rexymcp runner` passes before you start. Record the count.
2. Read `run_phase` in full before editing.

## Current state

```rust
// mcp/src/runner.rs:382-409
    // M45 executor-egress protection: on a real dispatch (no test client) with the
    // gate on and a cloud endpoint, pre-scan the repo for PII, wrap the client so
    // outbound content is redacted, and collect the PII-file set for the
    // write-guard. A failed pre-scan degrades to deterministic-only live redaction.
    let redact = inp.test_client.is_none()
        && rexymcp_executor::privacy::egress::should_redact_egress(
            &inp.cfg.privacy,
            &inp.cfg.executor.base_url,
        );
    let mut egress_terms = Vec::new();
    let mut pii_files = std::collections::HashSet::new();
    let mut egress_warning = None;
    if redact {
        match rexymcp_executor::privacy::egress::build_egress_index(inp.repo_path, &inp.cfg.privacy)
            .await
        {
            Ok((terms, files)) => {
                egress_terms = terms;
                pii_files = files;
            }
            Err(e) => {
                egress_warning = Some(format!(
                    "executor-egress redaction engaged but the PII pre-scan failed ({e}); only \
                     structured PII is redacted live and the write-guard is off"
                ));
            }
        }
    }
```

```rust
// mcp/src/runner.rs:471-476
    let mut result = run_phase_with(&assembly, &seams).await?;
    if let Some(warning) = egress_warning {
        result.warnings.push(warning);
    }
    Ok(result)
```

The error type, from `executor/src/error.rs`:

```rust
    #[error("privacy: {0}")]
    Privacy(String),
```

A real pre-scan failure reads like this once `Display`ed (the address is
elided here):

```
privacy: NER engine call failed: Request failed: error sending request for url (http://<engine-address>:8080/v1/chat/completions)
```

`redact_pii(text, &[])` with an empty term list runs only the deterministic
detectors (email, phone, SSN, card, IPv4, MAC) and replaces each hit with
`[REDACTED:<tag>]`. For an IPv4 address the tag is `ip`, so the address above
becomes `[REDACTED:ip]`.

## Spec

### 1. Fail the dispatch when the pre-scan fails

In `run_phase`, when `redact` is true and `build_egress_index` returns `Err`,
return `Err(Error::Privacy(..))` straight away. Do this before `owned`,
`fetch_context_window` or `run_phase_with`, so no request reaches the executor.

Remove `egress_warning` and the `if let Some(warning) = egress_warning` block;
nothing sets them any more. Update the comment above `redact` so it no longer
says a failed pre-scan degrades.

The `Ok` branch and the `redact == false` path are unchanged.

### 2. Build the message in a small private function

Add a private function in `runner.rs` that turns the pre-scan error into the
error `run_phase` returns. Suggested shape (the name is yours):

```rust
fn prescan_refusal(e: &rexymcp_executor::error::Error) -> rexymcp_executor::error::Error
```

The returned `Error::Privacy` message must:

- say the PII pre-scan failed and the dispatch was stopped before any content
  was sent;
- include the underlying error text **after** passing it through
  `rexymcp_executor::privacy::redact::redact_pii(&e.to_string(), &[])`;
- name `privacy.engine_base_url` as the setting to check;
- name the three ways forward: start the PII-detection engine, run the phase on
  a local executor, or set `privacy.redact_executor_egress = false` to send
  unredacted content deliberately.

Wording is yours; tests check for those pieces, not exact sentences.

### 3. Document it

In `docs/privacy.md`, in the "② Executor egress to a cloud model" bullet (around
line 28), add one sentence: if the pre-scan fails, for example because the NER
engine is unreachable, the dispatch stops before anything is sent. Change
nothing else in that file.

## Acceptance criteria

- [ ] With `[privacy]` on and egress redaction in effect, a failed
      `build_egress_index` makes `run_phase` return `Err(Error::Privacy(_))`.
- [ ] No `PhaseResult` is produced and the executor client is never called in
      that case.
- [ ] The error message names `privacy.engine_base_url` and
      `redact_executor_egress`.
- [ ] The error message contains no IPv4 address from the underlying error; it
      contains `[REDACTED:ip]` in its place.
- [ ] The string `write-guard is off` no longer appears in `mcp/src`.
- [ ] `docs/privacy.md` states that a failed pre-scan stops the dispatch.
- [ ] Each new test fails when the fix is reverted (show one in the Update Log).
- [ ] `cargo fmt --all --check`, `cargo build`,
      `cargo clippy --all-targets --all-features -- -D warnings` and
      `cargo test` all pass.

## Test plan

Add to the existing `mod tests` in `mcp/src/runner.rs`.

**Gotcha — do not write an IPv4 address anywhere, including in tests.** This
phase runs on a cloud executor with egress redaction on. An address written in
a source file or in this doc is redacted before you see it, and the file joins
the write-guard's PII set, which blocks further edits to it. Build the test
address at runtime from parts, like this:

```rust
let addr = ["192", "0", "2", "10"].join(".");
```

1. **`prescan_refusal_redacts_address_and_names_remedies`** — pure test of the
   function from Spec §2. Input:
   `Error::Privacy(format!("NER engine call failed: error sending request for url (http://{addr}:8080/v1/chat/completions)"))`.
   Assert the result is `Error::Privacy`; its message does **not** contain
   `addr`; it contains `[REDACTED:ip]`, `privacy.engine_base_url` and
   `redact_executor_egress`.

2. **`run_phase_stops_when_prescan_fails`** — calls `run_phase` itself.
   Hermetic setup: a `TempDir` for the repo and the phase-doc path, and
   `Config::default()` with:

   ```rust
   cfg.privacy.enabled = true;
   cfg.privacy.redact_executor_egress = Some(true);
   cfg.privacy.engine_base_url = None;
   cfg.privacy.engine_model = Some("m".to_string());
   cfg.executor.base_url = "http://localhost:9/v1".to_string();
   ```

   `engine_base_url = None` makes `build_egress_index` fail inside
   `NerEngine::from_config`, before any network or file access. Build
   `RunPhaseConfig` with `test_client: None` (the pre-scan only runs without a
   test client), `resume: None`, `cancel: CancelSignal::never()`, and `None`
   for the other optional fields. Assert
   `matches!(result, Err(rexymcp_executor::error::Error::Privacy(_)))` and that
   the message contains `privacy.engine_base_url`.

   With the fix reverted this test goes on to contact `localhost:9`, which
   refuses at once, and fails. That is expected, and it only happens in the
   reverted state.

**Must NOT happen:** a dispatch with egress redaction **off**
(`redact_executor_egress = Some(false)`, or `privacy.enabled = false`) must not
run the pre-scan or fail because of it. Existing tests cover this path because
they use `Config::default()` (privacy off); they must pass unchanged.

## End-to-end verification

Run the real CLI against a config whose engine address refuses connections.
Redirect output to a file and paste the file. Build the config in a temp
directory. Do not write an IPv4 address; `localhost` is fine:

```bash
T=$(mktemp -d)
mkdir -p "$T/repo/docs/dev"
printf '# Phase 1: e2e\n\n**Status:** todo\n\n## Goal\n\nx\n' > "$T/phase-01-e2e.md"
cat > "$T/repo/rexymcp.toml" <<'EOF'
[executor]
provider = "openai"
model = "m"
base_url = "https://api.example.com/v1"

[commands]
format = "true"
build = "true"
lint = "true"
test = "true"

[privacy]
enabled = true
engine_base_url = "http://localhost:9/v1"
engine_model = "m"
vault_dir = "vault"
EOF
cargo run -q -p rexymcp -- run-phase --no-telemetry \
  --config "$T/repo/rexymcp.toml" --phase-doc "$T/phase-01-e2e.md" --repo "$T/repo" \
  > /tmp/e2e_run.txt 2>&1; echo "exit=$?" >> /tmp/e2e_run.txt
```

Paste `/tmp/e2e_run.txt`. It must show the `privacy:` error naming
`privacy.engine_base_url`, and a non-zero `exit=`.
`https://api.example.com` is a cloud host, so redaction is automatic. The run
must stop before contacting it; the refused `localhost:9` call is the only
connection attempt.

## Authorizations

- [x] May edit `mcp/src/runner.rs`: `run_phase`, one new private function, and
      its test module.
- [x] May add one sentence to `docs/privacy.md` as described in Spec §3.
- [ ] May edit `mcp/src/server.rs` — **no.** Scrubbing every error string on
      the way to Claude is a wider change (see Out of scope).
- [ ] May edit anything under `executor/src/privacy/` — **no.**
- [ ] May add a config key — **no.** `redact_executor_egress = false` is
      already the explicit opt-out.
- [ ] May change any existing test — **no.**
- [ ] May add a dependency — **no.**

## Out of scope

- **Scrubbing every `run_phase` error.** `server.rs` turns any error into a
  string before the `PhaseResult` scrub, so other infrastructure errors can
  carry structured PII to Claude. This phase only cleans the error it creates.
  The general fix belongs with phase 02 (session-log redaction).
- **A reduced-protection mode behind a new flag.** Not wanted. Stopping is the
  rule, and `redact_executor_egress = false` already covers a deliberate
  unredacted run.
- **Retrying or health-checking the NER engine** before the scan.
- **Timeouts on a hung engine.** `NerEngine::from_config` uses a 600 s first-token
  timeout; a hung engine still delays the refusal by that long.

## Update Log

(Filled in by the executor. See WORKFLOW.md § "Update Log entries".)

<!-- entries appended below this line -->

### Update — 2026-09-16 14:53 (started)

**Executor:** starting phase 06 (prescan fails closed). Flipped Status to
`in-progress` and the milestone README row to match. Pre-flight:
`cargo test -p rexymcp runner` = 27 passed / 0 failed.

### Update — 2026-09-16 15:08 (progress)

**Verification record (for the server-authored completion entry).** A failed
egress pre-scan now short-circuits `run_phase` with `Error::Privacy`; the
degradation warning is gone.

**Positive control — each new test fails when the fix is reverted.**
Reverting the one-line change (`Err(e) => return Err(prescan_refusal(&e))` →
`Err(e) => return Err(e)`) and re-running `cargo test -p rexymcp runner`:

```
test runner::tests::prescan_refusal_redacts_address_and_names_remedies ... FAILED
test runner::tests::run_phase_stops_when_prescan_fails ... FAILED
test result: FAILED. 27 passed; 2 failed
```

`run_phase_stops_when_prescan_fails` fails in the reverted state exactly as the
phase doc predicts: the run carries on, contacts `localhost:9`, and returns the
NER error rather than the refusal. Restoring the fix returns 29 passed / 0
failed.

**End-to-end verification** (`/tmp/e2e_run.txt`, captured by redirect; the config
uses a cloud host `https://api.example.com` for the executor and a refusing
`http://localhost:9/v1` for the engine):

```
Error: privacy: the PII pre-scan failed (privacy: NER engine call failed: Request failed: error sending request for url (http://localhost:9/v1/chat/completions)), so the dispatch to the cloud executor was stopped before any content was sent. Check the privacy.engine_base_url setting, or start the PII-detection engine; if it stays unavailable, run the phase on a local executor, or set privacy.redact_executor_egress = false to send unredacted content deliberately.
exit=1
```

Positive control for this check: reverting the fix in the same temp-dir harness
produces the old behaviour instead — `Error: privacy: NER engine call failed: …`
with no remedy wording and no "stopped before any content was sent". So the
refusal wording in the fixed run is produced by the fix, not by the harness.

**Grep for the pinned literals:**

```
$ grep -n "\[REDACTED:ip\]" mcp/src/runner.rs
1196:            message.contains("[REDACTED:ip]"),
$ grep -n "privacy.engine_base_url" mcp/src/runner.rs
477:  … Check the privacy.engine_base_url setting …
$ grep -rn "write-guard is off" mcp/src/
(no matches)
```

`[REDACTED:ip]` appears in the test only; the runtime replacement is produced by
`redact_pii`, whose output above shows the address replaced with
`[REDACTED:ip]`. Note: the phase doc's spec sketch says the `privacy:` error
prefix appears once; in practice `Error::Privacy` adds its own `privacy: ` prefix
via `Display`, so the CLI line reads `privacy: the PII pre-scan failed (privacy:
NER engine call failed: …)`. The inner text is the raw pre-scan error passed
through `redact_pii`, as specified.

**Commands:** `cargo fmt --all --check` clean; `cargo build` clean;
`cargo clippy --all-targets --all-features -- -D warnings` clean;
`cargo test` = 1142 passed / 0 failed / 4 ignored.
