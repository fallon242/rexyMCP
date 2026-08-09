# Phase 02: Migrate the MCP server to rmcp 3.1.2

**Milestone:** M45 — Executor Work-Preservation Guards
**Status:** todo
**Depends on:** phase-01 (done)
**Estimated diff:** ~40 lines
**Tags:** language=rust, kind=refactor, size=s

## Goal

`mcp/` is pinned to `rmcp = "2.2"`. Move it to `3.1.2` (the MCP `2026-07-28`
spec line). This is a **small, fully-enumerated** migration: the architect built
a throwaway copy of `HEAD` against 3.1.2 and drove it to green, so every
breakage below is measured, and every replacement in § Spec is code that was
actually compiled and tested — not a sketch.

## Architecture references

Read before starting:

- `docs/architecture.md` §M5 — what the MCP server exposes and why the tool
  surface is the contract; this phase must not change that surface.
- `docs/architecture.md` §45 — the milestone entry naming this upgrade.

## Pre-flight

1. Read `docs/dev/STANDARDS.md` top to bottom.
2. Read the architecture references above.
3. Read this entire phase doc before touching any code.
4. Confirm the repo is on a clean branch with no uncommitted changes.

**You do not need to consult external documentation for this phase**, and you
cannot reach it anyway. Everything the migration requires is quoted below,
derived from the vendored crate source at
`~/.cargo/registry/src/index.crates.io-*/rmcp-3.1.2/`. If something does not
match, **the compiler error is authoritative** — read it and follow it; do not
re-read files in a loop hunting for the problem.

## Current state

`mcp/Cargo.toml:17`:

```toml
rmcp = { version = "2.2", features = ["server", "macros", "transport-io"] }
```

**What does NOT break** (measured — do not touch any of it, and do not "modernize"
it on pattern-momentum): `#[rmcp::tool_router]`, `#[rmcp::tool]`,
`rmcp::serve_server`, `rmcp::transport::stdio`, `rmcp::service::QuitReason`,
`rmcp::ErrorData` (+ `internal_error` / `invalid_params`), `Parameters`, `Json`,
`ToolCallContext`, `schema_for_type`, `RequestContext`, `RoleServer`, `Peer`,
`MaybeSendFuture`, and `ServerCapabilities` / `ServerInfo`. `schemars` stays at
`1.0` in both rmcp versions, so no derive cascade. `mcp/src/main.rs` needs **no
changes at all**.

Exactly **five** sites break, in two files. All five are quoted in § Spec with
their verbatim replacements.

### Three gotchas that will cost you a wrong turn

**1. `CallToolResponse` is `#[non_exhaustive]` and has three variants.** rmcp 3
added SEP-2663 MRTR, so `ServerHandler::call_tool` now returns
`CallToolResponse` — an enum of `Complete(CallToolResult)` /
`InputRequired(InputRequiredResult)` / `Task(CreateTaskResult)`. **This phase
implements none of the new variants.** `impl From<CallToolResult> for
CallToolResponse` exists, so every conversion here is one `.into()`. Do not
write a `match` over the enum in production code, and do not add
`InputRequired` / `Task` handling — that is out of scope.

**2. Do NOT hand-fill `ListToolsResult`'s three new fields.** rmcp 3 added
`result_type`, `ttl_ms`, and `cache_scope` (SEP-2322 / SEP-2549) as required
struct fields. The compiler will name them and it is tempting to write
`result_type: None, ttl_ms: None, cache_scope: None`. That is **wrong** —
`result_type: None` means "absent on the wire". Use the generated constructor
`ListToolsResult::with_all_items(tools)`, which sets
`result_type: Some(ResultType::COMPLETE)` and the rest `None`. Verbatim source
(`rmcp-3.1.2/src/model.rs:1624`):

```rust
            pub fn with_all_items(items: $t_item) -> Self {
                Self {
                    result_type: Some(ResultType::COMPLETE),
                    meta: None,
                    next_cursor: None,
                    ttl_ms: None,
                    cache_scope: None,
                    $i_item: items,
                }
            }
```

