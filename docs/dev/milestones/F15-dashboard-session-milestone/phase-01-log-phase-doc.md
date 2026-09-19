# Phase 1: log phase doc

**Milestone:** F15 — Dashboard session milestone
**Status:** todo
**Depends on:** none
**Estimated diff:** ~130 lines, about half tests
**Tags:** language=rust, kind=bugfix, size=s

## Goal

Log the phase doc path at the start of every session, and have the dashboard
name the milestone from it instead of guessing from the phase id.

**Verified by the architect 2026-09-19** in a scratch worktree at `6bba5ae`:
format and clippy clean, suite **734 / 2 / 1220**. Mutation checks — each of
these, applied alone, turns its test red: logging an empty path
(`logs_session_start_first_then_prompt`); `summarize` ignoring the event
(`summarize_reads_phase_doc_path`); the label ignoring the logged doc
(`session_milestone_uses_logged_phase_doc_over_guess`); `milestone_number`
back to `M`-only (`milestone_number_parses_and_rejects`).

## Pre-flight

1. `cargo test -p rexymcp dashboard::` → **183 passed**; `status::` → **45**.
2. Full `cargo test` → **732 / 2 / 1220 passed**.

## Current state

- `SessionEvent::SessionStart` carries `session_id`, `model`, `phase` only.
- The dashboard's `resolve_milestone_dir` scans milestone directories for a
  doc starting with the phase id and picks the highest number; `milestone_number`
  rejects anything not starting with `M`.
- **Gotcha — record order.** Two existing executor tests look at the session
  log's order. The new event lands at index 1, so `logs_session_start_first_then_prompt`
  moves `Prompt` to index 2 (block 2) and
  `logs_completion_parsed_and_tool_result_for_dispatched_turn` filters
  `phase_doc` out like `progress` and `metrics` (block 3). No other test changes.
- **Gotcha — exhaustive matches.** Exactly 4 `match`es over `SessionEvent` have
  no wildcard and need the new arm: `executor/src/agent/tests.rs` (block 4),
  `mcp/src/dashboard/filter.rs` (6), `mcp/src/dashboard/transcript.rs` (11),
  `mcp/src/log_query.rs` (12). The compiler will list any you miss.

## Spec

Apply these replace blocks exactly; each "Replace" text occurs once in its file.

### `executor/src/agent/mod.rs`

**1.** Replace:

```rust
        },
    );
    log_event(
        &log_handle,
```

with:

```rust
        },
    );
    log_event(
        &log_handle,
        &redactor,
        deps.clock,
        0,
        SessionEvent::PhaseDoc {
            path: input.phase_doc_path.clone(),
        },
    );
    log_event(
        &log_handle,
```

### `executor/src/agent/tests.rs`

**2.** Replace:

```rust
    let recs = records(dir.path());
    assert!(matches!(recs[0].event, SessionEvent::SessionStart { .. }));
    assert!(matches!(recs[1].event, SessionEvent::Prompt { .. }));
    match &recs[0].event {
        SessionEvent::SessionStart {
```

with:

```rust
    let recs = records(dir.path());
    assert!(matches!(recs[0].event, SessionEvent::SessionStart { .. }));
    match &recs[1].event {
        SessionEvent::PhaseDoc { path } => assert_eq!(path, &input().phase_doc_path),
        other => panic!("second record must be phase_doc, got {other:?}"),
    }
    assert!(matches!(recs[2].event, SessionEvent::Prompt { .. }));
    match &recs[0].event {
        SessionEvent::SessionStart {
```

**3.** Replace:

```rust
        .iter()
        .map(|r| event_kind(&r.event))
        .filter(|k| *k != "progress" && *k != "metrics")
        .collect();
    // SessionStart, Prompt, then turn 1: Completion, Parsed, ToolResult, then
```

with:

```rust
        .iter()
        .map(|r| event_kind(&r.event))
        .filter(|k| *k != "progress" && *k != "metrics" && *k != "phase_doc")
        .collect();
    // SessionStart, Prompt, then turn 1: Completion, Parsed, ToolResult, then
```

**4.** Replace:

```rust
    match event {
        SessionEvent::SessionStart { .. } => "session_start",
        SessionEvent::Prompt { .. } => "prompt",
        SessionEvent::Completion { .. } => "completion",
```

with:

```rust
    match event {
        SessionEvent::SessionStart { .. } => "session_start",
        SessionEvent::PhaseDoc { .. } => "phase_doc",
        SessionEvent::Prompt { .. } => "prompt",
        SessionEvent::Completion { .. } => "completion",
```

### `executor/src/store/sessions/event.rs`

**5.** Replace:

```rust
        phase: String,
    },
    Prompt {
        rendered: String,
```

with:

```rust
        phase: String,
    },
    /// The phase doc this session runs, logged right after `session_start`.
    /// Lets readers name the milestone exactly instead of guessing from the
    /// phase id, which repeats across milestones.
    PhaseDoc {
        path: String,
    },
    Prompt {
        rendered: String,
```

### `mcp/src/dashboard/filter.rs`

**6.** Replace:

