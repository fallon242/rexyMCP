# Phase 10: run gate, hook and verifier commands in the sandbox

**Milestone:** F05 — Privacy and security hardening
**Status:** review
**Depends on:** phase 07 (done)
**Estimated diff:** ~400 lines, about half of it tests
**Tags:** language=rust, kind=security, size=m

> **Dispatch on a LOCAL executor only.** This phase fixes part of the hole that
> stops all cloud dispatch (finding 11). Do not send it to a cloud endpoint.

## Goal

Phase 07 put the model's `bash` tool inside a bubblewrap (`bwrap`) sandbox when
the executor is a cloud endpoint. rexymcp itself still runs three kinds of
command on the host, with the full environment, and those commands build and
run code the model wrote:

- the gate commands (`format`, `build`, `lint`, `test`) and the post-write
  hooks (`format_fix`, `lint_fix`), through `RealCommandRunner`;
- the per-write verifier (`cargo check`, `tsc`, `ruff check`), through
  `governor::verifier`.

A failing test can print `$HOME/.config/...` or an API key, and the gate
feedback sends that text to the cloud model (F05 README, finding 11).

After this phase, when the dispatch has a sandbox (a cloud endpoint):

1. Every gate and hook command runs inside the same sandbox as `bash`, with the
   same environment allowlist.
2. The verifier's `cargo`/`tsc`/`ruff` run inside the sandbox too, with the
   same allowlist.
3. rexymcp's own bookkeeping git commands (`finalize`) keep running on the host,
   through a separate `host_runner` seam. Inside the sandbox, `~/.gitconfig` is
   hidden, so a sandboxed `git commit` has no author identity.

A local executor is unchanged: host runner, host verifier, inherited
environment.

Protecting `.git/` and `rexymcp.toml` from the model is **phase 11**, not this
phase.

## Architecture references

- `executor/src/security/sandbox.rs` — `Sandbox::argv` (lines 39–106). Read in
  full.
- `executor/src/agent/command.rs` — `CommandRunner` trait (lines 23–25),
  `RealCommandRunner` (lines 28–57).
- `executor/src/agent/verify.rs` — `FileVerifier` trait and `RealVerifier`
  (the whole file, 32 lines).
- `executor/src/governor/verifier.rs` — `capture_baseline` (line 152),
  `verify` (line 224), `spawn_failure` (line 242), the three spawns at lines
  264 (`cargo`), 497 (`tsc`), 589 (`ruff`).
- `executor/src/tools/bash.rs` — `is_allowed_env_key` (exported as
  `crate::tools::is_allowed_env_key`), and the env loop in `execute`.
- `mcp/src/runner.rs` — `Seams` (lines 97–107), `run_phase_with`'s
  `FinalizeInput` (line 328), the sandbox block in `run_phase` (lines
  404–424), `verifier`/`runner` locals and the `Seams` literal (lines
  464–478), `NoopRunner`/`NoopVerifier` in tests (lines 543–567).
- `docs/privacy.md` — the "② Executor egress" bullet, which phase 07 extended.

## Pre-flight

1. `git status --short` is clean. If it is not, stop and file a blocker.
2. `bwrap --version` prints a version. Record it.
3. `cargo test -p rexymcp-executor verifier` and `cargo test -p rexymcp runner`
   pass. Record the counts.
4. Read `executor/src/security/sandbox.rs`, `executor/src/agent/command.rs`
   lines 1–60, `executor/src/agent/verify.rs`, and
   `executor/src/governor/verifier.rs` lines 140–300 and 480–610 in full before
   editing.

## Current state

```rust
// executor/src/agent/command.rs:27-57
/// Production runner: `sh -c <command>` via `tokio::process`, stdout then stderr.
pub struct RealCommandRunner;

#[async_trait]
impl CommandRunner for RealCommandRunner {
    async fn run(&self, command: &str, cwd: &Path) -> CommandResult {
        match tokio::process::Command::new("sh")
            .arg("-c")
            .arg(command)
            .current_dir(cwd)
            .output()
            .await
        {
            Ok(out) => {
                let mut combined = String::from_utf8_lossy(&out.stdout).into_owned();
                let stderr = String::from_utf8_lossy(&out.stderr);
                if !stderr.is_empty() {
                    combined.push_str(&stderr);
                }
                CommandResult {
                    output: combined,
                    success: out.status.success(),
                }
            }
            Err(e) => CommandResult {
                output: format!("failed to run `{command}`: {e}"),
                success: false,
            },
        }
    }
}
```

