# Phase 1: budget warning

**Milestone:** F14 — Dashboard unparsed count
**Status:** todo
**Depends on:** none
**Estimated diff:** ~60 lines, half tests
**Tags:** language=rust, kind=feature, size=xs

## Goal

`load_data` already reads the store with `read_all`, whose `unparsed` count
(F12) says how many telemetry lines no longer parse. Carry it into
`DashboardData` and show `Unreadable telemetry: N` in the Budget panel, after
the "Top skill" line, only when N > 0.

**Verified by the architect 2026-09-19** in a scratch worktree at `65917e6`:
format and clippy clean, suite **732 / 2 / 1220**, `dashboard::` **183**. Each
new test fails when its part of the fix is removed (count dropped in
`load_data`; line never shown; line shown at zero).

## Pre-flight

1. `cargo test -p rexymcp dashboard::` → **180 passed**.
2. Full `cargo test` → **729 / 2 / 1220 passed**.

## Spec

Apply these replace blocks exactly; each "Replace" text occurs once.
Blocks 3–6 add `unparsed_records` to the four `DashboardData` literals in
`load_data` — there are exactly **4**.

### `mcp/src/dashboard/mod.rs`

**1.** Replace:

```rust
    pub arch_cache_5m: u64,
    pub arch_cache_1h: u64,
}

```

with:

```rust
    pub arch_cache_5m: u64,
    pub arch_cache_1h: u64,
    /// `StoreRecords::unparsed` — telemetry lines that no longer parse. Shown in
    /// the Budget panel when non-zero, the same count `rexymcp costs` prints.
    pub unparsed_records: usize,
}

```

**2.** Replace:

```rust
        .map(|dir| telemetry::read_all(&dir.join("phase_runs.jsonl")).unwrap_or_default())
        .unwrap_or_default();
    let phase_runs: Vec<PhaseRun> = store.runs;

```

with:

```rust
        .map(|dir| telemetry::read_all(&dir.join("phase_runs.jsonl")).unwrap_or_default())
        .unwrap_or_default();
    let unparsed_records = store.unparsed;
    let phase_runs: Vec<PhaseRun> = store.runs;

```

**3.** Replace:

```rust
                        arch_cache_5m,
                        arch_cache_1h,
                    }
                }
```

with:

```rust
                        arch_cache_5m,
                        arch_cache_1h,
                        unparsed_records,
                    }
                }
```

**4.** Replace:

```rust
                    arch_cache_5m,
                    arch_cache_1h,
                },
            }
```

with:

```rust
                    arch_cache_5m,
                    arch_cache_1h,
                    unparsed_records,
                },
            }
```

**5.** Replace:

```rust
                        arch_cache_5m: 0,
                        arch_cache_1h: 0,
                    }
                }
```

with:

```rust
                        arch_cache_5m: 0,
                        arch_cache_1h: 0,
                        unparsed_records,
                    }
                }
```

**6.** Replace:

```rust
                    arch_cache_5m: 0,
                    arch_cache_1h: 0,
                },
            }
```

with:

```rust
                    arch_cache_5m: 0,
                    arch_cache_1h: 0,
                    unparsed_records,
                },
            }
```

**7.** Replace:

```rust
    }

    #[test]
    fn load_data_reads_project_architect_tokens_from_ledger() {
```

with:

```rust
    }

    #[test]
    fn load_data_counts_unparsed_telemetry_lines() {
        let dir = TempDir::new().unwrap();
        let sessions = sessions_dir(dir.path());
        std::fs::create_dir_all(&sessions).unwrap();
        let telemetry_dir = dir.path().join("telemetry");
        std::fs::create_dir_all(&telemetry_dir).unwrap();
        // A current-schema ledger line missing `session_id`, plus a non-JSON line.
        std::fs::write(
            telemetry_dir.join("phase_runs.jsonl"),
            "{\"record\":\"architect_ledger\",\"schema_version\":1,\"model\":\"m\"}\nnot json\n",
        )
        .unwrap();

        let data = load_data(
            dir.path(),
            None,
            Some(&telemetry_dir),
            Some("aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee"),
            &default_architect_cfg(),
        );
        assert_eq!(data.unparsed_records, 2, "both unusable lines are counted");
    }

    #[test]
    fn load_data_reads_project_architect_tokens_from_ledger() {
```

### `mcp/src/dashboard/render.rs`

**8.** Replace:

```rust
}

/// Render the dashboard into a three-panel header band (Session · Budget ·
/// Compactions) above a body (Activity wide-left · Files right), or a
```

with:

```rust
}

/// Budget-panel warning when the telemetry store holds lines that no longer
/// parse — the same count `rexymcp costs` prints. Hidden at zero.
fn unparsed_line(n: usize) -> Option<Line<'static>> {
    (n > 0).then(|| {
        Line::from(Span::styled(
            format!("  Unreadable telemetry: {n}"),
            Style::new().fg(Color::Yellow),
        ))
    })
}

/// Render the dashboard into a three-panel header band (Session · Budget ·
/// Compactions) above a body (Activity wide-left · Files right), or a
```

**9.** Replace:

```rust
        budget.push(line);
    }

    let context = reclaim_lines(&data.summary);
```

with:

```rust
        budget.push(line);
    }
    if let Some(line) = unparsed_line(data.unparsed_records) {
        budget.push(line);
    }

    let context = reclaim_lines(&data.summary);
```

**10.** Replace:

```rust
    }

    // --- visible_offset tests ---

```

with:

```rust
    }

    #[test]
    fn unparsed_line_shows_count() {
        let line = unparsed_line(3).expect("non-zero count renders a line");
        let text = format!("{line}");
        assert!(text.contains("Unreadable telemetry: 3"), "got: {text}");
    }

    #[test]
    fn unparsed_line_hidden_when_zero() {
        assert!(unparsed_line(0).is_none(), "zero must render nothing");
    }

    // --- visible_offset tests ---

```

**Must NOT:**

- Re-read the telemetry file; use the `store` `load_data` already has.
- Show the line at 0, or anywhere but the Budget panel.
- Run `cargo fmt --all`. If needed, `rustfmt --edition 2024` on the two files.

## Acceptance criteria

- [ ] `load_data` on a store with one field-less ledger line and one non-JSON
      line returns `unparsed_records == 2`.
- [ ] `unparsed_line(3)` renders `Unreadable telemetry: 3`; `unparsed_line(0)`
      is `None`.
- [ ] `cargo fmt --all --check`, `cargo build`,
      `cargo clippy --all-targets --all-features -- -D warnings`, `cargo test` pass.

## Test plan

The three tests are in blocks 7 and 10 (block 10 holds two). **Finish condition:**

- `cargo test -p rexymcp dashboard::` → **183 passed**
- Full `cargo test` → **732 / 2 / 1220 passed**

Paste the `test result:` lines.

## End-to-end verification

In a `### Update — YYYY-MM-DD HH:MM (end-to-end verification)` entry, paste the
output of `cargo test -p rexymcp unparsed -- --nocapture`.

## Authorizations

- `mcp/src/dashboard/mod.rs` and `mcp/src/dashboard/render.rs` as specified.

## Out of scope

- The scorecard, `runs`, and the per-type telemetry readers.

## Update Log

<!-- entries appended below this line -->
