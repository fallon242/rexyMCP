# Phase 01: Observe the serve loop's exit

**Milestone:** M41 — Serve Liveness & Run Durability
**Status:** todo
**Depends on:** none (first phase of M41)
**Estimated diff:** ~70 lines (a ~30-line `main.rs` change, a small `JobRegistry`
accessor, and tests)
**Tags:** language=rust, kind=bugfix, size=s

## Goal

`rexymcp serve` currently keeps running — silently, at 0 % CPU, holding the client
connection open — after its MCP service loop has died. Make `main` **wait on the
service loop instead of on `ctrl_c`**, so the process logs why the loop quit and
then exits, turning a permanent invisible wedge into a visible, recoverable
failure.

This phase does not fix what killed the loop (phase 02) and does not make the lost
run result recoverable (phase 03). It is the safety net that makes both of those,
and any future transport failure, diagnosable at all.

## Architecture references

Read before starting:

- `docs/dev/milestones/M41-serve-liveness/README.md` — the milestone, including the
  stack-dump forensics this phase acts on. Read § "What the forensics prove".
- `docs/architecture.md` § Status #30 — the async job model (`run_id`,
  `get_run_status`) whose reporting path this outage broke.

## Pre-flight

1. Read `docs/dev/STANDARDS.md` top to bottom.
2. Read the architecture references above.
3. Read this entire phase doc before touching any code.
4. Confirm the repo is on a clean branch with no uncommitted changes.

## Current state

The `Serve` arm of the CLI dispatch, `mcp/src/main.rs:597-605`, verbatim:

```rust
let server = server::RexyMcpServer::new(config);
let transport = rmcp::transport::stdio();
let _running = rmcp::serve_server(server, transport)
    .await
    .map_err(|e| anyhow::anyhow!("MCP server failed: {}", e))?;
tokio::signal::ctrl_c()
    .await
    .map_err(|e| anyhow::anyhow!("failed to wait for signal: {}", e))?;
Ok(())
```

`rmcp::serve_server` returns a `RunningService` whose event loop runs in a
**spawned task**, not in the returned future. Binding it to `_running` keeps it
alive (a named binding, so no immediate drop) but nothing ever polls its
completion. When the loop breaks — on stdin EOF, a stdin read error, or an
over-long line — the task ends and `main` stays parked in `ctrl_c` forever. The
attached `gdb` stack from the incident shows exactly that: `Runtime::block_on`
called from `rexymcp::main`, with no other rexymcp frame in the process.

The API you need (`rmcp` 2.2.0, already a dependency):

```rust
// rmcp::service::RunningService
pub async fn waiting(mut self) -> Result<QuitReason, tokio::task::JoinError>

// rmcp::service::QuitReason  — #[non_exhaustive]
pub enum QuitReason { Cancelled, Closed, JoinError(tokio::task::JoinError) }
```

`QuitReason` is **not** re-exported at the crate root; the path is
`rmcp::service::QuitReason`. It is `#[non_exhaustive]`, so any `match` on it needs
a catch-all arm.

The eprintln! logging convention this arm already uses is
`"rexymcp serve: <message>"` on stderr (`main.rs:549-555`, `:566-594`) — stderr is
the only safe stream, since stdout is the JSON-RPC transport.

`RexyMcpServer.runs` is a `pub std::sync::Arc<JobRegistry>` (`mcp/src/server.rs:129-141`),
so the registry can be cloned out of the server value **before** it is moved into
`serve_server`.

## Spec

### 1. Add a non-terminal run count to `JobRegistry`

In `mcp/src/jobs.rs`, add a method next to `is_running` (`jobs.rs:134-139`):

```rust
/// How many registered runs are still non-terminal. Read at serve shutdown to
/// tell a clean client disconnect from a loop death that stranded live work.
pub fn running_count(&self) -> usize
```

Implement it over the same lock + `state_tx.borrow().is_terminal()` predicate
`is_running` uses. Do not change `is_running`.

### 2. Wait on the service loop, not on `ctrl_c`

Rewrite `mcp/src/main.rs:597-605` so that:

- The registry is cloned out before the server is moved:
  `let server = server::RexyMcpServer::new(config); let runs = server.runs.clone();`
- `serve_server(...).await` binds to a **named, used** value (`running`), not `_running`.
- The process then waits on `tokio::select!` over two arms:
  - `running.waiting()` — the service loop finished on its own.
  - `tokio::signal::ctrl_c()` — the human interrupted. Keep this arm; Ctrl-C must
    still exit cleanly. Its current error handling (map to `anyhow`) is fine.

`waiting(self)` consumes the `RunningService`, which is what `select!` needs — pass
the future directly as the arm's expression.

### 3. Log the quit reason, then exit

On the `waiting()` arm, emit **one** stderr line before returning, in the existing
`rexymcp serve: …` style, carrying three facts:

- the `QuitReason` (`{:?}` on the enum is acceptable — `JoinError` prints its own
  detail),
- `runs.running_count()` at that moment,
- a fixed hint string naming the two known causes, so the next person to hit this
  does not need `gdb`. Use exactly this wording for the hint so it is greppable:
  `"stdin EOF or a read error on the MCP transport (see M41)"`.

