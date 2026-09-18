//! Cost-report core — `rexymcp costs` CLI.
//!
//! Token-native: reports Architect / Executor / Cache token totals across
//! Session / Milestone / Project scopes, plus a by-skill architect token
//! table. No dollar values anywhere on this path (M46 phase-01).

use std::path::Path;

use rexymcp_executor::config::Config;
use rexymcp_executor::store::metrics;
use rexymcp_executor::store::telemetry::{self, ArchitectTokens, PhaseRun};

use crate::dashboard::ScopeCosts;
use crate::status;

/// One scope's token totals. All-`u64`; summed per-run with `saturating_add`
/// (never routed through the u32 `TokenBreakdown`).
#[derive(Debug, Clone, Copy, Default, PartialEq, serde::Serialize)]
pub struct ScopeReport {
    /// Non-cached executor input tokens (the disjoint `input` class).
    pub executor_input: u64,
    pub executor_output: u64,
    pub executor_cache_read: u64,
    pub executor_cache_write: u64,
    /// All four executor classes summed. `0` when the scope has no runs.
    pub executor_tokens: u64,
    /// Architect tokens for this scope, all four classes summed.
    pub architect_tokens: u64,
}

/// Token totals across the three scopes.
#[derive(Debug, Clone, serde::Serialize)]
pub struct CostReport {
    pub session: ScopeReport,
    /// `None` when no active milestone could be resolved (no project runs).
    pub milestone: Option<ScopeReport>,
    pub project: ScopeReport,
    pub assists: u32,
    pub by_skill: Vec<SkillCost>,
}

/// Fold one scope's `ScopeCosts` into its token report.
pub fn scope_report(costs: &ScopeCosts) -> ScopeReport {
    let executor_tokens = costs
        .executor_in
        .saturating_add(costs.executor_out)
        .saturating_add(costs.executor_cache_read)
        .saturating_add(costs.executor_cache_write);
    let architect_tokens = costs
        .architect
        .input
        .saturating_add(costs.architect.output)
        .saturating_add(costs.architect.cache_creation)
        .saturating_add(costs.architect.cache_read);

    ScopeReport {
        executor_input: costs.executor_in,
        executor_output: costs.executor_out,
        executor_cache_read: costs.executor_cache_read,
        executor_cache_write: costs.executor_cache_write,
        executor_tokens,
        architect_tokens,
    }
}

/// One skill's architect spend: total tokens (all four classes).
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct SkillCost {
    pub skill: String,
    pub tokens: u64,
}

/// Display name for a stored architect-ledger skill key.
///
/// The harvester buckets messages with no `attributionSkill` under the stable
/// storage key `other`. That is untagged architect work — non-skill sessions and
/// the user↔architect conversation between phase runs — so it renders as
/// `architect chat`. Mapping here rather than at write time keeps already-
/// harvested records valid and cannot split one bucket across two rows.
pub(crate) fn display_skill(skill: &str) -> &str {
    match skill {
        "other" => "architect chat",
        s => s,
    }
}

/// Per-skill architect tokens for a project, from the ledger.
/// Sorted by `tokens` descending (ties broken by `skill` for determinism).
pub(crate) fn skill_costs(
    ledgers: &[telemetry::ArchitectLedger],
    project_id: &str,
) -> Vec<SkillCost> {
    use std::collections::HashMap;
    let mut acc: HashMap<String, u64> = HashMap::new();
    for l in ledgers
        .iter()
        .filter(|l| l.project_id.as_deref() == Some(project_id))
    {
        let toks = l
            .tokens
            .input
            .saturating_add(l.tokens.cache_creation)
            .saturating_add(l.tokens.cache_read)
            .saturating_add(l.tokens.output);
        let key = display_skill(&l.skill).to_string();
        let e = acc.entry(key).or_insert(0);
        *e = e.saturating_add(toks);
    }
    let mut out: Vec<SkillCost> = acc
        .into_iter()
        .map(|(skill, tokens)| SkillCost { skill, tokens })
        .collect();
    out.sort_by(|a, b| b.tokens.cmp(&a.tokens).then_with(|| a.skill.cmp(&b.skill)));
    out
}

/// Sum executor tokens over project runs, optionally scoped to one milestone_id.
pub(crate) fn scope_costs(
    runs: &[PhaseRun],
    ledgers: &[telemetry::ArchitectLedger],
    project_id: &str,
    milestone_id: Option<&str>,
) -> ScopeCosts {
    let exec: ScopeCosts = runs
        .iter()
        .filter(|r| {
            r.project_id.as_deref() == Some(project_id)
                && (milestone_id.is_none() || r.milestone_id.as_deref() == milestone_id)
        })
        .fold(ScopeCosts::default(), |mut c, r| {
            c.executor_in = c.executor_in.saturating_add(r.tokens.input_tokens as u64);
            c.executor_out = c.executor_out.saturating_add(r.tokens.output_tokens as u64);
            c.executor_cache_read = c
                .executor_cache_read
                .saturating_add(r.tokens.cache_read_tokens as u64);
            c.executor_cache_write = c
                .executor_cache_write
                .saturating_add(r.tokens.cache_write_tokens as u64);
            c
        });

    // Architect: ledger records carry the milestone their tokens were
    // attributed to. Project scope (`None`) sums every record.
    let mut architect_tokens = ArchitectTokens::default();
    for l in ledgers.iter().filter(|l| {
        l.project_id.as_deref() == Some(project_id)
            && (milestone_id.is_none() || l.milestone_id.as_deref() == milestone_id)
    }) {
        architect_tokens.input = architect_tokens.input.saturating_add(l.tokens.input);
        architect_tokens.cache_creation = architect_tokens
            .cache_creation
            .saturating_add(l.tokens.cache_creation);
        architect_tokens.cache_read = architect_tokens
            .cache_read
            .saturating_add(l.tokens.cache_read);
        architect_tokens.output = architect_tokens.output.saturating_add(l.tokens.output);
    }

    ScopeCosts {
        executor_in: exec.executor_in,
        executor_out: exec.executor_out,
        executor_cache_read: exec.executor_cache_read,
        executor_cache_write: exec.executor_cache_write,
        architect: architect_tokens,
    }
}

