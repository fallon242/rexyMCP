# Phase 7: confine `bash` for a cloud executor

**Milestone:** F05 — Privacy and security hardening
**Status:** in-progress
**Depends on:** phase 06 (done)
**Estimated diff:** ~350 lines, about half of it tests
**Tags:** language=rust, kind=security, size=m

> **Dispatch on a LOCAL executor only.** This phase fixes the hole that stops
> all cloud dispatch (finding 10). Do not send it to a cloud endpoint.

## Goal

The executor's `bash` tool runs `sh -c <command>` with the cwd set to the repo.
Nothing stops a command from reading `$HOME`, `~/.config`, or another project's
vault, or from deleting files under `.rexymcp/`. On 2026-09-16 a cloud executor
did all of those things (F05 README, finding 10).

After this phase:

1. When the executor endpoint is a **cloud** host, every `bash` command runs
   inside a `bwrap` (bubblewrap) sandbox. In the sandbox the host filesystem is
   read-only, `/home` and `/tmp` are empty, the repo is writable, and the repo's
   `.rexymcp/` directory is an empty scratch directory. Nothing written there
   reaches the host.
2. If `bwrap` is missing or cannot create a sandbox, a cloud dispatch **stops
   before turn 1**, in the same way as a failed pre-scan (phase 06).
3. For **every** executor, local or cloud, the file tools (`read_file`,
   `write_file`, `patch`, `delete_file`, `move_file`, …) refuse paths under
   `<repo>/.rexymcp/`. The one exception is `<repo>/.rexymcp/output/`, where the
   output filter writes the full-output recovery logs the model is told to read.
4. The executor contract forbids looking for redaction dictionaries, vaults,
   keys or credentials.

A local executor's `bash` is unchanged. The sandbox applies only to cloud
executors.

## Architecture references

- `executor/src/tools/bash.rs` — the `Bash` tool. Spawn at lines 138–150,
  constructors at lines 310–320, tests from line 324.
- `executor/src/security/scope.rs` — `Scope::resolve` and `confine_to_root`
  (lines 51–85). Every file tool calls `scope.resolve(...)` and turns an `Err`
  into `ToolResult { error: Some(e.to_string()), .. }`.
- `executor/src/security/mod.rs` — module list and re-exports.
- `executor/src/privacy/egress.rs` — `endpoint_is_local(base_url)` (line 20).
  Read only.
- `mcp/src/runner.rs` — `Seams` (lines 97–105), `build_registry`
  (lines 188–227), the `build_registry` call in `run_phase_with` (line 252),
  the pre-scan block in `run_phase` (lines 382–408), the `Seams` literal
  (lines 438–445), `prescan_refusal` (lines 473–481).
- `executor/templates/executor_contract.md` — "Hard rules" section (line 138).
- `docs/privacy.md` — the "② Executor egress" bullet (around line 28).

## Pre-flight

1. `git status --short` is clean. If it is not, stop and file a blocker.
2. `bwrap --version` prints a version. Record it.
3. `cargo test -p rexymcp-executor bash` and `cargo test -p rexymcp runner` pass.
   Record the counts.
4. Read `executor/src/tools/bash.rs` lines 1–320 and `mcp/src/runner.rs`
   lines 97–481 in full before editing.

## Current state

```rust
// executor/src/tools/bash.rs:138-150
        let mut cmd = Command::new("sh");
        cmd.arg("-c")
            .arg(&parsed.command)
            .current_dir(self.scope.root())
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped());

        cmd.env_clear();
        for (key, value) in std::env::vars() {
            if is_allowed_env_key(&key) {
                cmd.env(&key, &value);
            }
        }
```

```rust
// executor/src/tools/bash.rs:306-320
pub fn is_allowed_env_key(key: &str) -> bool {
    ALLOWED_ENV_KEYS.contains(&key) || key.starts_with("LC_")
}

pub fn bash(scope: Scope, default_timeout_secs: u32) -> Arc<dyn Tool> {
    bash_with_filter(scope, default_timeout_secs, true)
}

pub fn bash_with_filter(scope: Scope, default_timeout_secs: u32, filter: bool) -> Arc<dyn Tool> {
    Arc::new(Bash {
        scope,
        default_timeout_secs,
        filter,
    })
}
```