```rust
    pub(crate) fn allows(&self, event: &SessionEvent) -> bool {
        match event {
            SessionEvent::SessionStart { .. } | SessionEvent::SessionEnd { .. } => self.session,
            SessionEvent::Prompt { .. } => self.prompt,
            SessionEvent::Completion { .. } => self.completion,
```

with:

```rust
    pub(crate) fn allows(&self, event: &SessionEvent) -> bool {
        match event {
            SessionEvent::SessionStart { .. }
            | SessionEvent::PhaseDoc { .. }
            | SessionEvent::SessionEnd { .. } => self.session,
            SessionEvent::Prompt { .. } => self.prompt,
            SessionEvent::Completion { .. } => self.completion,
```

### `mcp/src/dashboard/mod.rs`

**7.** Replace:

```rust
                Ok(records) => {
                    let summary = status::summarize(&records);
                    let milestone = resolve_milestone(repo, summary.phase.as_deref());
                    let milestone_costs = resolve_milestone_dir(repo, summary.phase.as_deref())
                        .map(|milestone_dir| {
                            costs::scope_costs(
                                &phase_runs,
```

with:

```rust
                Ok(records) => {
                    let summary = status::summarize(&records);
                    let milestone = session_milestone(repo, &summary);
                    let milestone_costs =
                        session_milestone_dir(repo, &summary).map(|milestone_dir| {
                            costs::scope_costs(
                                &phase_runs,
```

**8.** Replace:

```rust
                Ok(records) => {
                    let summary = status::summarize(&records);
                    let milestone = resolve_milestone(repo, summary.phase.as_deref());
                    DashboardData {
                        summary,
```

with:

```rust
                Ok(records) => {
                    let summary = status::summarize(&records);
                    let milestone = session_milestone(repo, &summary);
                    DashboardData {
                        summary,
```

**9.** Replace:

```rust
}

/// Parse the leading `M<n>` milestone number from a directory name like
/// `M15-dashboard-polish-2`. `None` if the name doesn't start with `M` followed
/// by digits and a `-`.
fn milestone_number(dir: &str) -> Option<u32> {
    let rest = dir.strip_prefix('M')?;
    let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
    if digits.is_empty() {
```

with:

```rust
}

/// The milestone directory a session belongs to. The session log's `phase_doc`
/// event names the phase doc exactly, and its parent directory is the
/// milestone. Logs written before that event existed fall back to guessing from
/// the phase id.
fn session_milestone_dir(repo: &Path, summary: &StatusSummary) -> Option<String> {
    summary
        .phase_doc_path
        .as_deref()
        .and_then(|p| Path::new(p).parent()?.file_name()?.to_str())
        .map(str::to_string)
        .or_else(|| resolve_milestone_dir(repo, summary.phase.as_deref()))
}

/// Display label for the session's milestone; see `session_milestone_dir`.
fn session_milestone(repo: &Path, summary: &StatusSummary) -> Option<String> {
    match summary.phase_doc_path {
        Some(_) => session_milestone_dir(repo, summary).map(|d| format_milestone_name(&d)),
        None => resolve_milestone(repo, summary.phase.as_deref()),
    }
}

/// Parse the leading milestone number from a directory name like
/// `M15-dashboard-polish-2` or `F07-completion-entry-date`. `None` if the name
/// doesn't start with an uppercase letter followed by digits.
fn milestone_number(dir: &str) -> Option<u32> {
    let rest = dir.strip_prefix(|c: char| c.is_ascii_uppercase())?;
    let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
    if digits.is_empty() {
```

**10.** Replace:

```rust
        assert_eq!(milestone_number("scratch"), None);
        assert_eq!(milestone_number("MX-foo"), None);
    }

```

with:

```rust
        assert_eq!(milestone_number("scratch"), None);
        assert_eq!(milestone_number("MX-foo"), None);
        assert_eq!(milestone_number("F14-dashboard-unparsed-count"), Some(14));
        assert_eq!(milestone_number("f14-lower"), None);
    }

    #[test]
    fn session_milestone_uses_logged_phase_doc_over_guess() {
        let dir = TempDir::new().unwrap();
        let milestones = dir.path().join("docs/dev/milestones");
        // The guess would pick M46: highest number with a phase-01 doc.
        for (m, doc) in [
            ("M46-token-first", "phase-01-a.md"),
            ("F14-dash-count", "phase-01-b.md"),
        ] {
            std::fs::create_dir_all(milestones.join(m)).unwrap();
            std::fs::write(milestones.join(m).join(doc), "**Status:** done\n").unwrap();
        }
        let summary = StatusSummary {
            phase: Some("phase-01".into()),
            phase_doc_path: Some("docs/dev/milestones/F14-dash-count/phase-01-b.md".into()),
            ..Default::default()
        };
        assert_eq!(
            session_milestone(dir.path(), &summary),
            Some("F14 — Dash Count".to_string())
        );
        assert_eq!(
            session_milestone_dir(dir.path(), &summary),
            Some("F14-dash-count".to_string())
        );
        // Without the logged path (an old log), the guess still runs.
        let old = StatusSummary {
            phase: Some("phase-01".into()),
            ..Default::default()
        };
        assert_eq!(
            session_milestone(dir.path(), &old),
            Some("M46 — Token First".to_string())
        );
    }

```