```rust
// executor/src/governor/verifier.rs:264-272 (the cargo spawn; tsc at 497 and
// ruff at 589 have the same shape)
    let output = match Command::new("cargo")
        .arg("check")
        .arg("--message-format=json")
        .current_dir(&crate_root)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .await
```

```rust
// executor/src/security/sandbox.rs:98-105 (end of Sandbox::argv)
        // 10-11.
        a.push("--chdir".into());
        a.push(p(&root));
        a.push("sh".into());
        a.push("-c".into());
        a.push(command.to_string());

        a
```

```rust
// mcp/src/runner.rs:464-478 (run_phase)
    let verifier = rexymcp_executor::agent::verify::RealVerifier;
    let runner = rexymcp_executor::agent::command::RealCommandRunner;
    ...
    let seams = Seams {
        client,
        verifier: &verifier,
        runner: &runner,
        clock: &clock,
        pii_files,
        sandbox,
    };
```

`run_phase_with` passes `seams.runner` both to the agent loop
(`runner: seams.runner`, line 311) and to finalize
(`FinalizeInput { runner: seams.runner, .. }`, line 333).

## Spec

### 1. `Sandbox::command_prefix`

In `executor/src/security/sandbox.rs`, split `argv` into two parts. **Do not
change the order or content of any mount.** Phase 07's tests pin them.

```rust
/// Everything up to and including `--chdir <chdir>`: element 0 is `program`,
/// then rows 1-9 of the mount table, then `--chdir`, `chdir`. Append a program
/// and its arguments to run it in the sandbox.
pub fn command_prefix(&self, chdir: &Path) -> Vec<String>;

/// `command_prefix(root)` followed by `"sh"`, `"-c"`, `command`.
pub fn argv(&self, command: &str) -> Vec<String>;
```

`argv` must return exactly what it returns today. The existing `argv_*` tests
must pass unchanged.

### 2. `SandboxedCommandRunner`

In `executor/src/agent/command.rs`:

- Move the `Ok(out)` / `Err(e)` match in `RealCommandRunner::run` into a
  private function
  `fn to_result(command: &str, out: std::io::Result<std::process::Output>) -> CommandResult`
  and call it from `RealCommandRunner`. `RealCommandRunner`'s behavior does not
  change.
- Add:

```rust
/// Cloud-executor runner: the same `sh -c <command>`, run inside the bash
/// sandbox with the bash tool's environment allowlist.
pub struct SandboxedCommandRunner {
    pub sandbox: crate::security::Sandbox,
}

#[async_trait]
impl CommandRunner for SandboxedCommandRunner {
    async fn run(&self, command: &str, cwd: &Path) -> CommandResult {
        let mut argv = self.sandbox.command_prefix(cwd);
        argv.push("sh".to_string());
        argv.push("-c".to_string());
        argv.push(command.to_string());
        let Some((program, args)) = argv.split_first() else {
            return CommandResult {
                output: "internal error: sandbox argv is empty".to_string(),
                success: false,
            };
        };
        let mut cmd = tokio::process::Command::new(program);
        cmd.args(args).current_dir(cwd).env_clear();
        for (key, value) in std::env::vars() {
            if crate::tools::is_allowed_env_key(&key) {
                cmd.env(&key, &value);
            }
        }
        to_result(command, cmd.output().await)
    }
}
```

`cwd` is always inside the repo (the loop passes the project root), so
`--chdir cwd` is valid inside the sandbox.

### 3. The verifier can run in the sandbox

In `executor/src/governor/verifier.rs`:

- Add `pub async fn verify_in(path: &Path, sandbox: Option<&Sandbox>) -> VerifierResult`
  and `pub async fn capture_baseline_in(paths: &[PathBuf], sandbox: Option<&Sandbox>) -> Baseline`.
  Move the bodies of `verify` and `capture_baseline` into them.
  `capture_baseline_in` calls `verify_in(path, sandbox)`. Keep `verify(path)`
  and `capture_baseline(paths)` as one-line wrappers that pass `None`, so
  existing callers and tests do not change.
