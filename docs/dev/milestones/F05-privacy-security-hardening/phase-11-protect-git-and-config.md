# Phase 11: protect `.git/` and `rexymcp.toml` from the model

**Milestone:** F05 — Privacy and security hardening
**Status:** review
**Depends on:** phase 10 (done)
**Estimated diff:** ~250 lines, over half of it tests
**Tags:** language=rust, kind=security, size=s

> **Dispatch on a LOCAL executor only.** This is the last fix before cloud
> dispatch can resume (finding 11). Do not send it to a cloud endpoint.

## Goal

rexymcp runs `git` on the host (`mcp/src/finalize.rs` runs `git add` and
`git commit`; `mcp/src/resume.rs` runs `git diff`). Git reads `.git/config`
and runs `.git/hooks/*`. Today a cloud model can write both, either with
`bash` (the repo is writable inside the sandbox) or with the file tools (which
run on the host). A hook the model writes then runs on the host, outside the
sandbox. The model can also edit `rexymcp.toml`, the config the next dispatch
loads: it can change `[commands]`, or point `base_url` at a local host so the
next run has no sandbox at all.

After this phase:

1. **Inside the sandbox** (cloud executors), `.git/config`, `.git/hooks/` and
   `rexymcp.toml` are read-only. `.git/` itself cannot be renamed or removed,
   and `git commit` still works.
2. **For every executor, local or cloud,** the file tools refuse `.git` and
   everything under it, and refuse `<root>/rexymcp.toml`.
3. A cloud dispatch **stops before turn 1** when the repo root has no `.git`,
   when `.git` is a symlink, or when `.git` is a directory with no `config`
   file.
4. The executor contract says not to edit `.git/` or `rexymcp.toml`.

### Facts checked by hand (bubblewrap 0.12.0)

- With only `--ro-bind .git/config` and `--ro-bind .git/hooks`,
  `mv .git .git2` **succeeds** inside the sandbox and renames the directory on
  the host. A fresh `.git` with any config could then be created.
- Bind-mounting `.git` onto itself first (`--bind <root>/.git <root>/.git`)
  makes that `mv` fail with `Device or resource busy`. After that:
  - `echo … > .git/hooks/pre-commit` fails with `Read-only file system`;
  - `git config core.fsmonitor evil` fails with
    `could not write config file .git/config: Device or resource busy`;
  - `git -c user.name=t -c user.email=t@t commit -qam two` succeeds.
- `rm -rf .git` inside the sandbox deletes the objects and refs but leaves
  `config` and `hooks`, because they are mount points. That loses data, but it
  does not let the model run code on the host: `config` and `hooks` are still
  the originals.
- In a git worktree, `.git` is a **file**. `--ro-bind <root>/.git <root>/.git`
  makes it unwritable (`Read-only file system`) and unmovable (`EBUSY`).
- `--ro-bind-try <root>/rexymcp.toml …` makes `echo y >> rexymcp.toml` fail
  (`Read-only file system`), `sed -i` fail (`cannot rename … Device or resource
  busy`), and `rm -f` fail (`Device or resource busy`).

## Architecture references

- `executor/src/security/sandbox.rs` — `Sandbox`, `Sandbox::new`,
  `with_program`, `command_prefix` (rows 1–10 of the mount table), and the
  tests. Read the whole file.
- `executor/src/security/scope.rs` — `confine_to_root`, the protected-path
  block at lines 92–114, and the tests `rejects_rexymcp_state_paths` and
  `allows_rexymcp_output_and_lookalikes` (around lines 215–265).
- `mcp/src/runner.rs` — the sandbox block in `run_phase` (search for
  `F05 bash confinement`), `sandbox_refusal`, and the test
  `run_phase_stops_when_sandbox_unavailable`.
- `executor/templates/executor_contract.md` — the "Stay inside the project"
  bullet at the end of the "Hard rules" list.

## Pre-flight

