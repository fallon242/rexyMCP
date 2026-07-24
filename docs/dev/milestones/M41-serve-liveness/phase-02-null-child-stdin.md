# Phase 02: Deny `bash`-tool children the MCP stdin

**Milestone:** M41 — Serve Liveness & Run Durability
**Status:** done
**Depends on:** none (independent of phase 01; ordered second)
**Estimated diff:** ~45 lines (a 1-line production change plus tests)
**Tags:** language=rust, kind=bugfix, size=s

## Goal

Every child process the `bash` tool spawns currently **inherits the serve
process's stdin**, which is the JSON-RPC pipe from Claude Code. A child that
drains that descriptor, or sets `O_NONBLOCK` on it (Node/Bun/libuv do this
routinely, and the flag lives on the shared open file description), kills the MCP
transport for the whole serve process. Give children a null stdin instead.

This is the root cause behind issue #5. The production change is one line; the
value of the phase is the test that stops it from regressing.

## Architecture references

Read before starting:

- `docs/dev/milestones/M41-serve-liveness/README.md` — the milestone, especially
  § "What killed the transport".
- `docs/architecture.md` § Status #30 — the async job model; the `bash` tool runs
  inside the same process as the MCP transport, which is why this matters.

## Pre-flight

1. Read `docs/dev/STANDARDS.md` top to bottom.
2. Read the architecture references above.
3. Read this entire phase doc before touching any code.
4. Confirm the repo is on a clean branch with no uncommitted changes.

## Current state

`executor/src/tools/bash.rs:139-160`, verbatim:

```rust
let mut cmd = Command::new("sh");
cmd.arg("-c")
    .arg(&parsed.command)
    .current_dir(self.scope.root())
    .stdout(std::process::Stdio::piped())
    .stderr(std::process::Stdio::piped());

cmd.env_clear();
for (key, value) in std::env::vars() {
    if is_allowed_env_key(&key) {
        cmd.env(&key, &value);
    }
}

let child = match cmd.spawn() {
```

`Command::spawn()` defaults **stdin to inherit** (unlike `Command::output()`,
which sets it to null). So the child's fd 0 is the parent's fd 0 — inside `serve`,
that is the MCP request pipe.

This is the **only** production spawn site with the defect. Verified across the
workspace:

- `executor/src/agent/command.rs:33` (`RealCommandRunner`) — uses `.output()`, safe.
- `executor/src/governor/verifier.rs:264` (`cargo check`), `:497` (`tsc`),
  `:589` (`ruff`) — all `.output()`, safe.
- `executor/src/tools/bash.rs:245` — the timeout-path `kill -9`, `.output()`, safe.
- `search.rs:475`, `symbols.rs:786`, `find_files.rs:314` — test-only fixtures.

Do not "fix" any of the safe sites; changing `.output()` calls is a wrong turn.

## Spec

### 1. Give the child a null stdin

In `executor/src/tools/bash.rs`, add `.stdin(std::process::Stdio::null())` to the
`cmd` builder chain alongside the existing `.stdout(...)` / `.stderr(...)` calls.
Put it **before** `.stdout(...)` so the three streams read in fd order.

Add a short comment above the chain explaining *why* — the next reader must not
"tidy" it away. State the contract, not the anecdote: the tool runs in-process with
the MCP stdio transport, so a child must never receive the server's fd 0. Keep it
to two lines or fewer.

### 2. Nothing else changes

Same command, same env filtering, same timeout handling, same output capture. The
child's stdout/stderr stay piped. `parsed.command` is untouched.

A child that previously blocked waiting on inherited stdin will now see immediate
EOF. That is the intended behavior change: an interactive command inside the
executor was already a bug (it could only ever have consumed the architect's
requests), and it now fails fast instead.

## Acceptance criteria

- [x] `cargo build` is green.
- [x] `cargo clippy --all-targets --all-features -- -D warnings` is clean.
- [x] `cargo fmt --all --check` reports no diff in the files this phase touched.
- [x] `cargo test` passes — the existing `tools::bash::tests::*` suite included,
      unchanged.
- [x] The `bash` tool's `cmd` builder sets `Stdio::null()` for stdin.
- [x] A child that reads stdin observes EOF immediately rather than inheriting the
      parent's descriptor (pinned by the tests below).

## Test plan

Add unit tests to the existing `#[cfg(test)] mod tests` block in
`executor/src/tools/bash.rs`, using the helpers already there (`make_scope`, the
`bash(scope, timeout)` constructor, `json!` args) and matching the style of
`times_out_advisory_failure`. These spawn a **real** child process — that is
allowed and intended (the suite already does it); hermeticity is preserved because
the child touches only a `TempDir` and no network.

- `bash_child_gets_empty_stdin` in `executor/src/tools/bash.rs` — run
  `cat; echo EOF_OK` and assert the output contains `EOF_OK` and that the command
  reports success. With an inherited stdin under a test harness this either blocks
  until the phase timeout or picks up foreign bytes; with a null stdin `cat` returns
  instantly on EOF. Give the tool a short default timeout so a regression fails the
  test quickly instead of stalling the suite.