```rust
// mcp/src/runner.rs:382-408 (run_phase)
    let redact = inp.test_client.is_none()
        && rexymcp_executor::privacy::egress::should_redact_egress(
            &inp.cfg.privacy,
            &inp.cfg.executor.base_url,
        );
    let mut egress_terms = Vec::new();
    let mut pii_files = std::collections::HashSet::new();
    if redact {
        match rexymcp_executor::privacy::egress::build_egress_index(inp.repo_path, &inp.cfg.privacy)
            .await
        {
            Ok((terms, files)) => {
                egress_terms = terms;
                pii_files = files;
            }
            Err(e) => return Err(prescan_refusal(&e)),
        }
    }
```

The mcp crate refers to executor items with the full path
`rexymcp_executor::...`, as above. Error type:
`rexymcp_executor::error::Error::Privacy(String)`.

## Spec

### 1. New module `executor/src/security/sandbox.rs`

Add `pub mod sandbox;` to `executor/src/security/mod.rs` and re-export
`pub use sandbox::Sandbox;`. Start the file with a `//` header comment in the
same style as `scope.rs` (what the module does, in two to four lines).

```rust
#[derive(Debug, Clone)]
pub struct Sandbox {
    program: String,      // "bwrap"; a test seam, see with_program
    root: PathBuf,        // canonical repo root (Scope::root())
    home: Option<PathBuf>,
}

impl Sandbox {
    /// `home` is the value of $HOME, or None when unset. `program` = "bwrap".
    pub fn new(root: &Path, home: Option<PathBuf>) -> Self;

    /// Replace the program name. Tests use a name that does not exist.
    pub fn with_program(self, program: &str) -> Self;

    /// The full argv to spawn: element 0 is `program`, the last three are
    /// "sh", "-c", command.
    pub fn argv(&self, command: &str) -> Vec<String>;
}

/// Run `<program> --ro-bind / / --dev /dev true` with stdin, stdout and stderr
/// null. Ok(()) on exit 0. Otherwise Err with a one-line reason that starts
/// with the program name: the spawn error ("bwrap: No such file or
/// directory …") or the exit status.
pub fn probe_with(program: &str) -> Result<(), String>;

/// `probe_with("bwrap")`.
pub fn probe() -> Result<(), String>;
```

`probe_with` uses `std::process::Command` (blocking is fine: it runs once,
before turn 1).

`argv` produces exactly these arguments, **in this order**. bwrap applies
mounts in order, so a later mount covers an earlier one. The order is the
security property.

| # | Arguments | Why |
|---|-----------|-----|
| 1 | `--die-with-parent` `--unshare-pid` | killing bwrap on timeout kills the whole process tree |
| 2 | `--ro-bind` `/` `/` | toolchains stay usable; nothing on the host is writable |
| 3 | `--dev` `/dev` `--proc` `/proc` | a working `/dev/null` and a `/proc` for the new pid namespace |
| 4 | `--tmpfs` `/tmp` | hides other processes' temp files |
| 5 | `--tmpfs` `/home` | hides every home directory |
| 6 | `--tmpfs` `<home>`, only if `home` is Some and not under `/home` | same, for a home outside `/home` (for example `/root`, `/var/home/x`) |
| 7 | for each path in the toolchain list below: `--ro-bind-try` `<p>` `<p>` | cargo and rustup still work |
| 8 | `--bind` `<root>` `<root>` | the repo is writable |
| 9 | `--tmpfs` `<root>/.rexymcp` | sessions, vault, keys: hidden, and writes do not reach the host |
| 10 | `--chdir` `<root>` | |
| 11 | `sh` `-c` `<command>` | |