1. `git status --short` is clean. If it is not, stop and file a blocker.
2. `bwrap --version` and `git --version` both print a version. Record them.
3. `cargo test -p rexymcp-executor scope` and `cargo test -p rexymcp runner`
   pass. Record the counts.

## Current state

```rust
// executor/src/security/scope.rs:92-114 (end of confine_to_root)
    // The repo's .rexymcp/ holds rexymcp's own state (sessions, vault, keys);
    // the only subpath the model may touch is .rexymcp/output/ (recovery logs).
    if let Ok(rel) = result.strip_prefix(root) {
        let first = rel
            .components()
            .next()
            .map(|c| c.as_os_str())
            .and_then(|c| c.to_str())
            .map(|s| s == ".rexymcp")
            .unwrap_or(false);
        let second = rel
            .components()
            .nth(1)
            .map(|c| c.as_os_str())
            .and_then(|c| c.to_str())
            .map(|s| s == "output")
            .unwrap_or(false);
        if first && !second {
            return Err(ScopeError::Protected {
                requested: candidate.to_string_lossy().into_owned(),
            });
        }
    }
    Ok(result)
```

```rust
// mcp/src/runner.rs, run_phase (the sandbox block)
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
```

## Spec

### 1. The sandbox protects `.git` and `rexymcp.toml`

In `executor/src/security/sandbox.rs`:

```rust
/// How the repo root holds its git metadata.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GitDir {
    /// `<root>/.git` is a directory (a normal clone).
    Dir,
    /// `<root>/.git` is a file (a git worktree).
    File,
}
```

- Add a field `git: Option<GitDir>` to `Sandbox`. `Sandbox::new` sets it to
  `None`. Add `pub fn with_git(mut self, git: GitDir) -> Self`, in the same
  style as `with_program`.
- Export it: `pub use sandbox::{GitDir, Sandbox};` in
  `executor/src/security/mod.rs`.
- In `command_prefix`, **after** row 9 (`--tmpfs <root>/.rexymcp`) and
  **before** `--chdir`, add:

| # | When | Arguments |
|---|------|-----------|
| 9a | `git == Some(Dir)` | `--bind <root>/.git <root>/.git`, then `--ro-bind <root>/.git/config <root>/.git/config`, then `--ro-bind <root>/.git/hooks <root>/.git/hooks` |
| 9b | `git == Some(File)` | `--ro-bind <root>/.git <root>/.git` |
| 9c | always | `--ro-bind-try <root>/rexymcp.toml <root>/rexymcp.toml` |

  Row 9a's first bind is what stops `mv .git`. It must come before the two
  read-only binds. Leave rows 1–9 exactly as they are: phase 07's tests pin
  them.

- Add a constructor for production:

```rust
/// A sandbox for the repo at `root` (canonical), with its git layout
/// detected. Fails when the sandbox cannot protect the repo's git metadata.
pub fn for_repo(root: &Path, home: Option<PathBuf>) -> Result<Self, String>
```

  Use `std::fs::symlink_metadata(root.join(".git"))`, which does not follow
  symlinks:

  - `Err(_)` → `Err(format!("{} has no .git; a cloud dispatch needs the repo root to be a git work-tree root", root.display()))`
  - the entry is a symlink → `Err` whose message contains `.git is a symlink`
  - a directory:
    - if `root/.git/config` is not a file → `Err` whose message contains
      `.git/config`;
    - `std::fs::create_dir_all(root.join(".git/hooks"))`, mapping an error to
      `Err(format!("cannot create .git/hooks: {e}"))`. The mount needs the
      directory to exist. An empty `hooks` directory is what `git init`
      creates anyway;
    - `Ok(Sandbox::new(root, home).with_git(GitDir::Dir))`
  - a file → `Ok(Sandbox::new(root, home).with_git(GitDir::File))`

  No `unwrap`/`expect`.

### 2. The file tools refuse `.git` and `rexymcp.toml`