- `verify_in` passes `sandbox` to `verify_rust`, `verify_typescript` and
  `verify_python` (add a `sandbox: Option<&Sandbox>` parameter to each).
- Add one helper and use it at all three spawn sites:

```rust
/// A `Command` for `program`, run in `cwd`. With a sandbox: `bwrap <prefix>
/// --chdir <cwd> <program>`, with the environment cleared to the bash
/// allowlist. The caller appends the checker's own arguments.
fn checker_command(sandbox: Option<&Sandbox>, program: &str, cwd: &Path) -> Command {
    match sandbox {
        None => {
            let mut c = Command::new(program);
            c.current_dir(cwd);
            c
        }
        Some(sb) => {
            let mut argv = sb.command_prefix(cwd);
            argv.push(program.to_string());
            // argv always has at least the program, so this never falls back.
            let (first, rest) = argv.split_first().map_or((program, &[][..]), |(f, r)| (f.as_str(), r));
            let mut c = Command::new(first);
            c.args(rest).current_dir(cwd).env_clear();
            for (key, value) in std::env::vars() {
                if crate::tools::is_allowed_env_key(&key) {
                    c.env(&key, &value);
                }
            }
            c
        }
    }
}
```

  The `map_or` line is a suggestion. Any form without `unwrap`/`expect`/
  indexing is fine.

  - `cargo`: `checker_command(sandbox, "cargo", &crate_root)`, then
    `.arg("check").arg("--message-format=json")`.
  - `tsc`: `checker_command(sandbox, &cmd.program, &project_root)`, then
    `.args(cmd.prefix_args).arg("--noEmit").arg("--pretty=false")`.
  - `ruff`: today it sets **no** `current_dir`. Use the directory that
    contains `target`: `target` itself if it is a directory, otherwise its
    parent. `path_str` is absolute, so the result is the same.

  Keep `.stdout(Stdio::piped()).stderr(Stdio::piped()).output().await` and
  the `spawn_failure` mapping as they are.

- **A checker that is missing inside the sandbox.** bwrap does not fail to
  spawn. It exits 1 and prints this to stderr (checked by hand, bubblewrap
  0.12.0):

  ```
  bwrap: execvp rexymcp-no-such-tool: No such file or directory
  ```

  Without handling, stdout is empty and the verifier would report
  `Checked { diagnostics: [] }`, a silent pass. Add:

  ```rust
  /// True when a sandboxed checker never started: bwrap exits non-zero and
  /// reports the failed `execvp` on stderr.
  fn sandbox_exec_failed(sandboxed: bool, success: bool, stderr: &[u8]) -> bool {
      sandboxed && !success && stderr.starts_with(b"bwrap: execvp ")
  }
  ```

  At each spawn site, after `output` is obtained, if
  `sandbox_exec_failed(sandbox.is_some(), output.status.success(), &output.stderr)`,
  return
  `VerifierResult::Skipped(format!("{tool} is not available inside the bash sandbox; incremental verification is disabled this run"))`,
  where `{tool}` is `cargo`, `tsc` or `ruff`.

In `executor/src/agent/verify.rs`, add:

```rust
/// Cloud-executor verifier: the same checks, run inside the bash sandbox.
pub struct SandboxedVerifier {
    pub sandbox: crate::security::Sandbox,
}

#[async_trait]
impl FileVerifier for SandboxedVerifier {
    async fn verify(&self, path: &Path) -> VerifierResult {
        verifier::verify_in(path, Some(&self.sandbox)).await
    }