Toolchain list for row 7, only when `home` is Some, in this order:
`<home>/.cargo/bin`, `<home>/.cargo/registry`, `<home>/.cargo/git`,
`<home>/.cargo/config.toml`, `<home>/.rustup`.

**Do not bind `<home>/.cargo` as a whole.** It can hold
`credentials.toml` (a registry token). `--ro-bind-try` skips a path that does
not exist.

Paths go into argv with `to_string_lossy().into_owned()`.

Row 9 always appears, even if `<root>/.rexymcp` does not exist yet. bwrap then
creates the empty mount-point directory on the host, which is harmless.

These flags were checked by hand on the target host (bubblewrap 0.12.0).
`cargo check -p rexymcp-executor` succeeded inside the sandbox. `~/.config` was
invisible, `touch ~/.cargo/x` failed with "Read-only file system", and
`.rexymcp/vault/key` was not visible from inside. After killing the outer
`bwrap` with `kill -9`, no `sleep` process from inside was left running.

### 2. `Bash` runs through the sandbox when it has one

Add a field `sandbox: Option<Sandbox>` to `Bash`. Add a constructor:

```rust
pub fn bash_sandboxed(
    scope: Scope,
    default_timeout_secs: u32,
    filter: bool,
    sandbox: Option<Sandbox>,
) -> Arc<dyn Tool>
```

`bash_with_filter` calls `bash_sandboxed(scope, default_timeout_secs, filter, None)`.
`bash` stays as it is. Export `bash_sandboxed` from `executor/src/tools/mod.rs`
next to `bash_with_filter` (line 18).

In `execute`, build the `Command` from the sandbox when there is one:

```rust
let mut cmd = match &self.sandbox {
    Some(sb) => {
        let argv = sb.argv(&parsed.command);
        let mut c = Command::new(&argv[0]);
        c.args(&argv[1..]);
        c
    }
    None => {
        let mut c = Command::new("sh");
        c.arg("-c").arg(&parsed.command);
        c
    }
};
cmd.current_dir(self.scope.root())
    .stdin(std::process::Stdio::null())
    .stdout(std::process::Stdio::piped())
    .stderr(std::process::Stdio::piped());
```

`argv` is never empty, but do not index it blindly in production code if clippy
or the no-panic rule objects. Use `split_first()` and return a `ToolResult`
error in the `None` arm. Everything else in `execute` stays as it is: the
classifier, the env allowlist, the timeout and kill, and the output filter.

Keep the stdin comment at lines 133–137. It still applies.

Update the header comment at lines 5–7 to say that when a sandbox is set,
`bash` runs under bwrap with a read-only host, a hidden `/home` and a hidden
`.rexymcp/`, and that without one it is not a jail.

### 3. Scope refuses `.rexymcp/`, except `.rexymcp/output/`

In `executor/src/security/scope.rs`:

- Add a variant `ScopeError::Protected { requested: String }`. It displays as
  `path is inside rexymcp's private state directory and cannot be accessed: {requested}`.
- In `confine_to_root`, after building `result` and before returning it, check
  whether `result` is under the root. If `result.strip_prefix(root)` has a first
  component equal to `.rexymcp`, and the second component is not `output`,
  return `Err(ScopeError::Protected { requested })`. The `requested` value is
  `candidate.to_string_lossy().into_owned()`, the same as `Escapes`.
- `<root>/.rexymcp` on its own (no second component) is **protected**.

Do this in `confine_to_root`, not in each tool. Every file tool goes through
`Scope::resolve`, so one check covers all of them.

| Requested (relative to root) | Result |
|---|---|
| `.rexymcp` | `Protected` |
| `.rexymcp/vault/key` | `Protected` |
| `.rexymcp/sessions/x.jsonl` | `Protected` |
| `./.rexymcp/vault` | `Protected` |
| `src/../.rexymcp/vault/key` (with `src/` existing) | `Protected` |
| `.rexymcp/output/cmd-output-1.log` | Ok |
| `.rexymcp/output` | Ok |
| `.rexymcpx/file` | Ok (different name, not a prefix match) |
| `docs/.rexymcp/notes.md` | Ok (only the top-level one is protected) |
| `rexymcp.toml` | Ok |