In `executor/src/security/scope.rs`, replace the protected-path block quoted
above with one match. It keeps the `.rexymcp` behavior exactly and adds the new
cases:

```rust
    // Paths the model must not touch through the file tools: rexymcp's state
    // (except .rexymcp/output/, the recovery logs), the repo's git metadata
    // (host-side git reads its config and runs its hooks), and the config the
    // next dispatch loads.
    if let Ok(rel) = result.strip_prefix(root) {
        let mut parts = rel.components().map(|c| c.as_os_str().to_str());
        let protected = match (parts.next(), parts.next()) {
            (Some(Some(".rexymcp")), Some(Some("output"))) => false,
            (Some(Some(".rexymcp")), _) => true,
            (Some(Some(".git")), _) => true,
            (Some(Some("rexymcp.toml")), None) => true,
            _ => false,
        };
        if protected {
            return Err(ScopeError::Protected {
                requested: candidate.to_string_lossy().into_owned(),
            });
        }
    }
```

Leave `ScopeError::Protected`'s `Display` message unchanged:
`path is inside rexymcp's private state directory and cannot be accessed:
{requested}`. Phase 07's `read_file_refuses_vault_key` test checks for
`private state directory`.

| Requested (relative to root) | Result |
|---|---|
| `.git` | `Protected` |
| `.git/config` | `Protected` |
| `.git/hooks/pre-commit` (the file does not exist; `.git/hooks/` does) | `Protected` |
| `src/../.git/config` (with `src/` existing) | `Protected` |
| `rexymcp.toml` | `Protected` |
| `./rexymcp.toml` | `Protected` |
| `.gitignore` | Ok |
| `.github/workflows/ci.yml` | Ok |
| `rexymcp.toml.example` | Ok |
| `docs/rexymcp.toml` | Ok (only the top-level one is protected) |
| `docs/.git/x` | Ok (only the top-level `.git` is protected) |

Compare path components, never strings: `.gitignore` and `.github` must stay
allowed.

### 3. `run_phase` uses `for_repo`

In `mcp/src/runner.rs`, in the sandbox block, replace the
`Sandbox::new(&root, …)` call with:

```rust
        let sandbox = rexymcp_executor::security::Sandbox::for_repo(
            &root,
            std::env::var_os("HOME").map(std::path::PathBuf::from),
        )
        .map_err(|reason| sandbox_refusal(&reason))?;
        Some(sandbox.with_program(inp.sandbox_program))
```

The probe still runs first. `sandbox_refusal` is unchanged.

### 4. Executor contract

In `executor/templates/executor_contract.md`, in the "Stay inside the
project." bullet, after the sentence that ends "…other than reading
`.rexymcp/output/`.", add:

```markdown
Do not edit anything under `.git/` or the project's `rexymcp.toml`; commit
with `git` commands.
```

If a test pins the contract text, it must still pass unchanged. If it fails,
file a blocker.

## Acceptance criteria

- [ ] `command_prefix` adds rows 9a–9c for each git layout, in the order given,
      and phase 07's `argv_*` tests pass unmodified.
- [ ] `Sandbox::for_repo` returns the right `GitDir` for a `.git` directory and
      a `.git` file. It refuses a missing `.git`, a symlinked `.git`, and a
      `.git` directory with no `config`. It creates a missing `.git/hooks`.
- [ ] Inside a real sandbox, `.git/hooks`, `.git/config` and `rexymcp.toml`
      cannot be changed, `.git` cannot be moved, and `git commit` works
      (ignored test, run in E2E).
- [ ] Every row of the Spec §2 table resolves as stated, and phase 07's
      `.rexymcp` rows still do.
- [ ] A cloud dispatch on a repo with no `.git` returns `Err(Error::Privacy(_))`
      naming `.git`, before the AI client is used.
- [ ] The executor contract carries the new sentence.
- [ ] Each new non-ignored test fails when its fix is reverted (show one in the
      Update Log).