### `mcp/src/dashboard/transcript.rs`

**11.** Replace:

```rust
            None,
        ),
        SessionEvent::Prompt { rendered } => (
            format!("prompt ({} chars)", rendered.chars().count()),
```

with:

```rust
            None,
        ),
        SessionEvent::PhaseDoc { path } => {
            (format!("phase doc — {path}"), Color::Cyan, false, None)
        }
        SessionEvent::Prompt { rendered } => (
            format!("prompt ({} chars)", rendered.chars().count()),
```

### `mcp/src/log_query.rs`

**12.** Replace:

```rust
    match event {
        SessionEvent::SessionStart { .. } => "session_start",
        SessionEvent::Prompt { .. } => "prompt",
        SessionEvent::Completion { .. } => "completion",
```

with:

```rust
    match event {
        SessionEvent::SessionStart { .. } => "session_start",
        SessionEvent::PhaseDoc { .. } => "phase_doc",
        SessionEvent::Prompt { .. } => "prompt",
        SessionEvent::Completion { .. } => "completion",
```

### `mcp/src/status.rs`

**13.** Replace:

```rust
    pub session_id: Option<String>,
    pub phase: Option<String>,
    pub model: Option<String>,
    pub latest_turn: usize,
```

with:

```rust
    pub session_id: Option<String>,
    pub phase: Option<String>,
    /// From the `phase_doc` event; `None` on logs written before it existed.
    pub phase_doc_path: Option<String>,
    pub model: Option<String>,
    pub latest_turn: usize,
```

**14.** Replace:

```rust
                summary.phase = Some(phase.clone());
            }
            SessionEvent::Progress {
                turn,
```

with:

```rust
                summary.phase = Some(phase.clone());
            }
            SessionEvent::PhaseDoc { path } => {
                summary.phase_doc_path = Some(path.clone());
            }
            SessionEvent::Progress {
                turn,
```

**15.** Replace:

```rust
    }

    fn start() -> SessionEvent {
        SessionEvent::SessionStart {
```

with:

```rust
    }

    #[test]
    fn summarize_reads_phase_doc_path() {
        let recs = vec![
            rec(1, 0, start()),
            rec(
                2,
                0,
                SessionEvent::PhaseDoc {
                    path: "docs/dev/milestones/F14-x/phase-01-y.md".into(),
                },
            ),
        ];
        let s = summarize(&recs);
        assert_eq!(
            s.phase_doc_path.as_deref(),
            Some("docs/dev/milestones/F14-x/phase-01-y.md")
        );
    }

    fn start() -> SessionEvent {
        SessionEvent::SessionStart {
```

**Must NOT:**

- Add a field to `SessionStart` (it would touch ~17 sites).
- Remove the phase-id guess: logs written before this change need it.
- Run `cargo fmt --all`; if needed, `rustfmt --edition 2024` on touched files.

## Acceptance criteria

- [ ] A session log's second record is `phase_doc` with the input's
      `phase_doc_path`.
- [ ] `summarize` fills `StatusSummary.phase_doc_path` from it.
- [ ] With a logged path under `F14-dash-count/`, the dashboard label is
      `F14 — Dash Count` even when `M46-…` also has a `phase-01` doc; with no
      logged path the old guess (`M46 — …`) still runs.
- [ ] `cargo fmt --all --check`, `cargo build`,
      `cargo clippy --all-targets --all-features -- -D warnings`, `cargo test` pass.

## Test plan

New tests: `summarize_reads_phase_doc_path` (block 15) and
`session_milestone_uses_logged_phase_doc_over_guess` (block 10, with the
`milestone_number` asserts). Updated: blocks 2 and 3. **Finish condition:**

- `cargo test -p rexymcp dashboard::` → **184 passed**
- `cargo test -p rexymcp status::` → **46 passed**
- Full `cargo test` → **734 / 2 / 1220 passed**

Paste the `test result:` lines.

## End-to-end verification

Paste this block's output in a `### Update — YYYY-MM-DD HH:MM (end-to-end
verification)` entry:

```bash
cargo build -q -p rexymcp
T=$(mktemp -d); mkdir -p "$T/.rexymcp/sessions"
printf '%s\n%s\n' \
  '{"ts":1,"turn":0,"event":{"event_type":"session_start","session_id":"s1","model":"m","phase":"phase-01"}}' \
  '{"ts":2,"turn":0,"event":{"event_type":"phase_doc","path":"docs/dev/milestones/F14-dash-count/phase-01-b.md"}}' \
  > "$T/.rexymcp/sessions/session-phase-01-s1.jsonl"
./target/debug/rexymcp status --repo "$T" --json | grep '"phase_doc_path"'
```

Expected (the JSON is pretty-printed): `"phase_doc_path": "docs/dev/milestones/F14-dash-count/phase-01-b.md",`

## Authorizations

- The 8 files in the Spec, as specified, including the two existing test edits.

## Out of scope

- Any other use of the phase id; the README screenshot (after this lands).

## Update Log

<!-- entries appended below this line -->
