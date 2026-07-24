# M41 — Serve Liveness & Run Durability

**Goal:** Make a dead MCP serve loop **loud and fatal** instead of silent and
permanent, stop child processes from being handed the MCP stdin they can kill the
transport with, and make a completed run's result reapable after the serve process
goes away.

**Status:** planning *(opened 2026-07-24)*

**Depends on:** M30 (the async `execute_phase` job model — `JobRegistry`,
`get_run_status`, `stop_phase`), M27 (the `/rexymcp:auto` loop that polls
`get_run_status` and is the reporter of this outage).

## Why this milestone exists

GitHub issue [#5](https://github.com/ryanczak/rexyMCP/issues/5): after a
dispatched phase reached terminal state, `rexymcp serve` (0.9.1) stopped answering
**every** MCP request — permanently — while staying alive and looking healthy. The
phase itself succeeded (4/4 tasks, gates green, committed); only the reporting path
was dead. The `/rexymcp:auto` loop polled a finished phase for ~11.7 minutes and
then hung past every client timeout, and recovery required a human noticing and
killing the process.

The reporter attached `gdb` and `eu-stack` dumps taken with the process wedged.
Those dumps are what make this milestone precise rather than speculative.

### What the forensics prove

Thread census of the wedged process (33 threads total):

- 31 tokio workers **parked idle** in `park_condvar` — not starved, not blocked.
- 1 IO/time driver in `epoll_wait` — normal.
- 1 main thread parked in `Runtime::block_on`, called from `rexymcp::main`.
- **`rexymcp::main` is the only rexymcp frame in the entire process.**
- ~0 s cumulative CPU over an hour; no socket to the LLM endpoint (which answered
  in ~1 ms throughout).

Two deductions follow, and they are the whole milestone:

**1. The serve loop has exited, not stalled.** `tokio::io::Stdin` occupies a
blocking thread only *while a read is in flight*. No thread is blocked in `read()`
on fd 0, so nothing is reading stdin. `rmcp`'s stdio transport terminates the
service loop on exactly three inputs
(`rmcp-2.2.0/src/transport/async_rw.rs:140-145`): EOF (`Ok(0)`), a **read error**,
or `MaxLineLengthExceeded`. Any of them returns `None` from `transport.receive()`,
which breaks the loop with `QuitReason::Closed`
(`rmcp-2.2.0/src/service.rs:1030-1036`) and ends the spawned service task.
Malformed JSON is explicitly **not** fatal there — it is logged and skipped — so
this was a hard transport termination.

**2. `main` never observes that exit** (`mcp/src/main.rs:597-605`):

```rust
let server = server::RexyMcpServer::new(config);
let transport = rmcp::transport::stdio();
let _running = rmcp::serve_server(server, transport)
    .await
    .map_err(|e| anyhow::anyhow!("MCP server failed: {}", e))?;
tokio::signal::ctrl_c()
    .await
    .map_err(|e| anyhow::anyhow!("failed to wait for signal: {}", e))?;
```

`serve_server` returns a `RunningService` whose event loop runs in a **spawned
task**. We never call `running.waiting()`. When that task quits, `main` keeps
awaiting `ctrl_c` forever — process alive, 0 % CPU, no rexymcp frame anywhere,
client connection held open. That is a byte-for-byte match to the attached stack
(`#5 Runtime::block_on` → `#6 rexymcp::main`).

### What killed the transport

`executor/src/tools/bash.rs:139-144` spawns every shell child without configuring
stdin:

```rust
let mut cmd = Command::new("sh");
cmd.arg("-c")
    .arg(&parsed.command)
    .current_dir(self.scope.root())
    .stdout(std::process::Stdio::piped())
    .stderr(std::process::Stdio::piped());
```

`spawn()` defaults stdin to **inherit**, so every `bash`-tool child gets fd 0 =
the JSON-RPC pipe from Claude Code. That hands an arbitrary child control of our
transport's read side. Given the reporter's environment (TypeScript/**Bun**), the
most likely killer is the classic one: a Node/Bun/libuv child sets `O_NONBLOCK` on
its stdin, `O_NONBLOCK` lives on the shared **open file description**, and the
parent's next blocking read returns `EAGAIN` — a read error, which the transport
treats as fatal. A child that drains fd 0 toward EOF produces the same outcome.

This is the **only** production spawn site with the defect. `RealCommandRunner`
(`executor/src/agent/command.rs:33`) and every verifier
(`executor/src/governor/verifier.rs:264,497,589`) use `.output()`, which sets stdin
to null; the remaining `Command::new` hits (`search.rs:475`, `symbols.rs:786`,
`find_files.rs:314`) are test-only.

### What is *not* wrong

`JobRegistry::await_terminal` (`mcp/src/jobs.rs:88-103`) is **already** a correctly
bounded long-poll — `tokio::time::timeout` around `wait_for`, falling back to
`Running`. The issue's suggested fix #2 ("honor the documented ~15 s bound") is
implemented; it simply never runs, because no handler is ever dispatched. Do not
"fix" it.

The issue's three candidate causes (a dropped run-completion `oneshot`, a panicking
run task, a dropped stdio future) are all ruled out by the same census: each would
leave a rexymcp frame or a live reader thread. The issue's own correction is right
— duplicate serve processes are a **separate** failure mode, and this reproduced
with one process that was never restarted.

### Why the durable-registry phase is required, not optional

Phase 01 makes the serve process **exit** when its loop dies. The `run_id` is a v4
UUID that exists only in the process's `HashMap` (`mcp/src/jobs.rs:46-48`) — nothing
on disk maps it. Without phase 03, phase 01 trades "hang forever holding the
result" for "die and lose the result." `rexymcp status` read `ended (complete)`
correctly throughout the incident precisely because it reads on-disk state; a
completed run must be reapable the same way.

## Exit criteria

- `serve` terminates when its MCP service loop terminates, logging the
  `QuitReason` and the count of runs still in flight — verified by driving a real
  `serve` process to EOF on stdin and observing both the log line and process exit
  (not just a unit test).
- No child process spawned by the `bash` tool inherits fd 0 — pinned by a test in
  which a child reading stdin observes immediate EOF rather than the parent's
  descriptor.
- A run that reached terminal state is reapable by `run_id` from a **fresh** serve
  process: `get_run_status` returns `{state:"done", result:…}` after a restart,
  instead of `{state:"unknown"}`.
- The `~15 s` bound on `get_run_status` still holds on every path, including the
  new on-disk fallback (a fallback that stats the filesystem must not become a new
  unbounded wait).
- All four gates green.

## Architecture references

- `mcp/src/main.rs:545-606` — the `Serve` arm; the unobserved `RunningService`.
- `mcp/src/jobs.rs` — `JobRegistry`, `RunState`, `spawn_run`,
  `RUN_STATUS_POLL_TIMEOUT`.
- `mcp/src/server.rs:93-127` — `get_run_status_inner`; `:710-732` — where
  `execute_phase` spawns a run and where `repo_path` is in scope.
- `executor/src/tools/bash.rs:139-160` — the spawn site.
- `executor/src/agent/mod.rs:228` — the `<repo>/.rexymcp/sessions/` convention the
  run records sit beside.
- `docs/architecture.md` § Status #30 — the async job model these phases harden.

## Phases

| #  | Phase | Status |
|----|-------|--------|
| 01 | Observe the serve loop's exit ([phase-01-observe-serve-loop-exit.md](phase-01-observe-serve-loop-exit.md)) — architect-implemented; pre-fix binary hangs on the same input, post-fix logs and exits 0 | done |
| 02 | Deny children the MCP stdin ([phase-02-null-child-stdin.md](phase-02-null-child-stdin.md)) | todo |
| 03 | Durable run registry ([phase-03-durable-run-registry.md](phase-03-durable-run-registry.md)) | todo |

**Ordering rationale.** Phase 01 first, even though phase 02 is the root cause: 01
is what converts *any* future transport death — including causes we haven't found —
from an invisible wedge into a one-line stderr message and a dead process. It is
the safety net that makes the rest of the debugging cheap. Phase 02 removes the
known trigger. Phase 03 makes 01's new failure mode (a process that now exits)
recoverable. The three are independently testable and can be reviewed
independently, but they should ship together in one release.

## Notes

**Rejected: the issue's suggested fix #4 (serve-loop watchdog).** A liveness prober
that logs or self-terminates "if the top-level future has not been polled within N
seconds" is redundant once phase 01 makes loop death loud and fatal, and it adds a
background timer that can itself misfire while runs are legitimately quiet. Not
scoped.

**Deferred: the issue's suggested fix #5 (single-instance guard).** Real, but it
belongs to the *other* bug — the `{state:"unknown"}` symptom seen when duplicate
serve processes existed. Phase 03 incidentally softens that one too (a run recorded
by serve A becomes readable by serve B), which is a reason to see whether the guard
is still worth its friction afterward. File separately; do not fold in.

**Not distinguishable at the rmcp layer: EOF vs. read error.** Both collapse to
`QuitReason::Closed`, so phase 01 cannot report *why* the transport died — a clean
client disconnect and a hostile child look identical from `waiting()`. This is why
phase 01's log line carries the in-flight run count: that, not the `QuitReason`, is
what separates "Claude Code shut us down" from "something killed our stdin
mid-flight."

**No `libc` dependency.** Directly asserting the `O_NONBLOCK`-on-shared-file-
description mechanism would need `fcntl`. Phase 02 pins the *contract* (children
get a null stdin) rather than the mechanism, which is the durable property anyway
and keeps the dependency set closed.
