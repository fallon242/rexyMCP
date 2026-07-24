# M41 — Serve Liveness & Run Durability

**Goal:** Make a dead MCP serve loop **loud and fatal** instead of silent and
permanent, stop child processes from being handed the MCP stdin they can kill the
transport with, and make a completed run's result reapable after the serve process
goes away.

**Status:** done *(opened and closed 2026-07-24; all three phases
architect-implemented at the user's request — no dispatch, no `PhaseRun`. Signed
off by the human at the milestone boundary after the live verification landed via
the M42 phase-01 dispatch.)*

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
| 02 | Deny children the MCP stdin ([phase-02-null-child-stdin.md](phase-02-null-child-stdin.md)) — architect-implemented; mutation check bites all three tests | done |
| 03 | Durable run registry ([phase-03-durable-run-registry.md](phase-03-durable-run-registry.md)) — architect-implemented; two-process E2E reaps a run the fresh serve never saw | done |

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

## M41 retrospective (2026-07-24)

**Three phases, opened and closed the same day, all `approved_first_try`.** The
milestone existed because a user filed a bug report containing `gdb` and
`eu-stack` dumps. That is the whole story of why it was cheap: the reporter had
already excluded starvation, lock deadlock, livelock, and the LLM endpoint by
process forensics, so the architect's job was reading the census rather than
reproducing an intermittent hang.

**The dumps decided the diagnosis, and one negative fact did most of the work.**
`rexymcp::main` being the *only* rexymcp frame in the process is what proved the
service loop had exited rather than stalled — because `tokio::io::Stdin` holds a
blocking thread only while a read is in flight, so "no thread blocked on fd 0"
means "nobody is reading." Everything else followed: rmcp terminates the loop on
EOF/read-error, `main` awaited `ctrl_c` instead of `waiting()`, and children
inheriting fd 0 explained how a read error could arrive at all. **Lesson: when a
report includes stack dumps, mine the frames that are *absent* before theorising
about the ones present.** Three of the reporter's own hypotheses (dropped
`oneshot`, panicking task, dropped stdio future) were each ruled out by that same
absence.

**Safety-net-first ordering was the right call and should generalise.** Phase 01
(observe the loop's exit) is not the root cause — phase 02 is — but it shipped
first, because it converts *any* future transport death, including causes not yet
found, from an invisible wedge into one stderr line and a dead process. A fix that
makes the next bug cheap to diagnose outranks a fix that closes one known path.

**A fix that changes the failure mode owes you the follow-up.** Phase 01 made
`serve` exit where it used to hang, which silently converted "result unreachable
but present" into "result gone." Phase 03 existed only because of phase 01. Worth
generalising: when a phase changes *how* something fails, ask what the new failure
loses that the old one kept.

**Cross-milestone live verification.** The one thing the milestone could not prove
about itself — a real dispatched phase completing, being reaped, and leaving a
durable record — was verified by **dispatching M42 phase-01** rather than by
building a bespoke harness. The next real piece of work became the test. That is
worth repeating whenever a runtime fix cannot verify itself in the run that
implements it.

**Three of the issue's five suggested fixes were declined, on the record.** #2 (a
bounded `get_run_status` poll) already existed and simply was never reached — a
reminder to check whether a reported-missing behavior is missing or merely
unreachable. #4 (a watchdog) was redundant once loop death became loud and fatal.
#5 (single-instance guard) belongs to the separate duplicate-serve bug. Declining
with a reason, in the milestone doc, beats silently implementing all five.

**Calibration (2 occurrences, not yet folded).** Phase-01's E2E step 1 predicted
output from `echo -n "" | rexymcp serve` that turned out wrong when run — the
no-handshake path fails inside `serve_server` before `waiting()` is reached. Same
class as M39's `total() == 3017`: an **architect-authored predicted command output
that was never executed before being written into a phase doc**. Two occurrences,
different sub-forms (a computed value; a predicted CLI behavior). Held for a third
before folding a rule into WORKFLOW.md — the candidate wording is "a phase doc's
predicted command output must either be run first or be marked as an unverified
prediction."

**No WORKFLOW.md / STANDARDS.md folds landed at this close.**

**Open follow-ups leaving M41:**

- **Single-instance guard** for `serve` (issue #5's suggested fix #5) — still
  unaddressed, and now partly softened by phase 03: a run recorded by one serve is
  readable by another. Worth re-checking whether the guard still earns its friction
  before opening anything.
- **No way to inspect run records** from the CLI (`~/.rexymcp/runs/`). Noted during
  phase 03 and deliberately not built. If reaping-after-restart becomes routine, a
  `rexymcp runs --record <id>` view is the obvious shape.
- **Prune horizon unexercised.** `RECORD_MAX_AGE_MS` is 30 days and prunes on serve
  start; nothing has aged out yet, so the path is unit-tested but has never fired
  in production.