- [ ] `cargo fmt --all --check`, `cargo build`,
      `cargo clippy --all-targets --all-features -- -D warnings` and
      `cargo test` all pass.

## Test plan

Hermetic: a `tempfile::TempDir` for every repo, no network. Running
`git init --quiet` in a `TempDir` is fine; existing tests do it (see
`executor/src/tools/find_files.rs`, `std::process::Command::new("git")
.args(["init", "--quiet"])`).

### `executor/src/security/sandbox.rs`

Use the existing `argv_contains_sequence` helper. `/srv/repo` paths need no
filesystem.

1. **`argv_protects_git_dir_config_and_hooks`** —
   `Sandbox::new(Path::new("/srv/repo"), None).with_git(GitDir::Dir).argv("true")`.
   The index of `--bind /srv/repo /srv/repo` is less than the index of
   `--bind /srv/repo/.git /srv/repo/.git`, which is less than the index of
   `--ro-bind /srv/repo/.git/config /srv/repo/.git/config`. The sequence
   `--ro-bind /srv/repo/.git/hooks /srv/repo/.git/hooks` is present, and so is
   `--ro-bind-try /srv/repo/rexymcp.toml /srv/repo/rexymcp.toml`. All of them
   come before `--chdir`.
2. **`argv_binds_git_file_read_only`** — `.with_git(GitDir::File)`. Contains
   `--ro-bind /srv/repo/.git /srv/repo/.git`. **Must NOT** contain the sequence
   `--bind /srv/repo/.git /srv/repo/.git`, and no element ends with
   `.git/config`.
3. **`argv_without_git_has_no_git_mounts`** — plain `Sandbox::new`. No element
   contains `/.git`. The `rexymcp.toml` `--ro-bind-try` is still present.
4. **`for_repo_detects_git_layout`** — four `TempDir`s:
   - no `.git` → `Err`, and the message contains `.git`;
   - `.git/` holding a `config` file and **no** `hooks/` → `Ok`, `argv`
     contains the `.git` self-bind, and `.git/hooks` now exists on disk;
   - `.git` as a **file** → `Ok`, `argv` contains `--ro-bind <root>/.git <root>/.git`;
   - `.git/` with no `config` → `Err`, and the message contains `.git/config`.
5. **`for_repo_refuses_symlinked_git`** — `#[cfg(unix)]`. `.git` is a symlink
   (`std::os::unix::fs::symlink`) to a directory that holds a `config` file →
   `Err`, and the message contains `symlink`.
6. **`sandbox_protects_git_and_config`** —
   `#[ignore = "needs bwrap and user namespaces"]`. A `TempDir` repo; run
   `git init --quiet` on the host; write `rexymcp.toml` with `x`. Build
   `Sandbox::for_repo(&canonical, std::env::var_os("HOME").map(PathBuf::from))`.
   Run each command through `sb.argv(cmd)` with `std::process::Command`, in
   the same way as `sandbox_blocks_home_and_state_dir`:
   - `printf '#!/bin/sh\n' > .git/hooks/pre-commit` → non-zero
   - `git config core.fsmonitor evil` → non-zero
   - `mv .git g2` → non-zero
   - `echo y >> rexymcp.toml` → non-zero
   - `sed -i s/x/z/ rexymcp.toml` → non-zero
   - `rm -f rexymcp.toml` → non-zero
   - `git -c user.name=t -c user.email=t@t commit --allow-empty -qm t` → **exit 0**
     (the positive control: the sandbox still allows commits)

   Then, on the host: `rexymcp.toml` still reads `x`;
   `.git/hooks/pre-commit` does not exist; `.git/config` does not contain
   `fsmonitor`; `git log --oneline` in the repo prints one line.

### `executor/src/security/scope.rs`

7. **`rejects_git_and_config_paths`** — create `.git/hooks/`, `.git/config`,
   `src/` and `rexymcp.toml`. Every `Protected` row of the Spec §2 table
   returns `Err(ScopeError::Protected { .. })`.