    async fn capture_baseline(&self, paths: &[PathBuf]) -> Baseline {
        verifier::capture_baseline_in(paths, Some(&self.sandbox)).await
    }
}
```

### 4. `run_phase` picks the runner and verifier

In `mcp/src/runner.rs`:

- Add a field to `Seams`, next to `runner`:

  ```rust
  /// Runs rexymcp's own bookkeeping commands (finalize's git calls) on the
  /// host, even when `runner` is sandboxed.
  host_runner: &'a dyn CommandRunner,
  ```

  In `run_phase_with`, `FinalizeInput { runner: seams.host_runner, .. }`. The
  agent loop keeps `runner: seams.runner`.

- Add, above `run_phase`:

  ```rust
  /// The runner and verifier for one dispatch: the host ones for a local
  /// endpoint, the sandboxed ones when `run_phase` built a sandbox.
  enum ExecTools {
      Host(
          rexymcp_executor::agent::verify::RealVerifier,
          rexymcp_executor::agent::command::RealCommandRunner,
      ),
      Sandboxed(
          rexymcp_executor::agent::verify::SandboxedVerifier,
          rexymcp_executor::agent::command::SandboxedCommandRunner,
      ),
  }

  impl ExecTools {
      fn new(sandbox: Option<rexymcp_executor::security::Sandbox>) -> Self;
      fn verifier(&self) -> &dyn FileVerifier;
      fn runner(&self) -> &dyn CommandRunner;
  }
  ```

  `new(None)` is `Host`. `new(Some(sb))` is `Sandboxed` with a clone of `sb` in
  each part.

- In `run_phase`, replace the `verifier`/`runner` locals with:

  ```rust
  let exec = ExecTools::new(sandbox.clone());
  let host_runner = rexymcp_executor::agent::command::RealCommandRunner;
  ```

  In the `Seams` literal, use `verifier: exec.verifier()`,
  `runner: exec.runner()` and `host_runner: &host_runner`. `sandbox` still
  goes into `Seams` for the bash tool.

- Every `Seams { .. }` literal in the test module gets
  `host_runner: &NoopRunner,`. That is the only change allowed to existing
  tests.
- `verifier.rs` needs `use crate::security::Sandbox;`.

### 5. Document it

In `docs/privacy.md`, in the "② Executor egress" bullet, directly after the
sentence phase 07 added that ends "...and the dispatch stops if the sandbox is
unavailable.", add one sentence: the gate, hook and verifier commands run in
the same sandbox, with the same environment allowlist. Change nothing else.

## Acceptance criteria

- [ ] `Sandbox::argv` output is unchanged (phase 07's `argv_*` tests pass
      unmodified), and `argv(c)` equals `command_prefix(root)` followed by
      `sh`, `-c`, `c`.
- [ ] With a sandbox, gate and hook commands run through
      `SandboxedCommandRunner`, and the verifier runs through
      `SandboxedVerifier`. Without one, `RealCommandRunner` and `RealVerifier`
      are used as before.
- [ ] Sandboxed commands get only allowlisted environment variables (ignored
      test, run in E2E).
- [ ] A checker missing inside the sandbox gives `VerifierResult::Skipped`,
      not `Checked` with no diagnostics.
- [ ] Finalize's git commands use `host_runner`.
- [ ] `docs/privacy.md` has the sentence from Spec §5.
- [ ] Each new non-ignored test fails when its fix is reverted (show one in the
      Update Log).
- [ ] `cargo fmt --all --check`, `cargo build`,
      `cargo clippy --all-targets --all-features -- -D warnings` and
      `cargo test` all pass.

## Test plan

Hermetic: a `tempfile::TempDir` for any repo, no network. A program name that
does not exist (`rexymcp-no-such-bwrap`) makes a sandbox fail without needing
bwrap. A sandbox whose program is `true` "runs" every command as a no-op that
exits 0, which also needs no bwrap.

### `executor/src/security/sandbox.rs`

1. **`argv_is_prefix_plus_shell`** — `Sandbox::new(Path::new("/srv/repo"), Some("/home/u".into()))`.
   `sb.argv("echo hi")` equals `sb.command_prefix(Path::new("/srv/repo"))`
   with `"sh"`, `"-c"`, `"echo hi"` appended.
2. **`command_prefix_ends_with_chdir`** — `command_prefix(Path::new("/srv/repo/sub"))`
   ends with `["--chdir", "/srv/repo/sub"]`, and element 0 is `"bwrap"`.

### `executor/src/agent/command.rs` (the existing `mod tests` at line 219)

3. **`sandboxed_runner_runs_through_the_sandbox_program`** — `SandboxedCommandRunner`
   with `Sandbox::new(dir, None).with_program("rexymcp-no-such-bwrap")`.
   `run("echo $((40+2))", dir)` → `success == false`, and `output` does not
   contain `42`. With the sandbox bypassed, `sh` prints `42`. (The error text
   quotes the command, so assert on `42`, not on the command text.)
4. **`sandboxed_runner_passes_only_allowlisted_env`** —
   `#[ignore = "needs bwrap and user namespaces"]`. Real sandbox:
   `Sandbox::new(&canonical_tempdir, std::env::var_os("HOME").map(PathBuf::from))`.
   `run("env", dir)` succeeds. Every `KEY=` line has a key for which
   `is_allowed_env_key(key)` is true, or the key is one `sh` sets itself:
   `PWD`, `OLDPWD`, `SHLVL`, `_`.
   Do not set environment variables in the test: `std::env::set_var` is
   `unsafe` in edition 2024.