/// Load a full cost report from config + repo + telemetry.
pub fn load_cost_report(
    config_path: &Path,
    repo: &Path,
    session: Option<&str>,
    telemetry_path: Option<&Path>,
) -> Result<CostReport, String> {
    let cfg =
        Config::load_with_env(config_path).map_err(|e| format!("failed to load config: {e}"))?;

    let telemetry_file = if let Some(p) = telemetry_path {
        p.to_path_buf()
    } else if let Some(ref dir) = cfg.telemetry.dir {
        dir.join("phase_runs.jsonl")
    } else {
        return Err(
            "telemetry disabled: cfg.telemetry.dir not set and no --telemetry-path provided"
                .to_string(),
        );
    };

    // Session scope: from the live session log. No architect cost.
    let session_costs = match status::load_records(repo, session) {
        Ok(records) => {
            let summary = status::summarize(&records);
            ScopeCosts {
                executor_in: summary.last_input_tokens.unwrap_or(0) as u64,
                executor_out: summary.last_output_tokens.unwrap_or(0) as u64,
                executor_cache_read: summary.last_cache_read_tokens.unwrap_or(0) as u64,
                executor_cache_write: summary.last_cache_write_tokens.unwrap_or(0) as u64,
                architect: ArchitectTokens::default(),
            }
        }
        Err(_) => ScopeCosts::default(),
    };

    let session_report = scope_report(&session_costs);

    // Project and milestone scopes require project_id.
    let project_id = cfg.project.id.as_deref();

    // Read telemetry.
    let runs: Vec<PhaseRun> =
        telemetry::read(&telemetry_file).map_err(|e| format!("failed to read telemetry: {e}"))?;
    let activities = telemetry::fold_activities(
        telemetry::read_architect_activities(&telemetry_file).unwrap_or_default(),
    );
    let ledgers = telemetry::fold_ledger(
        telemetry::read_architect_ledger(&telemetry_file).unwrap_or_default(),
    );

    if let Some(pid) = project_id {
        let project_costs = scope_costs(&runs, &ledgers, pid, None);
        let project_report = scope_report(&project_costs);

        // Find the latest milestone_id from project runs.
        let latest_milestone_id = runs
            .iter()
            .filter(|r| r.project_id.as_deref() == Some(pid))
            .filter(|r| r.milestone_id.is_some())
            .max_by_key(|r| r.ts)
            .and_then(|r| r.milestone_id.as_deref());

        let milestone_report = latest_milestone_id.map(|mid| {
            let costs = scope_costs(&runs, &ledgers, pid, Some(mid));
            scope_report(&costs)
        });

        // Assists: count folded activities with project_id and activity == "assist".
        let assists = activities
            .iter()
            .filter(|a| a.project_id.as_deref() == Some(pid) && a.activity == "assist")
            .count() as u32;

        let by_skill = skill_costs(&ledgers, pid);
        Ok(CostReport {
            session: session_report,
            milestone: milestone_report,
            project: project_report,
            assists,
            by_skill,
        })
    } else {
        // No project_id: session still computes; project/milestone are zero.
        let zero = ScopeCosts::default();
        let zero_report = scope_report(&zero);
        Ok(CostReport {
            session: session_report,
            milestone: None,
            project: zero_report,
            assists: 0,
            by_skill: Vec::new(),
        })
    }
}

/// Format the cost report as a human-readable token table.
pub fn format_costs(report: &CostReport) -> String {
    let mut lines = ledger_lines(&report.session, report.milestone.as_ref(), &report.project);

    lines.push(format!("Assists: {}", report.assists));

    // Per-skill architect token table (project-scoped).
    if !report.by_skill.is_empty() {
        let total: u64 = report.by_skill.iter().map(|s| s.tokens).sum();
        lines.push(String::new());
        lines.push("By skill (architect)".to_string());
        lines.push(format!("{:<20}{:>10}{:>8}", "SKILL", "TOKENS", "%"));
        for s in &report.by_skill {
            let pct = if total > 0 {
                s.tokens as f64 / total as f64 * 100.0
            } else {
                0.0
            };
            lines.push(format!(
                "{:<20}{:>10}{:>7.1}%",
                s.skill,
                metrics::fmt_tokens(s.tokens),
                pct,
            ));
        }
    }

    lines.join("\n")
}

/// Build a row string with the right column widths for the given scope count.
fn make_row(label: &str, v1: String, v2: String, v3: String, has_milestone: bool) -> String {
    if has_milestone {
        format!("  {:<10}{:>10}{:>10}{:>10}", label, v1, v2, v3)
    } else {
        format!("  {:<10}{:>9}{:>9}", label, v1, v3)
    }
}

/// A token cell's dash aligned on the decimal column. `fmt_tokens`' bare
/// "—" right-aligns to the field's right edge, but a `X.Xk`/`X.XM` value
/// keeps its decimal 2 columns in (`.` + one digit + a k/M suffix); two
/// trailing spaces drop the em-dash onto that decimal column. Applied
/// here at the render level, NOT in `fmt_tokens` — that helper is shared
/// by scorecard/runs/calibrate-governor, where a bare "—" is correct.
/// The same 2-in-from-the-right padding places the Cache-row `%` cells
/// (`NN.N%` is `.` + digit + `%`) on the identical marker column.
const TOK_DASH: &str = "—  ";