**3. The two `structured_result(...)` call sites need NO change.** They are at
`mcp/src/server.rs:752` and `:801`. Changing the function's return type is
enough; both callers `return` it unchanged. Do not edit them.

## Spec

### 1. Bump the dependency

In `mcp/Cargo.toml`, change line 17 to:

```toml
rmcp = { version = "3.1.2", features = ["server", "macros", "transport-io"] }
```

The feature list is unchanged and correct — all three features still exist in
3.1.2. Then run `cargo fetch` so `Cargo.lock` updates (expect
`rmcp 2.2.0 -> 3.1.2`, `rmcp-macros 2.2.0 -> 3.1.2`, and a new `syn v3.0.3`).
**This is the only authorized dependency change** — see § Authorizations.

The build will now fail with 7 errors across tasks 2–5. That is expected;
work through them in order.

### 2. `structured_result` returns the MRTR response type

In `mcp/src/server.rs:189`, replace:

```rust
fn structured_result<T: serde::Serialize>(value: &T) -> Result<CallToolResult, rmcp::ErrorData> {
```

with:

```rust
fn structured_result<T: serde::Serialize>(
    value: &T,
) -> Result<rmcp::model::CallToolResponse, rmcp::ErrorData> {
```

and, at the end of the same function, replace:

```rust
    Ok(CallToolResult::structured(json))
}
```

with:

```rust
    Ok(CallToolResult::structured(json).into())
}
```

Leave the function's doc comment and its `serde_json::to_value` / `map_err`
body untouched.

### 3. `call_tool`'s return type

In `mcp/src/server.rs:676`, replace:

```rust
    ) -> impl std::future::Future<Output = Result<CallToolResult, rmcp::ErrorData>>
```

with:

```rust
    ) -> impl std::future::Future<Output = Result<rmcp::model::CallToolResponse, rmcp::ErrorData>>
```

The following two lines (`+ rmcp::service::MaybeSendFuture` and `+ '_ {`) stay
exactly as they are. The `router.call(ctx).await` fall-through at `:804` already
produces the new type in 3.x, so it needs no change either.

### 4. `list_tools` uses the generated constructor

In `mcp/src/server.rs:819`, replace:

```rust
        Ok(rmcp::model::ListToolsResult {
            tools,
            next_cursor,
            meta: None,
        })
```

with:

```rust
        let mut result = rmcp::model::ListToolsResult::with_all_items(tools);
        result.next_cursor = next_cursor;
        Ok(result)
```

Everything above it in the function (the `tools` assembly, the two `insert`s,
the `sort_by`, the `next_cursor` binding) is unchanged.

### 5. `schema_for_output` no longer returns a `Result`

It now returns `Arc<Map<String, Value>>` directly, so both `match` blocks
collapse. In `mcp/src/server.rs:843`, replace:

```rust
    match rmcp::handler::server::tool::schema_for_output::<SpawnedRun>() {
        Ok(schema) => tool.with_raw_output_schema(schema),
        Err(_) => tool,
    }
```

with:

```rust
    tool.with_raw_output_schema(rmcp::handler::server::tool::schema_for_output::<SpawnedRun>())
```

And in `mcp/src/server.rs:855`, replace:

```rust
    match rmcp::handler::server::tool::schema_for_output::<rexymcp_executor::phase::PhaseResult>() {
        Ok(schema) => tool.with_raw_output_schema(schema),
        Err(_) => tool,
    }
```

with **exactly this form** — it is what `rustfmt` produces, so writing it any
other way costs a format-gate round trip:

```rust
    tool.with_raw_output_schema(rmcp::handler::server::tool::schema_for_output::<
        rexymcp_executor::phase::PhaseResult,
    >())
```

### 6. Destructure the response in the one affected test

`cargo build` is green after task 5, but `cargo clippy --all-targets` /
`cargo test` still fail with three `E0609` errors in one test, because
`structured_result` now yields the enum. In
`mcp/src/server_tests.rs:1175`, replace:

```rust
    let result = structured_result(&SpawnedRun {
        run_id: "r-1".to_string(),
    })
    .unwrap();
    let expected = serde_json::json!({ "run_id": "r-1" });
```