Then map the outcome to a process result:

- `Ok(QuitReason::Closed)` with `running_count() == 0` → return `Ok(())`. This is
  the normal shutdown: the client closed the pipe with no work outstanding.
- `Ok(QuitReason::Closed)` with `running_count() > 0` → return an `anyhow` error.
  The loop died while runs were live; their results are now unreachable, and that
  is a failure the operator must see in the exit status.
- `Ok(QuitReason::Cancelled)` → return `Ok(())` (a deliberate shutdown).
- `Ok(other)` (i.e. `JoinError(_)`, plus any future variant — the enum is
  `#[non_exhaustive]`) → return an `anyhow` error naming the reason.
- `Err(join_error)` → return an `anyhow` error naming the join failure.

Return the errors with `?`/`Err(...)` through the existing `anyhow::Result` return
type. **Do not** call `std::process::exit` — `main` already propagates an error to a
non-zero status, and an explicit exit would skip the auto-sweep task's teardown.

### 4. Nothing else changes

Do not touch the auto-sweep block (`main.rs:557-595`), the startup banner, the
`RexyMcpServer` type, or any handler. Do not add a watchdog, a timer, or a
heartbeat — explicitly out of scope (README § Notes).

## Acceptance criteria

- [ ] `cargo build` is green.
- [ ] `cargo clippy --all-targets --all-features -- -D warnings` is clean.
- [ ] `cargo fmt --all --check` reports no diff in the files this phase touched.
- [ ] `cargo test` passes.
- [ ] `mcp/src/main.rs` contains no binding named `_running`, and the `Serve` arm
      awaits `running.waiting()`.
- [ ] `JobRegistry::running_count` returns the number of non-terminal runs.
- [ ] Driving a real `serve` process to stdin EOF makes it print one
      `rexymcp serve: …` line naming the quit reason and **exit** (see E2E below).
- [ ] Ctrl-C still shuts the server down cleanly.

## Test plan

Unit tests for the piece that is hermetically testable — the run count — in the
existing `#[cfg(test)] mod tests` block in `mcp/src/jobs.rs`, matching the style of
`is_running_true_for_running_false_after_publish`:

- `running_count_counts_only_non_terminal_runs` in `mcp/src/jobs.rs` — insert three
  runs, publish a terminal state for one, assert `running_count() == 2`.
- `running_count_is_zero_on_empty_registry` in `mcp/src/jobs.rs` — asserts `0`.
- `running_count_drops_to_zero_when_all_publish` in `mcp/src/jobs.rs` — insert two,
  publish both terminal, assert `0`. This is the case the shutdown branch keys on,
  so pin it directly.

**Do not** write a unit test that spawns a `serve` process, drives its stdin, or
binds stdio — that is host state outside a `TempDir` and violates hermeticity. The
serve-arm behavior is proven in the E2E section instead, which is the reviewer- and
executor-visible artifact for this phase.

## End-to-end verification

The `serve` arm is a real runtime-loadable artifact, so verify against it. Both
checks use a `TempDir`-independent, network-free invocation and must be quoted in
your Update Log.

1. **Clean EOF exits the process.** With a built binary, close stdin immediately:

   ```
   cargo build 2>&1 | tail -3
   echo -n "" | ./target/debug/rexymcp serve --config rexymcp.toml; echo "exit=$?"
   ```

   Expect: the startup banner, then the new quit line naming the reason, then
   `exit=0` — the process must **not** hang. Quote the full output. (Before this
   phase, the same command hangs forever; if you want the contrast for your log,
   run it against a stashed build, but do not commit that.)

2. **A well-formed request still gets a response.** Confirm the change did not
   break normal serving — pipe one `initialize` request and confirm a JSON-RPC
   response comes back on stdout before the EOF-driven exit:

   ```
   printf '%s\n' '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"e2e","version":"0"}}}' \
     | ./target/debug/rexymcp serve --config rexymcp.toml 2>/dev/null | head -c 400
   ```

   Expect a `"result"` object carrying `serverInfo`. Quote it.

If step 2's response does not appear, **stop and file a blocker** — that means the
`select!` is consuming the transport rather than the loop, and shipping it would
break every client.

## Authorizations

None. No new dependencies (`rmcp` is already in `mcp/Cargo.toml`). No edits to
`Cargo.toml`, `docs/architecture.md`, or any phase doc other than this one. Files
you may edit: `mcp/src/main.rs`, `mcp/src/jobs.rs`.

## Out of scope

- **Fixing what killed the transport** — that is phase 02. Do not touch
  `executor/src/tools/bash.rs`.
- **Making the lost run result recoverable** — that is phase 03. Do not add any
  on-disk persistence here.
- **A watchdog / liveness prober / heartbeat.** Explicitly rejected in the
  milestone README; waiting on `waiting()` is the whole mechanism.
- **A single-instance guard.** Separate bug, separate milestone.
- **Changing `await_terminal`'s bound** (`jobs.rs:88-103`). It is already correct;
  it was simply never reached. Touching it is a wrong turn.
- Restructuring the auto-sweep block or the startup logging.

## Update Log

(Filled in by the executor. See WORKFLOW.md § "Update Log entries".)

<!-- entries appended below this line -->