fn tok_cell(n: u64) -> String {
    let s = metrics::fmt_tokens(n);
    if s == "—" { TOK_DASH.to_string() } else { s }
}

/// Prompt-side cache-hit ratio in percent. `None` when the scope/run has no
/// cache activity (absence of instrumentation is not a measurement).
pub(crate) fn cache_hit_pct(input: u64, cache_read: u64, cache_write: u64) -> Option<f64> {
    let denom = input + cache_read + cache_write;
    if cache_read + cache_write == 0 || denom == 0 {
        None
    } else {
        Some(cache_read as f64 / denom as f64 * 100.0)
    }
}

/// The Budget ledger: a header plus Architect / Executor / Cache rows across
/// the Session / Milestone / Project scopes. First two rows are token counts;
/// the Cache row is the executor's prompt-side cache-hit ratio (`%`), or `—`
/// when the scope has no cache-class data.
///
/// Returns an empty Vec when there is nothing to render — never a lone header.
pub fn ledger_lines(
    session: &ScopeReport,
    milestone: Option<&ScopeReport>,
    project: &ScopeReport,
) -> Vec<String> {
    let has_milestone = milestone.is_some();
    let mile_default = ScopeReport::default();
    let mile = milestone.unwrap_or(&mile_default);

    let header = if has_milestone {
        format!(
            "{:<12}{:>10}{:>10}{:>10}",
            "Tokens", "Session", "Milestone", "Project"
        )
    } else {
        format!("{:<12}{:>9}{:>9}", "Tokens", "Session", "Project")
    };

    // Token cells use the shared decimal-aligned dash helper (see `TOK_DASH`).

    // Cache: prompt-side hit ratio. No cache-class data (Session scope) or no
    // cache activity renders the dash, never `0.0%` — an absence of
    // instrumentation should not read as a measurement.
    let cache_cell = |r: &ScopeReport| -> String {
        match cache_hit_pct(
            r.executor_input,
            r.executor_cache_read,
            r.executor_cache_write,
        ) {
            Some(pct) => format!("{pct:.1}%"),
            None => TOK_DASH.to_string(),
        }
    };

    vec![
        header,
        make_row(
            "Architect:",
            tok_cell(session.architect_tokens),
            tok_cell(mile.architect_tokens),
            tok_cell(project.architect_tokens),
            has_milestone,
        ),
        make_row(
            "Executor:",
            tok_cell(session.executor_tokens),
            tok_cell(mile.executor_tokens),
            tok_cell(project.executor_tokens),
            has_milestone,
        ),
        make_row(
            "Cache:",
            cache_cell(session),
            cache_cell(mile),
            cache_cell(project),
            has_milestone,
        ),
    ]
}