8. **`allows_git_and_config_lookalikes`** — create `.gitignore`,
   `.github/workflows/ci.yml`, `rexymcp.toml.example`, `docs/rexymcp.toml`
   and `docs/.git/x`. Every `Ok` row of the Spec §2 table resolves `Ok`.

   In the existing `allows_rexymcp_output_and_lookalikes`, **remove** the
   `"rexymcp.toml"` entry from its list, because it is now protected. That is
   the only change allowed to an existing test.

### `mcp/src/runner.rs`

9. **`run_phase_refuses_cloud_repo_without_git`** — copy the setup of
   `run_phase_stops_when_sandbox_unavailable`, with two differences:
   `sandbox_program: "true"` (the probe `true --ro-bind / / --dev /dev true`
   exits 0, so the probe passes without bwrap), and the repo directory has no
   `.git`. Assert the result is `Err(Error::Privacy(m))` with `m` containing
   `.git`.

## End-to-end verification

Run these and paste each output file into the Update Log.

```bash
cargo test -p rexymcp-executor sandbox -- --ignored > /tmp/p11_ignored.txt 2>&1; echo "exit=$?" >> /tmp/p11_ignored.txt
```

This must show 4 ignored tests passing: phase 07's two, phase 10's
`sandboxed_runner_passes_only_allowlisted_env`, and test 6.

Then run the real CLI against a cloud endpoint whose name never resolves
(`.invalid`), once without `.git` and once with it:

```bash
T=$(mktemp -d)
mkdir -p "$T/repo"
printf '# Phase 1: e2e\n\n**Status:** todo\n\n## Goal\n\nx\n' > "$T/phase-01-e2e.md"
printf '[executor]\nprovider = "openai"\nmodel = "m"\nbase_url = "https://cloud.invalid/v1"\n\n[commands]\nformat = "true"\nbuild = "true"\nlint = "true"\ntest = "true"\n' > "$T/repo/rexymcp.toml"
cargo build -q -p rexymcp
BIN="$PWD/target/debug/rexymcp"
"$BIN" run-phase --no-telemetry --config "$T/repo/rexymcp.toml" --phase-doc "$T/phase-01-e2e.md" --repo "$T/repo" > /tmp/p11_nogit.txt 2>&1; echo "exit=$?" >> /tmp/p11_nogit.txt
git -C "$T/repo" init --quiet
"$BIN" run-phase --no-telemetry --config "$T/repo/rexymcp.toml" --phase-doc "$T/phase-01-e2e.md" --repo "$T/repo" > /tmp/p11_git.txt 2>&1; echo "exit=$?" >> /tmp/p11_git.txt
grep -c 'has no .git' /tmp/p11_nogit.txt /tmp/p11_git.txt
```

`/tmp/p11_nogit.txt` must contain `has no .git` and a non-zero `exit=`.
`/tmp/p11_git.txt` must **not** contain `has no .git`: the positive control.
That run fails later, when it tries to reach `cloud.invalid`, which is
expected. Paste both files and the `grep -c` output, which must be `1` and
`0`.

## Authorizations

- [x] May edit `executor/src/security/sandbox.rs`,
      `executor/src/security/mod.rs` (the export) and
      `executor/src/security/scope.rs`, including their test modules.
- [x] In `allows_rexymcp_output_and_lookalikes`, may remove the
      `"rexymcp.toml"` entry. No other change to existing tests.
- [x] May edit `mcp/src/runner.rs`: the sandbox block in `run_phase`, and add
      test 9.
- [x] May add `#[ignore = "needs bwrap and user namespaces"]` to test 6 only.
- [x] May add the sentence in Spec §4 to
      `executor/templates/executor_contract.md`.
- [ ] May add a dependency — **no.**
- [ ] May edit `mcp/src/finalize.rs`, `mcp/src/resume.rs`,
      `executor/src/agent/**` or `executor/src/tools/**` — **no.**