### `executor/src/governor/verifier_tests.rs` (the verifier's test module, included with `#[path]`)

5. **`sandbox_exec_failure_is_detected`** — pure.
   `sandbox_exec_failed(true, false, b"bwrap: execvp ruff: No such file or directory\n")`
   is true.
   **Must be false:** `(false, false, <same stderr>)`,
   `(true, true, <same stderr>)`, and
   `(true, false, b"error[E0425]: cannot find value")`.
6. **`verify_in_uses_the_sandbox_program`** — a `TempDir` with a minimal crate
   (`Cargo.toml` with `[package] name = "t" version = "0.1.0" edition = "2021"`,
   and `src/lib.rs`). `verify_in(&lib_rs, Some(&sb))` with
   `sb = Sandbox::new(dir, None).with_program("rexymcp-no-such-bwrap")` is
   **not** `VerifierResult::Checked { .. }` (the spawn fails, so it is
   `Skipped` or `Failed`). Without the sandbox branch, host `cargo check` runs
   and returns `Checked`.

### `mcp/src/runner.rs`

7. **`exec_tools_host_runs_on_host`** — `ExecTools::new(None).runner().run("echo $((40+2))", dir)`
   → `success`, and `output` contains `42`.
8. **`exec_tools_sandboxed_uses_the_sandbox`** —
   `ExecTools::new(Some(Sandbox::new(dir, None).with_program("true")))`.
   `.runner().run("echo $((40+2))", dir)` → `success` (`true` exits 0), and
   `output` does **not** contain `42` (the command never ran).
9. **`run_phase_with_finalizes_through_host_runner`** — copy the setup of
   `run_phase_with_finalizes_an_in_progress_doc_to_review` (line 764). Add a
   small recording runner to the test module, in the same shape as
   `RecordingRunner` in `mcp/src/finalize.rs`'s tests. That one is private to
   its module, so copy it: a `Mutex<Vec<String>>` that each `run` call pushes
   `command` onto, and that returns `success: true`. Use two recorders:
   `runner: &gate_rec` and `host_runner: &host_rec`. After the run, some
   command in `host_rec` starts with `git commit`, and no command in
   `gate_rec` starts with `git `. `Config::default()` sets no gate commands, so
   `gate_rec` may be empty. With `FinalizeInput` still on `seams.runner`, the
   `git commit` lands in `gate_rec` and the test fails.

## End-to-end verification

Run each of these and paste its output into the Update Log.

```bash
cargo test -p rexymcp-executor sandbox -- --ignored > /tmp/p10_ignored.txt 2>&1; echo "exit=$?" >> /tmp/p10_ignored.txt
```

This must show phase 07's two ignored tests and test 4 passing.

Then run a gate command through the real sandbox with a secret-looking
variable set **on the command line** (not in a test), as a positive control
that the environment is really cleared:

```bash
REXYMCP_P10_CANARY=leak cargo test -p rexymcp-executor sandboxed_runner_passes_only_allowlisted_env -- --ignored --nocapture > /tmp/p10_canary.txt 2>&1; echo "exit=$?" >> /tmp/p10_canary.txt
env REXYMCP_P10_CANARY=leak sh -c 'env | grep -c REXYMCP_P10_CANARY' >> /tmp/p10_canary.txt
```

Paste `/tmp/p10_canary.txt`. The test must pass (`exit=0`) with the canary set,
and the last line must be `1`, which shows the canary really was in the parent
environment.

## Authorizations

- [x] May edit `executor/src/security/sandbox.rs` (Spec §1 and its tests).
- [x] May edit `executor/src/agent/command.rs`, `executor/src/agent/verify.rs`
      and `executor/src/governor/verifier.rs`, including their test modules.