Compare **path components**, not strings. `starts_with(".rexymcp")` on a string
would wrongly match `.rexymcpx`.

### 4. `run_phase` sandboxes cloud dispatches and fails closed

In `mcp/src/runner.rs`:

- Add `sandbox: Option<rexymcp_executor::security::Sandbox>` to `Seams`, with a
  one-line doc comment. Every existing `Seams { .. }` literal in the test module
  gets `sandbox: None`. That is the only change allowed to existing tests.
- `build_registry` gains a fifth parameter `sandbox: Option<Sandbox>` and builds
  the bash tool with
  `tools::bash_sandboxed(scope.clone(), bash_timeout_secs, filter_output, sandbox)`.
  The call in `run_phase_with` passes `seams.sandbox.clone()`. The three
  existing `build_registry` tests pass `None`.
- In `run_phase`, **before** the pre-scan block (line 382), add:

  ```rust
  let sandbox = if inp.test_client.is_none()
      && !rexymcp_executor::privacy::egress::endpoint_is_local(&inp.cfg.executor.base_url)
  {
      if let Err(reason) = rexymcp_executor::security::sandbox::probe_with(inp.sandbox_program) {
          return Err(sandbox_refusal(&reason));
      }
      let root = std::fs::canonicalize(inp.repo_path)?;
      Some(
          rexymcp_executor::security::Sandbox::new(
              &root,
              std::env::var_os("HOME").map(std::path::PathBuf::from),
          )
          .with_program(inp.sandbox_program),
      )
  } else {
      None
  };
  ```

  Check whether `canonicalize`'s `io::Error` converts into the executor `Error`
  with `?`. `std::fs::read_to_string(...)?` at the top of `run_phase_with`
  suggests it does. If it does not, map it the way nearby code does.
  Put `sandbox` into the `Seams` literal at lines 438–445.

- The sandbox decision uses **only** the endpoint, not `[privacy]`. A cloud
  executor is sandboxed even with privacy off.
- Add, next to `prescan_refusal`:

  ```rust
  /// Turn a failed sandbox probe into the refusal `run_phase` returns.
  fn sandbox_refusal(reason: &str) -> rexymcp_executor::error::Error {
      rexymcp_executor::error::Error::Privacy(format!(
          "the bash sandbox is unavailable ({reason}), so the dispatch to the cloud executor \
           was stopped before any content was sent. Install bubblewrap (the `bwrap` binary) \
           and check that unprivileged user namespaces are enabled, or run the phase on a \
           local executor."
      ))
  }
  ```

  No opt-out key. A cloud executor without a sandbox does not run.

### 5. Executor contract

In `executor/templates/executor_contract.md`, add one bullet to the end of the
"Hard rules — non-negotiable" list, after the in-place-edit bullet:

```markdown
- **Stay inside the project.** Do not read, list, search, or modify anything
  outside the project root, and do not touch `.rexymcp/` other than reading
  `.rexymcp/output/`. Never look for redaction dictionaries, mask tables,
  vaults, keys, API keys, or credentials, anywhere. If a name in your view is
  replaced by `[REDACTED:…]` and you cannot work without it, stop and file a
  blocker.
```

If a test pins the contract text or its section list (look in
`executor/src/agent/contract.rs`), it must still pass. Do not edit that test. If
it fails, file a blocker.

### 6. Document it

In `docs/privacy.md`, at the end of the "② Executor egress to a cloud model"
bullet, add two sentences. The first: for a cloud executor, `bash` runs inside a
bubblewrap sandbox that hides home directories and `.rexymcp/` and makes the
host read-only, and the dispatch stops if the sandbox is unavailable. The
second: only the Rust toolchain under `$HOME` (`~/.cargo`, `~/.rustup`) is
visible inside the sandbox. Change nothing else in the file.