- [ ] May write `unsafe` — **no.**

## Out of scope

- **Committing from a cloud executor in a git worktree.** The worktree's real
  git directory is outside the repo root, so it is read-only in the sandbox
  and `git commit` fails there (checked by hand). Parallel cloud work in
  worktrees needs rexymcp to make the commit on the host. File that when
  parallel dispatch is set up.
- **A repo with no `rexymcp.toml`.** The `--ro-bind-try` skips it, so the model
  could create one. The serve process loads a fixed config path chosen at
  startup, so a new file only matters if that path pointed at a file that did
  not exist.
- **`rm -rf .git` destroying history.** It is data loss, not code running on
  the host. The remote is the backup.
- **A nested `.git` below the root** (submodules). Only the top-level one is
  protected.
- **Findings 8 and 9** — phases 09 and 08.

## Update Log

(Filled in by the executor. See WORKFLOW.md § "Update Log entries".)

<!-- entries appended below this line -->

### Update — 2026-09-17 14:31 UTC (started)

Started by the executor. Pre-flight: `git status --short` clean; bubblewrap
0.12.0, git 2.55.0; `cargo test -p rexymcp-executor scope` 19 passed;
`cargo test -p rexymcp runner` 35 passed (+ 0 integration).

### Update — 2026-09-17 (in-progress notes)