- [x] May edit `mcp/src/runner.rs`: `Seams`, `run_phase_with`'s
      `FinalizeInput`, `run_phase`, the new `ExecTools`, and its test module.
      In existing tests the only permitted change is adding
      `host_runner: &NoopRunner,`.
- [x] May add `#[ignore = "needs bwrap and user namespaces"]` to test 4 only.
- [x] May add the sentence in Spec §5 to `docs/privacy.md`.
- [ ] May add a dependency — **no.**
- [ ] May edit `mcp/src/finalize.rs`, `mcp/src/server.rs`, `mcp/src/resume.rs`,
      `executor/src/tools/bash.rs` or `executor/src/security/scope.rs` —
      **no.**
- [ ] May write `unsafe`, or call `std::env::set_var`/`remove_var` — **no.**

## Out of scope

- **Protecting `.git/` and `rexymcp.toml`** from `bash` and from the file
  tools, and refusing a cloud dispatch when the repo root has no `.git`. That
  is phase 11. Until it lands, the model can still write `.git/hooks`, and
  finalize's host `git commit` would run them. Cloud dispatch stays halted.
- **Toolchains outside the sandbox's view** (`ruff` in `~/.local/bin`, `tsc`
  via `~/.nvm`). They report `Skipped` now. Adding read paths is a later,
  config-driven change.
- **Host-side git in `resume.rs`** (`git diff HEAD`). It reads the repo and
  runs no hooks. Phase 11 covers `.git/config`.
- **Findings 8 and 9** — phases 09 and 08.

## Update Log

(Filled in by the executor. See WORKFLOW.md § "Update Log entries".)

<!-- entries appended below this line -->

### Update — 2026-09-17 11:52 (started)

Starting phase 10. Pre-flight: `git status --short` clean; `bwrap --version` → bubblewrap 0.12.0; `cargo test -p rexymcp-executor verifier` → 62 passed, 2 ignored; `cargo test -p rexymcp runner` → 711 passed (0 failed); `cargo test -p rexymcp` → 711 + 2 passed. Beginning with Spec §1 (`Sandbox::command_prefix`).

### Update — 2026-09-17 (escalation)

**Chosen lever:** resume
**Rationale:** the first run (`a6907996`) hit `budget_exceeded` at 200 turns
with all Spec work on disk. The architect's gate run on the partial tree: build
passes; `cargo test` 714 + 2 + 1156 (7 ignored) passes; all 9 test-plan tests
exist; `docs/privacy.md` has the sentence. What remains is rustfmt, one clippy
error, E2E, a mutation check, the Update Log and the commit. The spec was not
the problem.

### Update — 2026-09-17 13:44 (resume, closing out)

Completed the remaining closeout steps on the tree left by run `a6907996`.
No Spec code was reworked; §1–§4 were already on disk and verified by the
architect's gate run.

**What changed in this run:**

- **Clippy fix** — `executor/src/governor/verifier.rs` (ruff `cwd` fallback):
  `target.parent().map(PathBuf::from).unwrap_or_else(PathBuf::new)` →
  `target.parent().map(PathBuf::from).unwrap_or_default()`. Nothing else in
  `verify_python` changed.
- **Formatting** — ran `rustfmt --edition 2024` on the four unformatted files
  (`executor/src/agent/command.rs`, `executor/src/governor/verifier.rs`,
  `executor/src/governor/verifier_tests.rs`, `mcp/src/runner.rs`).
  `cargo fmt --all --check` is now clean.

**Gate results (all run as separate commands, all pass):**

- `cargo fmt --all --check` → clean (exit 0)
- `cargo build` → clean
- `cargo clippy --all-targets --all-features -- -D warnings` → clean
- `cargo test` → unchanged counts, per the architect's baseline:
  `714 passed` (executor lib), `2 passed` (executor bin),
  `1156 passed; 7 ignored` (mcp lib), `0` doc-tests.

**Mutation check** — `sandbox_exec_failed` in `executor/src/governor/verifier.rs`:
with the body replaced by `false`, `cargo test -p rexymcp-executor
sandbox_exec_failure_is_detected` **fails** as required:

```
assertion failed: sandbox_exec_failed(true, false, stderr)
test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 1162 filtered out
```

After restoring the line, the same command passes:

```
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 1162 filtered out
```

**End-to-end verification** — both blocks run exactly as written in the phase
doc. `cargo test -p rexymcp-executor sandbox -- --ignored` (output file
`/tmp/p10_ignored.txt`, pasted below) shows the three sandbox-gated tests
passing: phase 07's two plus this phase's test 4.

```
Compiling rexymcp-executor v0.9.1 (/home/gpratt/rexyMCP/executor)
    Finished `test` profile [unoptimized + debuginfo] target(s) in 7.87s
     Running unittests src/lib.rs (target/debug/deps/executor-090557a76b736c2e)

running 3 tests
test security::sandbox::tests::probe_succeeds_where_bwrap_works ... ok
test agent::command::tests::sandboxed_runner_passes_only_allowlisted_env ... ok
test security::sandbox::tests::sandbox_blocks_home_and_state_dir ... ok

test result: ok. 3 passed; 0 failed; 0 ignored; 0 measured; 1160 filtered out; finished in 0.05s

exit=0
```

Then the canary run — the test with a secret-looking variable set on the
command line (`REXYMCP_P10_CANARY=leak`) as a positive control that the
environment really is cleared inside the sandbox. Output file
`/tmp/p10_canary.txt`, pasted below; the final line is `1`, confirming the
canary was present in the parent (host) environment:

```
Finished `test` profile [unoptimized + debuginfo] target(s) in 0.44s
     Running unittests src/lib.rs (target/debug/deps/executor-090557a76b736c2e)

running 1 test
test agent::command::tests::sandboxed_runner_passes_only_allowlisted_env ... ok

test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 1162 filtered out; finished in 0.01s

exit=0
1
```

**Deviation note:** none of the Spec's code deviates from the doc. One small
adaptations worth flagging: the spec's `checker_command` sketch took
`program: &str`; the existing tsc site holds `cmd.program` as a `PathBuf`, so
the implemented signature is `checker_command(sandbox, program: &Path, cwd)`
and the cargo/ruff sites pass `Path::new("cargo")` / `Path::new("ruff")` while
the tsc site passes the resolved program path. Behavior is identical; no
`unwrap`/`expect`/indexing is used at any spawn site.

### Update — ts=1789653217464 (complete, server-authored)

**Summary:** Working tree clean, all 9 files committed. Phase 10 complete.

**Summary + Notes for review**

Closed out phase 10 (resume run). All Spec work (§1 `Sandbox::command_prefix`, §2 `SandboxedCommandRunner`, §3 sandbox-capable verifier with `checker_command`/`sandbox_exec_failed`, §4 `ExecTools` + `host_runner` seam, §5 the `docs/privacy.md` sentence) was already on disk from the first run and verified by the architect's gate run; this run did the remaining closeout only: fixed the single clippy error (`unwrap_or_else(PathBuf::new)` → `unwrap_or_default()` in `verify_python`'s cwd fallback), rustfmt'd the four unformatted files, and re-ran all gates — `cargo fmt --all --check`, `cargo build`, `cargo clippy --all-targets --all-features -- -D warnings`, and `cargo test` (714 + 2 + 1156 passed, 7 ignored — unchanged).