with:

```rust
    let response = structured_result(&SpawnedRun {
        run_id: "r-1".to_string(),
    })
    .unwrap();
    let rmcp::model::CallToolResponse::Complete(result) = response else {
        panic!("structured_result must produce a Complete response");
    };
    let expected = serde_json::json!({ "run_id": "r-1" });
```

The rest of the test body (the three assertions on `structured_content`,
`content[0]`, and `is_error`) is unchanged and must stay unchanged. `panic!` in
a `let ... else` is fine here — STANDARDS exempts test code from the
no-panic rule.

**Add no new tests in this phase.** See § Acceptance criteria: the test counts
must not move.

### 7. Capture the end-to-end evidence

Run the block in § End-to-end verification **verbatim and unmodified**, then
paste the resulting `.rexymcp/e2e-45-02.txt` into a new Update Log entry headed
`### Update — <date> (end-to-end verification)`, inside a single fenced code
block. The server-authored `(complete)` entry does not satisfy this.

If your `bash` tool times out on the whole block, run it one numbered section at
a time — each section only appends to the artifact, so the result is the same.

### 8. Prove the paste is byte-identical

```bash
D=docs/dev/milestones/M45-executor-work-preservation-guards/phase-02-rmcp-3-migration.md
START=$(grep -n '^### Update .*(end-to-end verification)' $D | tail -1 | cut -d: -f1)
tail -n +$START $D | awk '/^```/{n++; next} n==1' > .rexymcp/pasted-45-02.txt
diff .rexymcp/pasted-45-02.txt .rexymcp/e2e-45-02.txt && echo "PASTE MATCH" || echo "PASTE MISMATCH"
```

On `PASTE MISMATCH` the `diff` names the lines that drifted — fix them from
`.rexymcp/e2e-45-02.txt`, do not retype from memory, and re-run until it prints
`PASTE MATCH`. Quote the `PASTE MATCH` line in the completion entry.

## Acceptance criteria

- [ ] `grep -n '^rmcp' mcp/Cargo.toml` shows version `3.1.2`.
- [ ] `cargo fmt --all --check` reports no diffs.
- [ ] `cargo clippy --all-targets --all-features -- -D warnings` is clean.
- [ ] `cargo test` is green.
- [ ] **Test counts are unchanged: the executor suite reports `1068 passed` and
      the `rexymcp` suite reports `711 passed`.** This phase adds no tests and
      removes none — a *rising* count means scope creep, a falling one means a
      test was dropped. Both are findings. (Measured on the pre-phase tree:
      1068 / 711 / 2.)
- [ ] The stdio handshake in § End-to-end verification returns
      `"serverInfo":{"name":"rmcp","version":"3.1.2"}` — the wire-level proof
      the running binary is actually on the new SDK. On the pre-phase tree this
      reads `"2.2.0"`, so it fails before the change and passes after.
- [ ] `tools/list` over that same handshake returns **exactly 10** tools, with
      exactly these names: `continue_phase`, `execute_phase`, `executor_health`,
      `executor_log_search`, `executor_log_tail`, `get_run_status`, `get_turn`,
      `model_profile`, `model_scorecard`, `stop_phase`. The tool surface is the
      contract; this migration must not add, drop, or rename one.
- [ ] An Update Log entry headed `### Update — <date> (end-to-end
      verification)` contains the artifact verbatim.
- [ ] Task 8 prints `PASTE MATCH`.

## Test plan

**No new tests.** This phase is a dependency migration whose correctness is
proven by the existing 1,781 tests continuing to pass unmodified plus the
live-handshake evidence below. The one test edit (task 6) is a mechanical
destructure that preserves all three of its existing assertions.

Adding tests here would be scope creep and would break the count criterion.

## End-to-end verification

A green unit suite does **not** prove the migrated server still speaks MCP —
every test mocks the transport. The proof is driving the real binary over real
stdio and reading the version it advertises on the wire.

```bash
mkdir -p .rexymcp
A=.rexymcp/e2e-45-02.txt
: > $A
echo "=== [1] dependency version ===" >> $A
grep -n '^rmcp' mcp/Cargo.toml >> $A
grep -A 2 '^name = "rmcp"$' Cargo.lock | head -3 >> $A
echo "=== [2] gates ===" >> $A
cargo fmt --all --check > .rexymcp/g-fmt.log 2>&1; echo "fmt exit=$?" >> $A
tail -5 .rexymcp/g-fmt.log >> $A
cargo clippy --all-targets --all-features -- -D warnings > .rexymcp/g-clippy.log 2>&1; echo "clippy exit=$?" >> $A
tail -5 .rexymcp/g-clippy.log >> $A
cargo test > .rexymcp/g-test.log 2>&1; echo "test exit=$?" >> $A
grep -E '^test result' .rexymcp/g-test.log >> $A
echo "=== [3] build the real binary ===" >> $A
cargo build -p rexymcp > .rexymcp/g-build.log 2>&1; echo "build exit=$?" >> $A
tail -3 .rexymcp/g-build.log >> $A
echo "=== [4] live stdio MCP handshake ===" >> $A
printf '%s\n%s\n%s\n' \
 '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"probe","version":"0"}}}' \
 '{"jsonrpc":"2.0","method":"notifications/initialized"}' \
 '{"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}' \
 | timeout 25 ./target/debug/rexymcp serve --config rexymcp.toml > .rexymcp/hs.txt 2>.rexymcp/hs.err