## Acceptance criteria

- [ ] `Sandbox::argv` produces the arguments in Spec §1, in that order.
- [ ] A cloud `base_url` with no test client and a failing `probe` makes
      `run_phase` return `Err(Error::Privacy(_))` naming `bwrap`. The
      AI client is never called.
- [ ] A local `base_url` never calls `probe` and never builds a sandbox.
- [ ] Every file tool refuses `.rexymcp/vault/key`. `read_file` still reads
      `.rexymcp/output/<file>`.
- [ ] With a sandbox, a `bash` command cannot see `$HOME/.config`, cannot write
      under `$HOME`, and cannot delete a file under `<root>/.rexymcp/` on the host
      (ignored tests, run in E2E).
- [ ] The executor contract carries the "Stay inside the project" rule.
- [ ] `docs/privacy.md` has the two sentences from Spec §6.
- [ ] Each new non-ignored test fails when its fix is reverted (show one in the
      Update Log).
- [ ] `cargo fmt --all --check`, `cargo build`,
      `cargo clippy --all-targets --all-features -- -D warnings` and
      `cargo test` all pass.

## Test plan

All tests are hermetic: a `tempfile::TempDir` for any repo, and no network.

### `executor/src/security/sandbox.rs` (always run, no bwrap needed)

1. **`argv_orders_mounts_so_repo_state_is_hidden`** — `Sandbox::new(root, Some(home))`
   with `root = /srv/repo` and `home = /home/u` (plain `PathBuf`s, no filesystem
   access). Assert: `argv[0] == "bwrap"`. The last three elements are
   `["sh", "-c", "echo hi"]`. The index of the `--ro-bind / /` triple is less
   than the index of `--tmpfs /home`, which is less than the index of
   `--bind /srv/repo /srv/repo`, which is less than the index of
   `--tmpfs /srv/repo/.rexymcp`. `--die-with-parent` and `--unshare-pid` are
   present. `--chdir /srv/repo` is present. Write a small test helper that
   finds the position of a consecutive slice in `argv`.
2. **`argv_binds_toolchain_dirs_but_not_cargo_home`** — same input. `argv`
   contains `--ro-bind-try /home/u/.cargo/registry /home/u/.cargo/registry` and
   `--ro-bind-try /home/u/.rustup /home/u/.rustup`. **Must NOT** contain any
   element equal to `/home/u/.cargo`.
3. **`argv_hides_home_outside_slash_home`** — `home = /root`. `argv` contains
   `--tmpfs /root`, and it comes before the `--bind` of the root.
4. **`argv_without_home_has_no_toolchain_binds`** — `home = None`. No element
   is `--ro-bind-try`.
5. **`sandbox_blocks_home_and_state_dir`** — `#[ignore = "needs bwrap and user namespaces"]`.
   Real run. Make a `TempDir` repo with `.rexymcp/vault/key` holding
   `"k"`. Build `Sandbox::new(&canonical_repo, std::env::var_os("HOME").map(PathBuf::from))`.
   Run `sb.argv(cmd)` with `std::process::Command` for each of these, and
   assert:
   - `test -e "$HOME/.config"` → non-zero exit (skip this check if the host
     has no `~/.config`)
   - `touch "$HOME/.sandbox-probe"` → non-zero exit
   - `cat .rexymcp/vault/key` → non-zero exit
   - `rm -rf .rexymcp/vault` → then, on the **host**, `.rexymcp/vault/key`
     still exists with content `"k"`
   - `echo ok > written.txt` → exit 0, and `written.txt` exists on the host
6. **`probe_succeeds_where_bwrap_works`** — `#[ignore = "needs bwrap and user namespaces"]`.
   `probe()` is `Ok(())`.

### `executor/src/security/scope.rs`

7. **`rejects_rexymcp_state_paths`** — create `.rexymcp/vault/key`,
   `.rexymcp/sessions/`, and `src/`. Each "Protected" row in the Spec §3
   table returns `Err(ScopeError::Protected { .. })`.