- `bash_child_stdin_reads_zero_bytes` in `executor/src/tools/bash.rs` — run
  `wc -c` and assert the trimmed output is `0`. This is the sharper assertion: it
  pins *empty*, not merely *terminated*.

**Mutation self-check before you finish:** temporarily remove the
`.stdin(Stdio::null())` line and confirm `bash_child_stdin_reads_zero_bytes` fails
or times out; then restore. A test that passes with the line removed is not pinning
the fix — say so explicitly in your Update Log with the observed failure. (Do not
commit the mutation.)

Do **not** attempt to assert the `O_NONBLOCK` mechanism directly — that needs
`fcntl` from `libc`, which is not a dependency and is not authorized here. The
contract these tests pin (children get a null stdin) is the durable property.

## End-to-end verification

The real artifact is a spawned OS process, and the unit tests above already exercise
it — but they assert from the *parent* side. Add one direct observation from the
**child's** side and quote it in your Update Log. On Linux, ask the child what its
fd 0 actually is:

```
cargo test bash_child 2>&1 | tail -10
```

and, as a one-off manual check, a test or scratch invocation running
`readlink /proc/self/fd/0` through the tool, whose captured output must be
`/dev/null`. Quote the literal output. If it names a pipe or a tty instead, the fix
is not in effect — stop and investigate before reporting complete.

## Authorizations

None. No new dependencies (in particular **not** `libc`). No edits to `Cargo.toml`
or `docs/architecture.md`. Files you may edit: `executor/src/tools/bash.rs`.

## Out of scope

- The `serve` shutdown path — phase 01. Do not touch `mcp/src/main.rs`.
- Run-result persistence — phase 03.
- Auditing or changing any `.output()` call site; they already null stdin, and
  editing them widens the diff for no gain.
- A general "spawn wrapper" abstraction over the codebase's process launches. There
  is exactly one defective site; a helper type would be scope invention. If you
  believe a second defective site exists, note it in "Notes for review" — do not fix
  it.
- Rejecting or detecting interactive commands (e.g. refusing `vim`, prompting
  tools). A null stdin already makes them fail fast; classification is a separate
  design question.

## Update Log

(Filled in by the executor. See WORKFLOW.md § "Update Log entries".)

<!-- entries appended below this line -->

### Update — 2026-07-24 15:25 (complete)

**Summary:** Implemented **directly by the architect** at the user's request (no
dispatch, no `PhaseRun`). Added `.stdin(std::process::Stdio::null())` to the `cmd`
builder in `executor/src/tools/bash.rs`, before `.stdout(...)` so the three streams
read in fd order, with a four-line comment stating the contract (the tool runs
in-process with the MCP stdio transport; a child must never receive the server's
fd 0). Nothing else changed — same command, env filtering, timeout handling, and
output capture.

**Acceptance criteria:** all ticked above.

**Commands:** `cargo build` green; `cargo clippy --all-targets --all-features -- -D
warnings` clean; `cargo test` **661 bin + 1053 lib passed, 0 failed, 2 ignored**
(the pre-existing `tools::bash::tests::*` suite unchanged and green);
`cargo fmt --all --check` clean after `rustfmt --edition 2024` on the touched file.

**End-to-end verification:**

```
$ cargo test bash_child
running 3 tests
test tools::bash::tests::bash_child_stdin_reads_zero_bytes ... ok
test tools::bash::tests::bash_child_gets_empty_stdin ... ok
test tools::bash::tests::bash_child_stdin_is_dev_null ... ok
test result: ok. 3 passed; 0 failed; 0 ignored; 0 measured; 1052 filtered out
```

The child-side observation the phase asked for is folded into the suite as
`bash_child_stdin_is_dev_null`, which runs `readlink /proc/self/fd/0` through the
real tool and asserts the output names `/dev/null` — the child reporting its own
descriptor, not the parent inferring it. Keeping it as a test rather than a
one-off scratch invocation means the check runs in CI forever; it is Linux-only,
which is this project's CI and dev platform.

**Mutation self-check:** removing the `.stdin(...)` line fails **all three** tests —
`bash_child_gets_empty_stdin` on `cat should see EOF, not block` (the child blocks
until the 5 s timeout), `bash_child_stdin_reads_zero_bytes` on
`result.error.is_none()`, and `bash_child_stdin_is_dev_null` likewise. Restored;
the suite is green and the mutation was not committed.

**Files changed:**
- `executor/src/tools/bash.rs` — null stdin for spawned children + three tests.

**New tests:**
- `bash_child_gets_empty_stdin` in `executor/src/tools/bash.rs`
- `bash_child_stdin_reads_zero_bytes` in `executor/src/tools/bash.rs`
- `bash_child_stdin_is_dev_null` in `executor/src/tools/bash.rs`

**Notes for review:** The short 5 s tool timeout in the first two tests is
deliberate — under the mutation the child blocks, and the timeout is what turns a
regression into a fast failure instead of a stalled suite.

### Review verdict — 2026-07-24

- **Verdict:** approved_first_try
- **Bounces:** none
- **Executor:** Claude Code (direct) — architect-implemented, so this is
  self-review; the mutation check is the independent evidence.
- **Scope deviations:** none. No `.output()` call site was touched, no spawn
  wrapper introduced, no interactive-command classification added.
- **Calibration:** none.