/// The Cache-split view: executor read/write per scope, plus the architect
/// ledger's 5m/1h cache-creation split (project scope only — the ledger has
/// no session or milestone dimension). Same column widths, dash and token-cell
/// conventions as `ledger_lines`; the architect cells are `TOK_DASH` outside
/// the Project column (no per-session/per-milestone architect data exists).
pub fn cache_split_lines(
    session: &ScopeReport,
    milestone: Option<&ScopeReport>,
    project: &ScopeReport,
    arch_cache_5m: u64,
    arch_cache_1h: u64,
) -> Vec<String> {
    let has_milestone = milestone.is_some();
    let mile_default = ScopeReport::default();
    let mile = milestone.unwrap_or(&mile_default);

    let header = if has_milestone {
        format!(
            "{:<12}{:>10}{:>10}{:>10}",
            "Cache", "Session", "Milestone", "Project"
        )
    } else {
        format!("{:<12}{:>9}{:>9}", "Cache", "Session", "Project")
    };

    // Architect cache-creation cells exist only at Project scope; everywhere
    // else the column is the padded dash.
    let arch_cell = |_: &ScopeReport| TOK_DASH.to_string();

    vec![
        header,
        make_row(
            "  Read:",
            tok_cell(session.executor_cache_read),
            tok_cell(mile.executor_cache_read),
            tok_cell(project.executor_cache_read),
            has_milestone,
        ),
        make_row(
            "  Write:",
            tok_cell(session.executor_cache_write),
            tok_cell(mile.executor_cache_write),
            tok_cell(project.executor_cache_write),
            has_milestone,
        ),
        make_row(
            "  Arch 5m:",
            arch_cell(session),
            arch_cell(mile),
            tok_cell(arch_cache_5m),
            has_milestone,
        ),
        make_row(
            "  Arch 1h:",
            arch_cell(session),
            arch_cell(mile),
            tok_cell(arch_cache_1h),
            has_milestone,
        ),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sess_input_output(inp: u64, outp: u64) -> ScopeReport {
        ScopeReport {
            executor_input: inp,
            executor_output: outp,
            executor_tokens: inp.saturating_add(outp),
            ..Default::default()
        }
    }

    #[test]
    fn scope_report_copies_class_fields_and_folds_totals() {
        let costs = ScopeCosts {
            executor_in: 600_000,
            executor_out: 200_000,
            executor_cache_read: 300_000,
            executor_cache_write: 100_000,
            architect: ArchitectTokens {
                input: 500_000,
                cache_creation: 100_000,
                cache_read: 200_000,
                output: 300_000,
            },
        };
        let r = scope_report(&costs);
        assert_eq!(r.executor_input, 600_000);
        assert_eq!(r.executor_output, 200_000);
        assert_eq!(r.executor_cache_read, 300_000);
        assert_eq!(r.executor_cache_write, 100_000);
        assert_eq!(r.executor_tokens, 1_200_000);
        assert_eq!(r.architect_tokens, 1_100_000);
    }

    #[test]
    fn scope_report_no_runs_is_zero() {
        let r = scope_report(&ScopeCosts::default());
        assert_eq!(r, ScopeReport::default());
    }

    #[test]
    fn format_costs_omits_milestone_when_none() {
        let report = CostReport {
            session: sess_input_output(40_000, 10_000),
            milestone: None,
            project: sess_input_output(200_000, 50_000),
            assists: 3,
            by_skill: Vec::new(),
        };
        let out = format_costs(&report);
        assert!(out.contains("Session"));
        assert!(out.contains("Project"));
        assert!(out.contains("Architect:"));
        assert!(out.contains("Executor:"));
        assert!(out.contains("Cache:"));
        // Milestone data row should NOT appear.
        let lines: Vec<&str> = out.lines().collect();
        for line in &lines[1..] {
            assert!(
                !line.starts_with("Milestone"),
                "Milestone data row should be omitted: {line}"
            );
        }
    }

    #[test]
    fn format_costs_shows_milestone_when_some() {
        let report = CostReport {
            session: sess_input_output(40_000, 10_000),
            milestone: Some(sess_input_output(30_000, 5_000)),
            project: sess_input_output(200_000, 50_000),
            assists: 3,
            by_skill: Vec::new(),
        };
        let out = format_costs(&report);
        assert!(out.contains("Session"));
        assert!(out.contains("Milestone"));
        assert!(out.contains("Project"));
        assert!(out.contains("Assists: 3"));
    }

    #[test]
    fn session_scope_cache_cells_from_summary() {
        // Hand-built ScopeCosts mirroring what load_cost_report wires from a
        // summary with cache_read=300k, input=600k, cache_write=100k.
        let session_costs = ScopeCosts {
            executor_in: 600_000,
            executor_out: 1_000,
            executor_cache_read: 300_000,
            executor_cache_write: 100_000,
            architect: ArchitectTokens::default(),
        };
        let sess = scope_report(&session_costs);
        let lines = ledger_lines(&sess, None, &ScopeReport::default());
        let cache_line = lines
            .iter()
            .find(|l| l.contains("Cache:"))
            .expect("Cache row present");
        assert!(
            cache_line.contains("30.0%"),
            "session cache cell should read 30.0%: {cache_line}"
        );
    }

    #[test]
    fn cache_split_lines_renders_four_rows() {
        let sess = ScopeReport {
            executor_cache_read: 90_200,
            executor_cache_write: 0,
            ..Default::default()
        };
        let proj = ScopeReport {
            executor_cache_read: 53_500_000,
            executor_cache_write: 810_400,
            ..Default::default()
        };
        let lines = cache_split_lines(&sess, None, &proj, 1_500_000, 500_300);
        let texts: Vec<String> = lines.iter().map(|l| l.to_string()).collect();

        let read_line = texts
            .iter()
            .find(|s| s.contains("Read:"))
            .expect("Read: row");
        assert!(read_line.contains("90.2k"), "session read: {read_line}");
        assert!(read_line.contains("53.5M"), "project read: {read_line}");

        let write_line = texts
            .iter()
            .find(|s| s.contains("Write:"))
            .expect("Write: row");
        assert!(write_line.contains("810.4k"), "project write: {write_line}");

        let arch_5m = texts
            .iter()
            .find(|s| s.contains("Arch 5m:"))
            .expect("Arch 5m: row");
        assert!(arch_5m.contains("1.5M"), "project arch 5m: {arch_5m}");
        // Session/milestone arch cells are all dashes (no per-session arch data).
        let dash_count = arch_5m.match_indices('—').count();
        assert!(
            dash_count >= 1,
            "Arch 5m: session cell must be a dash: {arch_5m}"
        );

        let arch_1h = texts
            .iter()
            .find(|s| s.contains("Arch 1h:"))
            .expect("Arch 1h: row");
        assert!(arch_1h.contains("500.3k"), "project arch 1h: {arch_1h}");

        // Row order: Read, Write, Arch 5m, Arch 1h.
        let labels: Vec<String> = texts[1..]
            .iter()
            .map(|s| {
                let end = s.find(':').unwrap_or(s.len());
                s[..end].trim().to_string()
            })
            .filter(|s| !s.is_empty())
            .collect();
        let expected: Vec<String> = vec!["Read", "Write", "Arch 5m", "Arch 1h"]
            .into_iter()
            .map(String::from)
            .collect();
        assert_eq!(labels, expected);
    }

    #[test]
    fn cache_split_alignment_matches_decimal_column() {
        // Decimal/dash alignment: Session cells share one marker column and
        // Project cells share another; dash cells land on the decimal column
        // of their scope. Char-columns, not bytes — the em-dash is 3 UTF-8
        // bytes wide, so byte offsets would be misaligned.
        let sess = ScopeReport {
            executor_cache_read: 90_200, // "90.2k" — Session column
            ..Default::default()
        };
        let proj = ScopeReport {
            executor_cache_read: 53_500_000, // "53.5M" — Project column
            executor_cache_write: 810_400,   // "810.4k" — Project column
            ..Default::default()
        };
        let lines = cache_split_lines(&sess, None, &proj, 1_500_000, 500_300);
        let texts: Vec<String> = lines.iter().map(|l| l.to_string()).collect();
        let read = texts.iter().find(|s| s.contains("Read:")).unwrap();
        let write = texts.iter().find(|s| s.contains("Write:")).unwrap();
        let arch_5m = texts.iter().find(|s| s.contains("Arch 5m:")).unwrap();
        let arch_1h = texts.iter().find(|s| s.contains("Arch 1h:")).unwrap();

        let col_of = |line: &str, needle: char, nth: usize| -> usize {
            line.chars()
                .enumerate()
                .filter(|(_, c)| *c == needle)
                .nth(nth)
                .map(|(i, _)| i)
                .unwrap()
        };

        // Session column: read=90.2k (decimal at col N), others are dashes.
        let read_session = col_of(read, '.', 0);
        let write_session = col_of(write, '—', 0);
        let arch_5m_session = col_of(arch_5m, '—', 0);
        let arch_1h_session = col_of(arch_1h, '—', 0);
        assert_eq!(
            write_session, read_session,
            "Write dash aligns with Read decimal\nRead:  {read}\nWrite: {write}"
        );
        assert_eq!(
            arch_5m_session, read_session,
            "Arch 5m dash aligns with Read decimal\nRead:    {read}\nArch5m:  {arch_5m}"
        );
        assert_eq!(
            arch_1h_session, read_session,
            "Arch 1h dash aligns with Read decimal\nRead:   {read}\nArch1h: {arch_1h}"
        );

        // Project column: every cell here is non-zero, so each row's project
        // decimal is the first '.' after the session cell.
        let read_p = col_of(read, '.', 1);
        let write_p = col_of(write, '.', 0);
        let arch_5m_p = col_of(arch_5m, '.', 0);
        let arch_1h_p = col_of(arch_1h, '.', 0);
        assert_eq!(
            read_p, write_p,
            "Read/Write Project decimals align\nRead:  {read}\nWrite: {write}"
        );
        assert_eq!(
            read_p, arch_5m_p,
            "Read/Arch 5m Project decimals align\nRead:   {read}\nArch5m: {arch_5m}"
        );
        assert_eq!(
            read_p, arch_1h_p,
            "Read/Arch 1h Project decimals align\nRead:  {read}\nArch1h: {arch_1h}"
        );
    }

    #[test]
    fn load_cost_report_telemetry_disabled_errors() {
        // Use a temp config file with telemetry.enabled = false.
        let tmp = tempfile::tempdir().unwrap();
        let config_path = tmp.path().join("rexymcp.toml");
        std::fs::write(
            &config_path,
            r#"provider = "ollama"
base_url = "http://localhost:1234/v1"

[telemetry]
enabled = false
"#,
        )
        .unwrap();
        let err = load_cost_report(&config_path, tmp.path(), None, None).unwrap_err();
        assert!(
            err.contains("telemetry disabled"),
            "expected telemetry disabled error: {err}"
        );
    }

    #[test]
    fn scope_costs_none_sums_all_milestones() {
        use rexymcp_executor::ai::types::TokenBreakdown;
        use rexymcp_executor::store::telemetry::{Gates, GenerationParams, PhaseRun};
        let run = |proj: &str, mile: &str, inp: u32, outp: u32| PhaseRun {
            ts: 1,
            model: "m".into(),
            generation_params: GenerationParams::default(),
            phase_id: "p".into(),
            phase_doc_path: None,
            tags: vec![],
            status: "complete".into(),
            escalated: false,
            gates: Gates {
                fmt: Some(true),
                build: Some(true),
                lint: Some(true),
                test: Some(true),
            },
            parse_failure_rate: 0.0,
            repairs_per_call: 0.0,
            verifier_retries: 0,
            tool_success_rate: 1.0,
            turns: 1,
            wall_clock_s: 1.0,
            tokens: TokenBreakdown {
                input_tokens: inp,
                output_tokens: outp,
                ..Default::default()
            },
            warnings: None,
            bugs_filed: None,
            bounces_to_approval: None,
            architect_verdict: None,
            served_model: None,
            length_finish_rate: None,
            context_window: None,
            context_efficiency: Default::default(),
            project_id: Some(proj.into()),
            milestone_id: Some(mile.into()),
            tier_telemetry: Default::default(),
            ..Default::default()
        };
        let runs = vec![
            run("P", "mA", 100, 10),
            run("P", "mB", 200, 20),
            run("OTHER", "mA", 999, 999), // different project — must be excluded
        ];
        // None = all milestones of project P: 100+200 input, 10+20 output.
        let all = scope_costs(&runs, &[], "P", None);
        assert_eq!(all.executor_in, 300);
        assert_eq!(all.executor_out, 30);
        // Some("mA") = only that milestone.
        let just_a = scope_costs(&runs, &[], "P", Some("mA"));
        assert_eq!(just_a.executor_in, 100);
        // Superset: project (None) >= milestone (Some).
        assert!(all.executor_in >= just_a.executor_in);
    }

    #[test]
    fn scope_costs_sums_cache_buckets() {
        use rexymcp_executor::ai::types::TokenBreakdown;
        use rexymcp_executor::store::telemetry::{Gates, GenerationParams, PhaseRun};
        let run = |proj: &str, inp: u32, outp: u32, cache_read: u32, cache_write: u32| PhaseRun {
            ts: 1,
            model: "m".into(),
            generation_params: GenerationParams::default(),
            phase_id: "p".into(),
            phase_doc_path: None,
            tags: vec![],
            status: "complete".into(),
            escalated: false,
            gates: Gates {
                fmt: Some(true),
                build: Some(true),
                lint: Some(true),
                test: Some(true),
            },
            parse_failure_rate: 0.0,
            repairs_per_call: 0.0,
            verifier_retries: 0,
            tool_success_rate: 1.0,
            turns: 1,
            wall_clock_s: 1.0,
            tokens: TokenBreakdown {
                input_tokens: inp,
                output_tokens: outp,
                cache_read_tokens: cache_read,
                cache_write_tokens: cache_write,
            },
            warnings: None,
            bugs_filed: None,
            bounces_to_approval: None,
            architect_verdict: None,
            served_model: None,
            length_finish_rate: None,
            context_window: None,
            context_efficiency: Default::default(),
            project_id: Some(proj.into()),
            milestone_id: None,
            tier_telemetry: Default::default(),
            ..Default::default()
        };
        let runs = vec![run("P", 100, 10, 50, 30), run("P", 200, 20, 100, 70)];
        let all = scope_costs(&runs, &[], "P", None);
        assert_eq!(all.executor_in, 300);
        assert_eq!(all.executor_out, 30);
        assert_eq!(all.executor_cache_read, 150);
        assert_eq!(all.executor_cache_write, 100);
    }

    fn ledger(model: &str) -> telemetry::ArchitectLedger {
        telemetry::ArchitectLedger {
            record: telemetry::ARCHITECT_LEDGER_RECORD_TAG.to_string(),
            project_id: Some("P".to_string()),
            session_id: "s".to_string(),
            model: model.to_string(),
            skill: "dispatch".to_string(),
            milestone_id: None,
            tokens: ArchitectTokens {
                input: 1_000_000,
                cache_creation: 0,
                cache_read: 0,
                output: 1_000_000,
            },
            cache_creation_5m: 0,
            cache_creation_1h: 0,
            messages: 1,
            last_ts: 1,
        }
    }

    #[test]
    fn scope_costs_milestone_counts_only_matching_ledgers() {
        // Architect tokens are attributable at milestone scope: only records
        // whose milestone_id matches are summed.
        let mut l1 = ledger("claude-opus-4-8");
        l1.milestone_id = Some("F07-a".to_string());
        let mut l2 = ledger("claude-opus-4-8");
        l2.milestone_id = Some("F08-b".to_string());
        let l3 = ledger("claude-opus-4-8");
        // l3 has milestone_id: None

        let c = scope_costs(
            &[],
            &[l1.clone(), l2.clone(), l3.clone()],
            "P",
            Some("F07-a"),
        );
        assert_eq!(c.architect.input, 1_000_000);

        let c = scope_costs(&[], &[l1, l2, l3], "P", None);
        assert_eq!(c.architect.input, 3_000_000);
    }

    #[test]
    fn skill_costs_groups_and_folds_per_skill() {
        // Two dispatch records (opus + sonnet-5) fold into one "dispatch" row,
        // and a review record into "review" — grouping only, no pricing.
        let mut ledgers = vec![
            ledger("claude-opus-4-8"), // dispatch: 2M tokens
            ledger("claude-sonnet-5"), // dispatch: 2M tokens
        ];
        let mut review = ledger("claude-opus-4-8");
        review.skill = "review".to_string();
        ledgers.push(review); // review: 2M tokens

        let costs = skill_costs(&ledgers, "P");

        assert_eq!(costs.len(), 2);
        assert_eq!(costs[0].skill, "dispatch");
        assert_eq!(costs[0].tokens, 4_000_000); // 2 records × 2M tokens
        assert_eq!(costs[1].skill, "review");
        assert_eq!(costs[1].tokens, 2_000_000);
    }

    #[test]
    fn skill_costs_sorted_by_tokens_desc() {
        // Two skills, larger token count first.
        let mut ledgers = vec![];
        let mut alpha = ledger("claude-sonnet-5");
        alpha.skill = "alpha".to_string();
        alpha.tokens = ArchitectTokens {
            input: 500_000,
            output: 500_000,
            ..Default::default()
        };
        ledgers.push(alpha); // alpha: 1M tokens
        let mut zeta = ledger("claude-opus-4-8");
        zeta.skill = "zeta".to_string();
        ledgers.push(zeta); // zeta: 2M tokens

        let costs = skill_costs(&ledgers, "P");

        assert_eq!(costs.len(), 2);
        assert_eq!(costs[0].skill, "zeta"); // higher tokens first
        assert_eq!(costs[1].skill, "alpha");

        // Equal-token ties break by skill name ascending: both rows now carry
        // the same 2M token count.
        let mut aaa = ledger("claude-opus-4-8");
        aaa.skill = "aaa".to_string();
        ledgers.push(aaa);
        let costs = skill_costs(&ledgers, "P");
        let zeta_row = costs.iter().find(|c| c.skill == "zeta").unwrap();
        let aaa_row = costs.iter().find(|c| c.skill == "aaa").unwrap();
        assert_eq!(zeta_row.tokens, 2_000_000);
        assert_eq!(aaa_row.tokens, 2_000_000);
        let zeta_pos = costs.iter().position(|c| c.skill == "zeta").unwrap();
        let aaa_pos = costs.iter().position(|c| c.skill == "aaa").unwrap();
        assert!(
            aaa_pos < zeta_pos,
            "equal-token skills must tie-break ascending by name: {costs:?}"
        );
    }

    #[test]
    fn skill_costs_empty_is_empty() {
        assert!(skill_costs(&[], "P").is_empty());
    }

    #[test]
    fn display_skill_maps_other_to_architect_chat() {
        assert_eq!(display_skill("other"), "architect chat");
    }

    #[test]
    fn display_skill_passes_through_named_skills() {
        assert_eq!(display_skill("rexymcp:dispatch"), "rexymcp:dispatch");
        assert_eq!(display_skill("rexymcp:auto"), "rexymcp:auto");
    }

    #[test]
    fn skill_costs_renders_other_as_architect_chat() {
        let mut ledgers = vec![ledger("claude-opus-4-8")]; // dispatch: 2M
        let mut other = ledger("claude-sonnet-5");
        other.skill = "other".to_string();
        ledgers.push(other); // other: 2M

        let costs = skill_costs(&ledgers, "P");

        assert_eq!(costs.len(), 2);
        let skills: Vec<&str> = costs.iter().map(|c| c.skill.as_str()).collect();
        assert!(skills.contains(&"dispatch"));
        assert!(skills.contains(&"architect chat"));
        assert!(!skills.contains(&"other"));
    }

    #[test]
    fn skill_costs_folds_other_and_architect_chat_into_one_row() {
        let mut ledgers = vec![ledger("claude-opus-4-8")]; // dispatch 2M
        let mut other = ledger("claude-sonnet-5");
        other.skill = "other".to_string();
        ledgers.push(other); // other 2M
        let mut already_renamed = ledger("claude-sonnet-5");
        already_renamed.skill = "architect chat".to_string();
        ledgers.push(already_renamed); // architect chat 2M

        let costs = skill_costs(&ledgers, "P");

        assert_eq!(costs.len(), 2);
        let chat = costs.iter().find(|c| c.skill == "architect chat").unwrap();
        assert_eq!(chat.tokens, 4_000_000); // 2 records × 2M tokens
        let dispatch = costs.iter().find(|c| c.skill == "dispatch").unwrap();
        assert_eq!(dispatch.tokens, 2_000_000);
    }

    #[test]
    fn by_skill_percent_is_token_share() {
        let report = CostReport {
            session: ScopeReport::default(),
            milestone: None,
            project: ScopeReport::default(),
            assists: 0,
            by_skill: vec![
                SkillCost {
                    skill: "alpha".to_string(),
                    tokens: 75_000,
                },
                SkillCost {
                    skill: "beta".to_string(),
                    tokens: 25_000,
                },
            ],
        };
        let out = format_costs(&report);
        assert!(
            out.contains("75.0%") && out.contains("25.0%"),
            "percent must be token share: {out}"
        );
    }

    #[test]
    fn costs_output_contains_no_dollar_sign() {
        // Fully populated report — milestone present, by-skill non-empty.
        let report = CostReport {
            session: sess_input_output(40_000, 10_000),
            milestone: Some(sess_input_output(30_000, 5_000)),
            project: sess_input_output(200_000, 50_000),
            assists: 3,
            by_skill: vec![
                SkillCost {
                    skill: "rexymcp:auto".to_string(),
                    tokens: 45_100_000,
                },
                SkillCost {
                    skill: "architect chat".to_string(),
                    tokens: 27_700_000,
                },
            ],
        };
        let out = format_costs(&report);
        assert!(
            !out.contains('$'),
            "token-native costs output must contain no $: {out}"
        );
    }

    #[test]
    fn scope_report_json_is_token_only() {
        let r = ScopeReport {
            executor_input: 1,
            executor_output: 2,
            executor_cache_read: 3,
            executor_cache_write: 4,
            executor_tokens: 10,
            architect_tokens: 20,
        };
        let json = serde_json::to_value(r).unwrap();
        let obj = json.as_object().unwrap();
        assert_eq!(obj.len(), 6, "ScopeReport must serialize exactly six keys");
        for key in [
            "executor_input",
            "executor_output",
            "executor_cache_read",
            "executor_cache_write",
            "executor_tokens",
            "architect_tokens",
        ] {
            assert!(obj.contains_key(key), "missing key {key}: {json}");
        }
    }

    #[test]
    fn format_costs_omits_by_skill_when_empty() {
        let report = CostReport {
            session: sess_input_output(40_000, 10_000),
            milestone: None,
            project: sess_input_output(200_000, 50_000),
            assists: 0,
            by_skill: Vec::new(),
        };

        let output = format_costs(&report);
        assert!(!output.contains("By skill"));
        assert!(!output.contains("SKILL"));
    }

    #[test]
    fn format_costs_by_skill_percent_zero_when_total_zero() {
        let report = CostReport {
            session: ScopeReport::default(),
            milestone: None,
            project: ScopeReport::default(),
            assists: 0,
            by_skill: vec![SkillCost {
                skill: "dispatch".to_string(),
                tokens: 0,
            }],
        };

        let output = format_costs(&report);
        assert!(
            output.contains("0.0%"),
            "zero total should show 0.0%: {output}"
        );
    }

    #[test]
    fn format_costs_header_has_no_baseline_column() {
        let report = CostReport {
            session: ScopeReport::default(),
            milestone: None,
            project: ScopeReport::default(),
            assists: 0,
            by_skill: Vec::new(),
        };
        let output = format_costs(&report);
        let header = output.lines().next().expect("header line present");
        let expected = format!("{:<12}{:>9}{:>9}", "Tokens", "Session", "Project");
        assert_eq!(header, expected, "header mismatch: {header}");
    }

    // --- Ledger tests ---

    #[test]
    fn ledger_row_order_is_architect_executor_cache() {
        let sess = sess_input_output(40_000, 10_000);
        let proj = sess_input_output(200_000, 50_000);
        let lines = ledger_lines(&sess, None, &proj);
        let labels: Vec<&str> = lines[1..]
            .iter()
            .map(|l| {
                let end = l.find(':').unwrap_or(l.len());
                l[..end].trim()
            })
            .collect();
        assert_eq!(labels, vec!["Architect", "Executor", "Cache"]);
    }

    #[test]
    fn cache_hit_pct_none_when_no_activity() {
        assert_eq!(cache_hit_pct(600_000, 0, 0), None);
        assert_eq!(cache_hit_pct(0, 0, 0), None);
    }

    #[test]
    fn cache_hit_pct_prompt_side_ratio() {
        assert!((cache_hit_pct(600_000, 300_000, 100_000).unwrap() - 30.0).abs() < f64::EPSILON);
        assert!((cache_hit_pct(0, 300_000, 100_000).unwrap() - 75.0).abs() < f64::EPSILON);
        assert!(
            (cache_hit_pct(100_000, 100_000, 100_000).unwrap() - 33.333333333333336).abs() < 1e-9
        );
    }

    #[test]
    fn cache_row_shows_hit_ratio_when_cache_present() {
        // Cache = cache_read / (input + cache_read + cache_write)
        //       = 300k / (600k + 300k + 100k) = 30.0%
        let proj = ScopeReport {
            executor_input: 600_000,
            executor_cache_read: 300_000,
            executor_cache_write: 100_000,
            executor_output: 0,
            executor_tokens: 1_000_000,
            architect_tokens: 0,
        };
        let lines = ledger_lines(&ScopeReport::default(), None, &proj);
        let cache_line = lines
            .iter()
            .find(|l| l.contains("Cache:"))
            .expect("Cache row present");
        assert!(
            cache_line.contains("30.0%"),
            "Cache row should show 30.0%: {cache_line}"
        );
    }

    #[test]
    fn cache_row_dashes_when_no_cache_activity() {
        // Input+output present but zero cache classes → padded dash, never 0.0%.
        let proj = sess_input_output(600_000, 400_000);
        let lines = ledger_lines(&ScopeReport::default(), None, &proj);
        let cache_line = lines
            .iter()
            .find(|l| l.contains("Cache:"))
            .expect("Cache row present");
        assert!(
            cache_line.contains('—'),
            "no-cache-activity scope must render dash: {cache_line}"
        );
        assert!(
            !cache_line.contains("0.0%"),
            "no-cache-activity must not render 0.0%: {cache_line}"
        );
    }

    #[test]
    fn ledger_shows_counts_and_cache_row() {
        let sess = ScopeReport {
            executor_tokens: 500_000,
            architect_tokens: 1_200_000,
            ..Default::default()
        };
        let proj = ScopeReport {
            executor_tokens: 2_000_000,
            architect_tokens: 5_500_000,
            executor_input: 600_000,
            executor_cache_read: 300_000,
            executor_cache_write: 100_000,
            executor_output: 1_000_000,
        };
        let lines = ledger_lines(&sess, None, &proj);
        let architect_line = lines
            .iter()
            .find(|l| l.contains("Architect:"))
            .expect("Architect row present");
        let executor_line = lines
            .iter()
            .find(|l| l.contains("Executor:"))
            .expect("Executor row present");
        let cache_line = lines
            .iter()
            .find(|l| l.contains("Cache:"))
            .expect("Cache row present");
        assert!(
            architect_line.contains("1.2M"),
            "Architect tokens compacted: {architect_line}"
        );
        assert!(
            executor_line.contains("500.0k"),
            "Executor tokens compacted: {executor_line}"
        );
        assert!(
            cache_line.contains("30.0%"),
            "Cache hit ratio compacted: {cache_line}"
        );
    }

    #[test]
    fn ledger_tokens_dash_aligns_with_decimal_column() {
        // Tokens mode: a scope's dash — Architect with 0 tokens, the always-—
        // Cache Session cell (no cache activity), and the Cache Project `%`
        // cell — must all sit on the decimal column of `X.Xk`/`X.XM` values.
        let sess = ScopeReport {
            executor_tokens: 500_000, // -> "500.0k" Session column
            architect_tokens: 0,      // -> "—" Session column
            ..Default::default()
        };
        let proj = ScopeReport {
            executor_tokens: 2_000_000,  // -> "2.0M" Project column
            architect_tokens: 5_500_000, // -> "5.5M" Project column
            executor_input: 600_000,
            executor_cache_read: 300_000,
            executor_cache_write: 100_000,
            executor_output: 0,
        };
        let lines = ledger_lines(&sess, None, &proj);
        let texts: Vec<String> = lines.iter().map(|l| l.to_string()).collect();
        let architect = texts
            .iter()
            .find(|s| s.contains("Architect:"))
            .expect("Architect row");
        let executor = texts
            .iter()
            .find(|s| s.contains("Executor:"))
            .expect("Executor row");
        let cache = texts
            .iter()
            .find(|s| s.contains("Cache:"))
            .expect("Cache row");

        let exec_dot = executor
            .find('.')
            .expect("Executor Session column is a k value with a decimal");
        let arch_dash = architect
            .find('—')
            .expect("Architect Session column is a dash");
        let cache_dash = cache
            .find('—')
            .expect("Cache Session column is a dash (no cache activity)");
        assert_eq!(
            arch_dash, exec_dot,
            "Architect dash must align on the Executor decimal\nArchitect: {architect}\nExecutor:  {executor}"
        );
        assert_eq!(
            cache_dash, exec_dot,
            "Cache dash must align on the Executor decimal\nExecutor: {executor}\nCache:    {cache}"
        );

        // Project column: Cache `%`'s decimal (`.` 2 in front of `%`) must align
        // with the Architect Project column's `.`.
        let cache_pct = cache.find('%').expect("Cache row has a % sign");
        let cache_pct_dot = cache_pct - 2;
        let proj_dot = architect
            .find('.')
            .expect("Architect Project column has a decimal");
        assert_eq!(
            cache_pct_dot, proj_dot,
            "Cache % decimal must align with Architect decimal\nArchitect: {architect}\nCache:     {cache}"
        );
        assert!(cache.contains("30.0%"));
    }

    #[test]
    fn ledger_tokens_mode_has_no_parens() {
        let sess = ScopeReport {
            executor_tokens: 500_000,
            architect_tokens: 1_200_000,
            ..Default::default()
        };
        let proj = ScopeReport {
            executor_tokens: 2_000_000,
            architect_tokens: 5_500_000,
            ..Default::default()
        };
        let lines = ledger_lines(&sess, None, &proj);
        for line in &lines[1..] {
            assert!(
                !line.contains('('),
                "token data rows must not contain parens: {line}"
            );
        }
    }

    #[test]
    fn ledger_tokens_header_is_tokens() {
        let sess = ScopeReport::default();
        let proj = ScopeReport::default();
        let lines = ledger_lines(&sess, None, &proj);
        let header = lines.first().expect("header present");
        let header_text = header.to_string();
        assert!(
            header_text.contains("Tokens"),
            "tokens-mode header must contain 'Tokens': {header_text}"
        );
        assert!(
            !header_text.contains("Spend"),
            "tokens-mode header must not contain 'Spend': {header_text}"
        );
    }
}