8. **`allows_rexymcp_output_and_lookalikes`** — create
   `.rexymcp/output/cmd-output-1.log`, `.rexymcpx/file`, and
   `docs/.rexymcp/notes.md`. Each "Ok" row in the Spec §3 table resolves `Ok`.

### `executor/src/tools/bash.rs`

9. **`sandboxed_bash_spawns_the_sandbox_program`** — proves `execute` spawns
   the sandbox argv, not `sh`. Build
   `bash_sandboxed(scope, 5, false, Some(sb))`, where
   `sb = Sandbox::new(tempdir_root, None).with_program("rexymcp-no-such-bwrap")`.
   Run `{"command": "echo inside"}`. Assert that `result.error` contains
   `failed to spawn shell` and that `result.output` does not contain `inside`.
   With the sandbox branch reverted, plain `sh` runs and prints `inside`, so
   the test fails.

   Existing `bash.rs` tests must pass unchanged. They use `bash(...)` or
   `bash_with_filter(...)`, which have no sandbox.

### `executor/src/tools/read_file.rs` (or whichever file tool test module is nearest)

10. **`read_file_refuses_vault_key`** — a `TempDir` with `.rexymcp/vault/key`.
    `read_file` on `.rexymcp/vault/key` returns a `ToolResult` whose `error`
    contains `private state directory`. **`read_file_reads_output_recovery_log`**
    — `.rexymcp/output/cmd-output-1.log` with content `"full"`. `read_file`
    returns it with no error.

### `mcp/src/runner.rs`

11. **`sandbox_refusal_names_bwrap_and_local_fallback`** — pure.
    `sandbox_refusal("bwrap not found: No such file or directory")` is
    `Error::Privacy`, and its message contains `bwrap`, `bubblewrap` and
    `local executor`.
12. **`run_phase_stops_when_sandbox_unavailable`** — calls `run_phase`, set up
    like phase 06's `run_phase_stops_when_prescan_fails` (read that test and
    copy its setup). Differences: privacy is off (`Config::default()`), and
    `cfg.executor.base_url = "https://api.example.com/v1"`. To make the probe fail
    hermetically, add a `sandbox_program: &'a str` field to `RunPhaseConfig`
    with a one-line doc comment. Production passes `"bwrap"`; this test passes
    `"rexymcp-no-such-bwrap"`. `run_phase` calls
    `probe_with(inp.sandbox_program)` as shown in Spec §4. Assert the
    result is `Err(Error::Privacy(m))` with `m` containing
    `rexymcp-no-such-bwrap`.

    `RunPhaseConfig` is constructed at `mcp/src/server.rs:232`,
    `mcp/src/server.rs:288`, `mcp/src/main.rs:573`, and in the test at
    `mcp/src/runner.rs:1229`. Each gets `sandbox_program: "bwrap"`. That is
    authorized. Stop and file a blocker if you find other construction sites
    outside tests.

    **Must NOT happen:** with a `localhost` base_url and the same bogus
    program name, `run_phase` must not return the sandbox refusal. Test it as
    **`run_phase_skips_sandbox_for_local_endpoint`**: same setup,
    `base_url = "http://localhost:9/v1"`, and assert that if the result is an
    `Err`, its message does **not** contain `bubblewrap`. The run will fail
    later when it contacts `localhost:9`. That is expected.

## End-to-end verification

Run these and paste the output of each into the Update Log.

```bash
cargo test -p rexymcp-executor sandbox -- --ignored > /tmp/p07_ignored.txt 2>&1; echo "exit=$?" >> /tmp/p07_ignored.txt
```

This must show tests 5 and 6 passing.

Then check the refusal through the real CLI with a missing bwrap, by hiding it
from PATH:

```bash
T=$(mktemp -d)
mkdir -p "$T/repo" "$T/bin"
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
EOF
cargo build -q -p rexymcp
BIN="$PWD/target/debug/rexymcp"
env PATH="$T/bin" "$BIN" run-phase --no-telemetry \
  --config "$T/repo/rexymcp.toml" --phase-doc "$T/phase-01-e2e.md" --repo "$T/repo" \
  > /tmp/p07_e2e.txt 2>&1; echo "exit=$?" >> /tmp/p07_e2e.txt
```

Paste `/tmp/p07_e2e.txt`. It must show the `bash sandbox is unavailable` error
and a non-zero `exit=`. Run it from the repo root.

## Authorizations

- [x] May create `executor/src/security/sandbox.rs` and add it to
      `executor/src/security/mod.rs`.
- [x] May edit `executor/src/tools/bash.rs`, `executor/src/tools/mod.rs` (one
      export), and `executor/src/security/scope.rs`, including their test modules.
- [x] May add tests to one file-tool test module (test 10).
- [x] May edit `mcp/src/runner.rs`: `Seams`, `RunPhaseConfig`,
      `build_registry`, `run_phase`, one new function, and its test module. In
      existing tests, the only permitted change is adding the new
      `sandbox: None` / `sandbox_program` fields and the new `build_registry`
      argument.
- [x] May add `sandbox_program: "bwrap"` to the three production
      `RunPhaseConfig` construction sites named in test 12 (`server.rs` twice,
      `main.rs` once).
- [x] May add `#[ignore = "needs bwrap and user namespaces"]` to tests 5 and 6
      only. CI (`ubuntu-latest`) is not guaranteed to allow unprivileged user
      namespaces.
- [x] May add the bullet in Spec §5 to `executor/templates/executor_contract.md`.
- [x] May add the two sentences in Spec §6 to `docs/privacy.md`.
- [ ] May add a dependency — **no.** bwrap is an external binary.
      `std::process::Command` is enough.
- [ ] May add a config key — **no.** See Out of scope.
- [ ] May write `unsafe` or use Landlock or seccomp directly — **no.**
- [ ] May edit `executor/src/privacy/**`, `mcp/src/server.rs` logic, or
      `docs/architecture.md` — **no.**

## Out of scope

- **Non-Rust toolchains under `$HOME`** (nvm, pyenv, `~/.local/bin`). They are
  invisible in the sandbox, so a cloud dispatch on such a project fails its
  `bash` calls. Add an `[executor]` read-path list when a project needs one.
- **Network isolation** (`--unshare-net`). Leaking to the network does not
  add much here: the model already receives everything `bash` prints. Revisit
  if a later finding shows otherwise.
- **Sandboxing the final command set** (`RealCommandRunner`). rexymcp runs
  those commands from the phase doc's config, not the model.
- **macOS.** bwrap is Linux-only. On other platforms `probe` fails and cloud
  dispatch stops, which is the safe default.
- **A vault directory configured inside the repo but outside `.rexymcp/`.**
  It is still readable. File it if it happens in practice.
- **Findings 8 and 9.** They are phases 08 and 09.

## Update Log

(Filled in by the executor. See WORKFLOW.md § "Update Log entries".)

<!-- entries appended below this line -->

### Update — 2026-09-16 (escalation)

**Chosen lever:** resume
**Rationale:** the first run (`ac852636`, session `6aaad526`) ended at turn 69
on a transient connection error to the local vLLM server, not a spec gap; Spec
§1–§2 and most of §3 were on disk, with one compile error
(`scope.rs:94`, `if let Some` on a `Result`).

### Update — 2026-09-16 (escalation)

**Chosen lever:** resume
**Rationale:** the resumed run (session `6aaadd94`) ended `budget_exceeded` at
200 turns with 5 of 6 tasks done. The architect's gate run left only small
fixes: executor tests pass (1151), and the build passes. What remains: the
`rexymcp` test compile (a missing `let dir`, and `sandbox: None` in six `Seams`
literals), rustfmt on three files, clippy in `sandbox.rs`, `docs/privacy.md`,
E2E, and the Update Log.