E2E: both blocks ran exactly as specified. `/tmp/p10_ignored.txt` shows all 3 sandbox-gated tests passing (phase 07's two + test 4), and the canary run passed (`exit=0`) with the final line `1`, confirming `REXYMCP_P10_CANARY` really was in the parent environment while being excluded from the sandbox. Mutation check done: `sandbox_exec_failed` forced to `false` makes `sandbox_exec_failure_is_detected` fail (`0 passed; 1 failed`); restored, it passes (`1 passed`).

Notes: the spec's `checker_command` sketch took `program: &str`; since the tsc site already holds `cmd.program` as a `PathBuf`, the implemented signature is `checker_command(sandbox, program: &Path, cwd)` (cargo/ruff pass `Path::new("cargo")`/`Path::new("ruff")`). No `unwrap`/`expect`/indexing at any spawn site. Status left at `in-progress` per the contract — the server flips it to `review` and writes the completion entry.

**Executor:** RedHatAI/Qwen3.8-27B-INT4

**Gates:** format=run, build=run, lint=run, test=run

**Command output tails:**

```
FORMAT


BUILD
   Compiling rexymcp-executor v0.9.1 (/home/gpratt/rexyMCP/executor)
   Compiling rexymcp v0.9.1 (/home/gpratt/rexyMCP/mcp)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 5.24s


LINT
    Checking rexymcp-executor v0.9.1 (/home/gpratt/rexyMCP/executor)
    Checking rexymcp v0.9.1 (/home/gpratt/rexyMCP/mcp)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 4.37s


TEST
rejects_path_outside_root ... ok
test tools::symbols::tests::references_exclude_substring ... ok
test tools::symbols::tests::references_finds_call_sites ... ok
test tools::symbols::tests::references_respects_max_results ... ok
test tools::symbols::tests::single_file_unsupported_extension_advisory_error ... ok
test tools::symbols::tests::type_mismatch_returns_recovery_hint ... ok
test tools::update_task::tests::flips_active_task_to_done ... ok
test tools::update_task::tests::flips_pending_task_to_active ... ok
test tools::update_task::tests::invalid_args_hint_lists_incomplete_ids ... ok
test tools::update_task::tests::invalid_args_hint_reports_all_complete ... ok
test tools::update_task::tests::invalid_state_returns_advisory_error ... ok
test tools::update_task::tests::malformed_args_returns_advisory_error ... ok
test tools::update_task::tests::metadata_shape_is_unchanged ... ok
test tools::symbols::tests::references_no_matches_advisory ... ok
test tools::update_task::tests::null_args_returns_recovery_hint ... ok
test tools::update_task::tests::result_flags_redundant_remark ... ok
test tools::update_task::tests::result_lists_remaining_incomplete_ids ... ok
test tools::update_task::tests::result_reports_all_complete_when_last_done ... ok
test tools::symbols::tests::metadata_carries_definitions_and_files_count ... ok
test tools::update_task::tests::success_output_names_task ... ok
test tools::update_task::tests::unknown_id_returns_advisory_error ... ok
test tools::symbols::tests::references_single_file_path ... ok
test tools::write_file::tests::append_creates_file_if_missing ... ok
test tools::write_file::tests::append_false_overwrites ... ok
test tools::write_file::tests::appends_to_existing_file ... ok
test tools::write_file::tests::missing_path_returns_recovery_hint ... ok
test tools::write_file::tests::non_object_args_do_not_panic ... ok
test tools::write_file::tests::creates_new_file ... ok
test tools::write_file::tests::overwrites_existing_file ... ok
test tools::write_file::tests::reports_missing_parent_dir ... ok
test tools::write_file::tests::rejects_malformed_args ... ok
test tools::write_file::tests::scope_escape_returns_advisory_error_and_writes_nothing ... ok
test tools::write_file::tests::success_output_includes_line_count ... ok
test tools::symbols::tests::references_snippet_shows_source_line ... ok
test tools::symbols::tests::finds_rust_struct_and_trait ... ok
test tools::symbols::tests::references_truncation_note_omits_kind_filter ... ok
test tools::symbols::tests::reports_line_and_column ... ok
test tools::symbols::tests::unsupported_extension_skipped_in_dir_walk ... ok
test tools::symbols::tests::respects_gitignore ... ok
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

test result: ok. 1156 passed; 0 failed; 7 ignored; 0 measured; 0 filtered out; finished in 6.19s


running 0 tests

test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

    Finished `test` profile [unoptimized + debuginfo] target(s) in 0.50s
     Running unittests src/main.rs (target/debug/deps/rexymcp-3de3446ef9b6a3ce)
     Running tests/readme_config_reference.rs (target/debug/deps/readme_config_reference-22f887757c17cb2b)
     Running unittests src/lib.rs (target/debug/deps/executor-0c52bf72536f9e8e)
   Doc-tests executor

```

**Files changed:**

- `docs/dev/milestones/F05-privacy-security-hardening/phase-10-sandboxed-commands.md` — +89 -0
- `executor/src/governor/verifier.rs` — +10 -12

**Commit:** e3d64117188ff3b1798aa836b61363354aa6b426

**Notes:** server-authored completion entry (executor no longer owns the bookkeeping tail; see M27 phase-03).