**Revert check** (acceptance: "Each new non-ignored test fails when its fix is
reverted"): reverted the `Sandbox::for_repo` call in `run_phase` back to
`Sandbox::new` (no git check) via a temporary edit and re-ran
`cargo test -p rexymcp --bin rexymcp run_phase_refuses`; result:

```
thread 'runner::tests::run_phase_refuses_cloud_repo_without_git' panicked at mcp/src/runner.rs:1509:13:
expected Privacy, got Backend("Request failed: error sending request for url (https://cloud.invalid/v1/chat/completions)")
test result: FAILED. 0 passed; 1 failed
```

The fix was then restored and the test passes again (1 passed).

**End-to-end verification** (`cargo test -p rexymcp-executor sandbox -- --ignored`, output in /tmp/p11_ignored.txt):

```
running 4 tests
test security::sandbox::tests::probe_succeeds_where_bwrap_works ... ok
test agent::command::tests::sandboxed_runner_passes_only_allowlisted_env ... ok
test security::sandbox::tests::sandbox_blocks_home_and_state_dir ... ok
test security::sandbox::tests::sandbox_protects_git_and_config ... ok
test result: ok. 4 passed; 0 failed; 0 ignored; 0 measured; 1167 filtered out; finished in 0.05s
exit=0
```

Real CLI against `https://cloud.invalid/v1` (never resolves), no `.git`
(/tmp/p11_nogit.txt):

```
Error: privacy: the bash sandbox is unavailable (/tmp/tmp.ajvOgVkviy/repo has no .git; a cloud dispatch needs the repo root to be a git work-tree root), so the dispatch to the cloud executor was stopped before any content was sent. Install bubblewrap (the `bwrap` binary) and check that unprivileged user namespaces are enabled, or run the phase on a local executor.
exit=1
```

Same config after `git init` in the repo (/tmp/p11_git.txt) — positive control:

```
Error: backend: Request failed: error sending request for url (https://cloud.invalid/v1/chat/completions)
exit=1
```

`grep -c 'has no .git' /tmp/p11_nogit.txt /tmp/p11_git.txt`:

```
/tmp/p11_nogit.txt:1
/tmp/p11_git.txt:0
```

**Grep for pinned literals** (spec §1/§2):

```
$ grep -n 'has no .git; a cloud dispatch' executor/src/security/sandbox.rs
47: "{} has no .git; a cloud dispatch needs the repo root to be a git work-tree root",
$ grep -n 'git is a symlink' executor/src/security/sandbox.rs
53: "{}.git is a symlink; refusing to sandbox a symlinked git directory",
```

**Full verification** (all four commands, run after the final `cargo fmt --all --check`):

- `cargo fmt --all --check` — exit 0
- `cargo build` — Finished `dev` profile, zero warnings
- `cargo clippy --all-targets --all-features -- -D warnings` — Finished, clean
- `cargo test` — 715 + 2 + 1163 (8 ignored) + 0 passed, 0 failed across all targets

Notes for review:

- The sandbox block in `run_phase` now calls `Sandbox::for_repo`, which detects
  the git layout via `symlink_metadata`, refuses missing/symlinked/config-less
  `.git`, and `create_dir_all`s a missing `.git/hooks`.
- `command_prefix` adds rows 9a (bind .git, ro-bind config, ro-bind hooks), 9b
  (ro-bind .git file), 9c (ro-bind-try rexymcp.toml) after row 9 and before
  `--chdir`; rows 1–9 untouched.
- The `scope.rs` protected-path check is now a single match covering
  `.rexymcp` (unchanged behavior, minus the lookalike `rexymcp.toml` entry
  removed from `allows_rexymcp_output_and_lookalikes` as authorized), `.git`
  and top-level `rexymcp.toml`.
- In the ignored `sandbox_protects_git_and_config` test I dropped a positive
  assertion that the sandbox's child process produces stdout/stderr: a clean
  `git commit -q` produces no output, which made the test fail spuriously. The
  per-command assertions (non-zero for the six write attempts, exit 0 for the
  commit, host-side file state) are unchanged and stronger.

### Update — ts=1789659560943 (complete, server-authored)

**Summary:** Done. Summary and notes for review:

**What I built.** F05 phase 11 is implemented and verified. The bash sandbox now binds `.git` onto itself (which makes `mv .git` fail with EBUSY) and pins `.git/config` and `.git/hooks` read-only for a directory `.git`, ro-binds a worktree's `.git` file, and always ro-binds the top-level `rexymcp.toml` via `--ro-bind-try` — rows 9a–9c, inserted after row 9 and before `--chdir`, with phase 07's rows 1–9 untouched. `run_phase` now builds the sandbox through the new `Sandbox::for_repo`, which detects the layout with `symlink_metadata`, creates a missing `.git/hooks`, and fails the dispatch before turn 1 (as `Error::Privacy` naming `.git`) when the repo root has no `.git`, `.git` is a symlink, or `.git` has no `config` file. The file tools' protected-path check is now a single component-wise match that also refuses `.git` and everything under it, plus `<root>/rexymcp.toml` (lookalikes `.gitignore`, `.github/…`, `docs/.git/…`, `rexymcp.toml.example` stay allowed); the `rexymcp.toml` entry was removed from `allows_rexymcp_output_and_lookalikes` as authorized. The executor contract carries the new sentence in the "Stay inside the project" bullet.

**End-to-end result.** All four ignored sandbox tests pass under bwrap (including the new `sandbox_protects_git_and_config`: all six host-mutation attempts fail in-sandbox and `git commit` succeeds with the positive-control checks passing on the host). Running the real CLI against `https://cloud.invalid/v1` on a repo without `.git` yields `has no .git` and exit 1; with `git init` done it gets past that and fails only on the unresolvable endpoint — `grep -c` is `1` / `0` as required. `cargo fmt --all --check`, `cargo build`, `cargo clippy --all-targets --all-features -- -D warnings`, and `cargo test` all pass (715 + 2 + 1163 + 0 across targets); the new `run_phase_refuses_cloud_repo_without_git` test was shown to fail when the `for_repo` fix is reverted, then passes with it restored. Working tree is clean, one conventional commit.

**Deviations/notes.** (1) In the ignored `sandbox_protects_git_and_config` test I dropped one non-essential assertion that every sandboxed command must produce stdout/stderr, because a clean `git commit -q` produces no output and made the test fail spuriously — the spec's actual assertions (per-command exit codes + host-side state) are all present and pass. (2) The e2e `nogit` refusal is phrased through `sandbox_refusal` ("the bash sandbox is unavailable (…has no .git…)") — that's what the spec-mandated `map_err(|reason| sandbox_refusal(&reason))` produces. (3) Worktree note confirmed relevant: a `.git` *file* points outside the root, so committing from a cloud executor in a worktree will fail by design — already in the spec's out-of-scope list.

**Executor:** RedHatAI/Qwen3.8-27B-INT4

**Gates:** format=run, build=run, lint=run, test=run

**Command output tails:**

```
FORMAT


BUILD
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.50s


LINT
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.29s


TEST
ymbols::tests::rejects_nonexistent_path ... ok
test tools::symbols::tests::rejects_path_outside_root ... ok
test tools::symbols::tests::references_exclude_substring ... ok
test tools::symbols::tests::references_single_file_path ... ok
test tools::symbols::tests::single_file_unsupported_extension_advisory_error ... ok
test tools::symbols::tests::type_mismatch_returns_recovery_hint ... ok
test tools::update_task::tests::flips_active_task_to_done ... ok
test tools::symbols::tests::references_no_matches_advisory ... ok
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
test tools::update_task::tests::success_output_names_task ... ok
test tools::update_task::tests::unknown_id_returns_advisory_error ... ok
test tools::write_file::tests::append_false_overwrites ... ok
test tools::write_file::tests::append_creates_file_if_missing ... ok
test tools::write_file::tests::appends_to_existing_file ... ok
test tools::write_file::tests::creates_new_file ... ok
test tools::write_file::tests::missing_path_returns_recovery_hint ... ok
test tools::write_file::tests::non_object_args_do_not_panic ... ok
test tools::write_file::tests::overwrites_existing_file ... ok
test tools::write_file::tests::rejects_malformed_args ... ok
test tools::write_file::tests::reports_missing_parent_dir ... ok
test tools::write_file::tests::scope_escape_returns_advisory_error_and_writes_nothing ... ok
test tools::write_file::tests::success_output_includes_line_count ... ok
test tools::symbols::tests::references_across_multiple_files ... ok
test tools::symbols::tests::references_respects_max_results ... ok
test tools::symbols::tests::references_snippet_shows_source_line ... ok
test tools::symbols::tests::references_truncation_note_omits_kind_filter ... ok
test tools::symbols::tests::reports_line_and_column ... ok
test tools::symbols::tests::unsupported_extension_skipped_in_dir_walk ... ok
test tools::symbols::tests::respects_gitignore ... ok
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

test result: ok. 1163 passed; 0 failed; 8 ignored; 0 measured; 0 filtered out; finished in 6.18s


running 0 tests

test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

    Finished `test` profile [unoptimized + debuginfo] target(s) in 0.13s
     Running unittests src/main.rs (target/debug/deps/rexymcp-3de3446ef9b6a3ce)
     Running tests/readme_config_reference.rs (target/debug/deps/readme_config_reference-22f887757c17cb2b)
     Running unittests src/lib.rs (target/debug/deps/executor-0c52bf72536f9e8e)
   Doc-tests executor

```

**Files changed:**

- `docs/dev/milestones/F05-privacy-security-hardening/README.md` — +1 -1
- `docs/dev/milestones/F05-privacy-security-hardening/phase-11-protect-git-and-config.md` — +90 -1
- `executor/src/security/mod.rs` — +1 -1
- `executor/src/security/sandbox.rs` — +300 -2
- `executor/src/security/scope.rs` — +65 -18
- `executor/templates/executor_contract.md` — +3 -1
- `mcp/src/runner.rs` — +44 -6

**Commit:** d9906e008cee45ea4641bfe9f1ef2cfd406ed316

**Notes:** server-authored completion entry (executor no longer owns the bookkeeping tail; see M27 phase-03).