echo "handshake exit=$?" >> $A
echo -n "serverInfo=" >> $A
sed -n '1p' .rexymcp/hs.txt | grep -o '"serverInfo":{[^}]*}' >> $A
echo -n "tool-count=" >> $A
sed -n '2p' .rexymcp/hs.txt | grep -o '"name":"[a-z_]*"' | wc -l >> $A
echo "tool-names:" >> $A
sed -n '2p' .rexymcp/hs.txt | grep -o '"name":"[a-z_]*"' | sed 's/.*:"//;s/"//' | sort >> $A
```

Section [4] is the load-bearing one. `serverInfo.version` is rmcp's own crate
version — the server does not set it (`mcp/src/server.rs:665` starts from
`ServerInfo::default()`), so it cannot be faked from our side and it reads
`2.2.0` until the dependency actually changes.

Note the handshake reads `serverInfo` from **line 1** of the response (the
`initialize` result) and the tool names from **line 2** (the `tools/list`
result) — line 1 also contains `"name":"rmcp"` in `serverInfo`, so a whole-file
grep would report 11 names instead of 10.

## Authorizations

- [x] **May edit `mcp/Cargo.toml`** — solely to change the `rmcp` version from
      `2.2` to `3.1.2`. The feature list stays exactly as it is. No other
      dependency may be added, removed, or version-changed.
- [x] **May update `Cargo.lock`** — as the automatic consequence of the above
      (`cargo fetch` / `cargo build`). Do not hand-edit it and do not run a
      blanket `cargo update`.

Nothing else. In particular this does **not** authorize touching
`rustfmt.toml`, `clippy.toml`, `.github/workflows/*`, or the workspace root
`Cargo.toml`.

## Out of scope

- **Implementing any new rmcp 3 capability.** MRTR's `InputRequired` / `Task`
  variants, SEP-2549 cache hints (`ttl_ms` / `cache_scope`), SEP-2243 headers,
  and protocol-version negotiation changes are all deliberately untouched. This
  phase is a *compile-and-behave-identically* migration.
- **Changing the tool surface** — no new tools, no renames, no description
  edits, no schema changes.
- **`mcp/src/main.rs`** — it needs no changes; leave it alone.
- **Reinstalling the binary or restarting `rexymcp serve`.** The human does
  that deliberately after review (a running `serve` does not hot-swap a rebuilt
  binary), and the executor must not `cargo install` anything.
- **Any `executor/` change.** This phase is confined to `mcp/` plus the two
  manifest files.
- Adding tests (see § Test plan).

## Update Log

(Filled in by the executor. See WORKFLOW.md § "Update Log entries".)

<!-- entries appended below this line -->
