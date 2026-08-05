//! Compact the telemetry store — `rexymcp compact` subcommand.
//!
//! Rewrites `phase_runs.jsonl` to keep only the records that still matter,
//! preserving kept lines byte-for-byte (no parse/re-serialize round-trip).
//!
//! **Residual race.** An append that lands between the final tail copy and the
//! atomic rename is lost. The window is sub-millisecond, and the sweep's
//! harvest is idempotent by design (it re-appends full-sum ledger records per
//! key), so the next sweep restores anything lost. This is a deliberate trade
//! against making the user stop `rexymcp serve`.

use std::collections::HashMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use rexymcp_executor::config::Config;
use rexymcp_executor::store::telemetry::TELEMETRY_SCHEMA_VERSION;

/// Borrowed compact inputs from the CLI flags.
pub struct CompactArgs<'a> {
    pub config_path: &'a Path,
    pub telemetry_path: Option<&'a Path>,
    pub ts: u64,
    pub dry_run: bool,
}

/// Per-class drop counts and the overall I/O summary.
pub struct CompactOutcome {
    pub input_lines: usize,
    pub input_bytes: usize,
    pub output_lines: usize,
    pub output_bytes: usize,
    pub dropped_blank: usize,
    pub dropped_malformed: usize,
    pub dropped_legacy_run: usize,
    pub dropped_other: usize,
    pub backup_path: Option<PathBuf>,
}

impl CompactOutcome {
    /// Reduction percentage (0.0 = no reduction, 99.56 = 99.56 % smaller).
    pub fn reduction_pct(&self) -> f64 {
        if self.input_bytes == 0 {
            return 0.0;
        }
        (1.0 - self.output_bytes as f64 / self.input_bytes as f64) * 100.0
    }
}

/// Copy bytes appended to the live store after the initial stat, up to 3 passes.
///
/// Returns the new tail offset (the file length after the last copy).
fn copy_tail(store_path: &Path, tmp: &mut fs::File, mut from: u64) -> Result<u64, String> {
    for _ in 0..3 {
        let current_len = fs::metadata(store_path)
            .map_err(|e| format!("failed to stat store: {}", e))?
            .len();
        if current_len <= from {
            break;
        }
        let new_bytes =
            fs::read(store_path).map_err(|e| format!("failed to read store tail: {}", e))?;
        let tail = &new_bytes[from as usize..];
        if tail.is_empty() {
            break;
        }
        tmp.write_all(tail)
            .map_err(|e| format!("failed to append tail: {}", e))?;
        tmp.flush()
            .map_err(|e| format!("failed to flush tail: {}", e))?;
        from = current_len;
    }
    Ok(from)
}

/// Compact the telemetry store according to the selection rules in the phase
/// spec. On success returns the outcome summary; on error returns a `String`
/// message.
pub fn compact_store(args: &CompactArgs) -> Result<CompactOutcome, String> {
    let cfg = Config::load_with_env(args.config_path)
        .map_err(|e| format!("failed to load config: {}", e))?;

    // Resolve the telemetry file path.
    let store_path: PathBuf = if let Some(p) = args.telemetry_path {
        p.to_path_buf()
    } else if let Some(ref dir) = cfg.telemetry.dir {
        dir.join("phase_runs.jsonl")
    } else {
        return Err(
            "telemetry disabled: cfg.telemetry.dir not set and no --telemetry-path provided"
                .to_string(),
        );
    };

    let telemetry_dir = store_path
        .parent()
        .map(Path::to_path_buf)
        .ok_or_else(|| "invalid telemetry path: no parent directory".to_string())?;

    // Stat the store.
    let metadata = fs::metadata(&store_path)
        .map_err(|e| format!("store not found: {}: {}", store_path.display(), e))?;
    let initial_len = metadata.len();

    // Read the file.
    let content =
        fs::read_to_string(&store_path).map_err(|e| format!("failed to read store: {}", e))?;
    let input_bytes = content.len();

    // Phase 1: select lines from the first `initial_len` bytes.
    let (kept_indices, counts) = select_lines(&content, initial_len as usize);

    // Phase 2: build the output from kept indices.
    let lines: Vec<&str> = content.split('\n').collect();
    let mut output = Vec::with_capacity(content.len());
    let mut output_bytes = 0usize;
    let mut output_lines = 0usize;
    for &idx in &kept_indices {
        let line = lines[idx];
        output.extend_from_slice(line.as_bytes());
        output.push(b'\n');
        output_bytes += line.len() + 1;
        output_lines += 1;
    }

    if args.dry_run {
        let input_lines = if content.ends_with('\n') {
            lines.len() - 1
        } else {
            lines.len()
        };
        return Ok(CompactOutcome {
            input_lines,
            input_bytes,
            output_lines,
            output_bytes,
            dropped_blank: counts.blank,
            dropped_malformed: counts.malformed,
            dropped_legacy_run: counts.legacy_run,
            dropped_other: counts.other,
            backup_path: None,
        });
    }

    // Write kept lines to a temp file in the same directory.
    let tmp_path = telemetry_dir.join("phase_runs.jsonl.compact-tmp");
    let mut tmp =
        fs::File::create(&tmp_path).map_err(|e| format!("failed to create temp file: {}", e))?;
    tmp.write_all(&output)
        .map_err(|e| format!("failed to write temp file: {}", e))?;
    tmp.flush()
        .map_err(|e| format!("failed to flush temp file: {}", e))?;

    copy_tail(&store_path, &mut tmp, initial_len)
        .map_err(|e| format!("tail-copy failed: {}", e))?;

    // Phase 4: backup the original store.
    let backup_name = format!("phase_runs.jsonl.bak-compact-{}", args.ts);
    let backup_path = telemetry_dir.join(&backup_name);
    fs::copy(&store_path, &backup_path).map_err(|e| format!("failed to create backup: {}", e))?;

    // Phase 5: atomic rename.
    fs::rename(&tmp_path, &store_path)
        .map_err(|e| format!("failed to rename compacted store: {}", e))?;

    // Recount output bytes from the final file.
    let final_bytes = fs::metadata(&store_path)
        .map_err(|e| format!("failed to stat compacted store: {}", e))?
        .len() as usize;

    Ok(CompactOutcome {
        input_lines: if content.ends_with('\n') {
            lines.len() - 1
        } else {
            lines.len()
        },
        input_bytes,
        output_lines,
        output_bytes: final_bytes,
        dropped_blank: counts.blank,
        dropped_malformed: counts.malformed,
        dropped_legacy_run: counts.legacy_run,
        dropped_other: counts.other,
        backup_path: Some(backup_path),
    })
}

/// Drop counts by class.
#[derive(Default)]
struct DropCounts {
    blank: usize,
    malformed: usize,
    legacy_run: usize,
    other: usize,
}

/// Select which lines to keep, returning `(kept_indices, drop_counts)`.
///
/// `content` is the full file content; `initial_len` is the byte offset up to
/// which compaction rules apply (bytes beyond this are tail-copied verbatim).
fn select_lines(content: &str, initial_len: usize) -> (Vec<usize>, DropCounts) {
    let mut counts = DropCounts::default();
    let mut kept_indices: Vec<usize> = Vec::new();

    // Ledger fold: last-write-wins per (project_id, session_id, model, skill).
    let mut ledger_latest: HashMap<(Option<String>, String, String, String), usize> =
        HashMap::new();
    let mut ledger_out: Vec<usize> = Vec::new();

    // Activity fold: last-write-wins per (phase_id, activity, ts).
    let mut activity_latest: HashMap<(String, String, u64), usize> = HashMap::new();
    let mut activity_out: Vec<usize> = Vec::new();

    let lines: Vec<&str> = content.split('\n').collect();

    // If the content ends with '\n', the last element from split is an empty
    // string that does not correspond to a real line — skip it.
    let effective_len = if content.ends_with('\n') {
        lines.len() - 1
    } else {
        lines.len()
    };

    for line_idx in 0..effective_len {
        let line = lines[line_idx];
        // If this line starts beyond the initial_len, it's tail data — keep it.
        let line_start = byte_offset_of_line(&lines, line_idx);
        if line_start >= initial_len {
            kept_indices.push(line_idx);
            continue;
        }

        // Blank line.
        if line.trim().is_empty() {
            counts.blank += 1;
            continue;
        }

        // Try to parse enough to classify.
        let val = match serde_json::from_str::<serde_json::Value>(line) {
            Ok(v) => v,
            Err(_) => {
                counts.malformed += 1;
                continue;
            }
        };

        let schema_version = val
            .get("schema_version")
            .and_then(|v| v.as_u64())
            .unwrap_or(0);

        if schema_version != TELEMETRY_SCHEMA_VERSION as u64 {
            // Check if it's a PhaseRun without schema_version (legacy).
            let record = val.get("record").and_then(|v| v.as_str()).unwrap_or("");
            if record.is_empty() && schema_version == 0 {
                counts.legacy_run += 1;
            } else {
                counts.other += 1;
            }
            continue;
        }

        let record = val.get("record").and_then(|v| v.as_str()).unwrap_or("");

        match record {
            "architect_ledger" => {
                let key = parse_ledger_key(&val);
                if let Some(key) = key {
                    if let Some(&out_idx) = ledger_latest.get(&key) {
                        ledger_out[out_idx] = line_idx;
                    } else {
                        ledger_latest.insert(key, ledger_out.len());
                        ledger_out.push(line_idx);
                    }
                } else {
                    counts.other += 1;
                }
            }
            "architect_activity" => {
                let key = parse_activity_key(&val);
                if let Some(key) = key {
                    if let Some(&out_idx) = activity_latest.get(&key) {
                        activity_out[out_idx] = line_idx;
                    } else {
                        activity_latest.insert(key, activity_out.len());
                        activity_out.push(line_idx);
                    }
                } else {
                    counts.other += 1;
                }
            }
            "review" => {
                kept_indices.push(line_idx);
            }
            "" => {
                // PhaseRun with schema_version == 1 — keep.
                kept_indices.push(line_idx);
            }
            _ => {
                counts.other += 1;
            }
        }
    }

    // Collect fold winners, preserving their insertion order.
    for &idx in &ledger_out {
        kept_indices.push(idx);
    }
    for &idx in &activity_out {
        kept_indices.push(idx);
    }

    // Sort by original file order.
    kept_indices.sort_unstable();

    (kept_indices, counts)
}

/// Compute the byte offset of line `line_idx` in the original string.
fn byte_offset_of_line(lines: &[&str], line_idx: usize) -> usize {
    let mut offset = 0;
    for line in lines.iter().take(line_idx) {
        offset += line.len() + 1; // +1 for the '\n' separator
    }
    offset
}

/// Parse the fold key from an architect_ledger JSON value.
fn parse_ledger_key(val: &serde_json::Value) -> Option<(Option<String>, String, String, String)> {
    let project_id = val
        .get("project_id")
        .and_then(|v| v.as_str())
        .map(str::to_string);
    let session_id = val.get("session_id")?.as_str()?.to_string();
    let model = val.get("model")?.as_str()?.to_string();
    let skill = val.get("skill")?.as_str()?.to_string();
    Some((project_id, session_id, model, skill))
}

/// Parse the fold key from an architect_activity JSON value.
fn parse_activity_key(val: &serde_json::Value) -> Option<(String, String, u64)> {
    let phase_id = val.get("phase_id")?.as_str()?.to_string();
    let activity = val.get("activity")?.as_str()?.to_string();
    let ts = val.get("ts")?.as_u64()?;
    Some((phase_id, activity, ts))
}

/// Format a human-readable compact report.
pub fn format_compact_report(outcome: &CompactOutcome) -> String {
    let mut lines = Vec::new();

    lines.push("=== Compact Report ===".to_string());
    lines.push(format!(
        "Input:  {} lines, {} bytes",
        outcome.input_lines, outcome.input_bytes
    ));
    lines.push(format!(
        "Output: {} lines, {} bytes",
        outcome.output_lines, outcome.output_bytes
    ));
    lines.push(format!("Reduction: {:.2}%", outcome.reduction_pct()));
    lines.push(String::new());
    lines.push("Dropped:".to_string());
    lines.push(format!("  blank:        {}", outcome.dropped_blank));
    lines.push(format!("  malformed:    {}", outcome.dropped_malformed));
    lines.push(format!("  legacy_run:   {}", outcome.dropped_legacy_run));
    lines.push(format!("  other:        {}", outcome.dropped_other));
    lines.push(String::new());

    if outcome.dry_run() {
        lines.push("(dry run — nothing was written)".to_string());
    } else if let Some(ref p) = outcome.backup_path {
        lines.push(format!("Backup: {}", p.display()));
    }

    lines.join("\n")
}

impl CompactOutcome {
    fn dry_run(&self) -> bool {
        self.backup_path.is_none()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn make_config(temp_dir: &TempDir) -> PathBuf {
        let telemetry_dir = temp_dir.path().join("telemetry");
        fs::create_dir_all(&telemetry_dir).unwrap();
        let config_path = temp_dir.path().join("rexymcp.toml");
        fs::write(
            &config_path,
            format!(
                r#"[project]
id = "test-project"

[executor]
provider = "openai"
base_url = "http://localhost:8000/v1"
model = "qwen"

[telemetry]
dir = "{}"
"#,
                telemetry_dir.display()
            ),
        )
        .unwrap();
        config_path
    }

    fn ledger_line(
        project_id: &str,
        session_id: &str,
        model: &str,
        skill: &str,
        messages: u64,
    ) -> String {
        format!(
            r#"{{"record":"architect_ledger","schema_version":1,"project_id":"{}","session_id":"{}","model":"{}","skill":"{}","tokens":{{"input":0,"cache_creation":0,"cache_read":0,"output":0}},"cache_creation_5m":0,"cache_creation_1h":0,"messages":{},"last_ts":0}}"#,
            project_id, session_id, model, skill, messages
        )
    }

    fn activity_line(phase_id: &str, activity: &str, ts: u64, outcome: &str) -> String {
        format!(
            r#"{{"record":"architect_activity","schema_version":1,"phase_id":"{}","activity":"{}","ts":{},"project_id":null,"phase_doc_path":null,"milestone_id":null,"outcome":"{}","model":null,"tokens":{{"input":0,"cache_creation":0,"cache_read":0,"output":0}}}}"#,
            phase_id, activity, ts, outcome
        )
    }

    fn review_line(phase_id: &str) -> String {
        format!(
            r#"{{"record":"review","schema_version":1,"ts":1717000000000,"phase_id":"{}","project_id":null,"phase_doc_path":null,"architect_verdict":"approved_first_try","bounces_to_approval":null,"bugs_filed":null,"warnings":null,"failure_class":[]}}"#,
            phase_id
        )
    }

    fn stamped_run_line() -> String {
        r#"{"ts":1717000000000,"schema_version":1,"model":"qwen","generation_params":{"temperature":null,"seed":null},"phase_id":"phase-01","phase_doc_path":null,"tags":[],"status":"complete","escalated":false,"gates":{"fmt":true,"build":true,"lint":true,"test":true},"parse_failure_rate":0.0,"repairs_per_call":0.0,"verifier_retries":0,"tool_success_rate":1.0,"turns":10,"wall_clock_s":60.0,"gen_time_s":30.0,"tokens":{"input_tokens":0,"output_tokens":0,"cache_read_tokens":0,"cache_write_tokens":0},"warnings":null,"bugs_filed":null,"bounces_to_approval":null,"architect_verdict":null,"served_model":null,"length_finish_rate":null,"context_window":null,"context_efficiency":{"peak_context_pct":0.0,"compaction_count":0,"compaction_tokens_reclaimed":0,"output_filtered_tokens":0,"read_evicted_tokens":0,"read_deduped_tokens":0},"project_id":null,"milestone_id":null,"tier_telemetry":{"tier":null}}"#
            .to_string()
    }

    fn unstamped_run_line() -> String {
        r#"{"ts":1700000000000,"model":"qwen","generation_params":{"temperature":null,"seed":null},"phase_id":"phase-00","phase_doc_path":null,"tags":[],"status":"complete","escalated":false,"gates":{"fmt":true,"build":true,"lint":true,"test":true},"parse_failure_rate":0.0,"repairs_per_call":0.0,"verifier_retries":0,"tool_success_rate":1.0,"turns":5,"wall_clock_s":30.0,"gen_time_s":15.0,"tokens":{"input_tokens":0,"output_tokens":0,"cache_read_tokens":0,"cache_write_tokens":0},"warnings":null,"bugs_filed":null,"bounces_to_approval":null,"architect_verdict":null}"#
            .to_string()
    }

    #[test]
    fn compact_keeps_only_the_last_ledger_per_key() {
        let dir = TempDir::new().unwrap();
        let config = make_config(&dir);
        let store = dir.path().join("telemetry").join("phase_runs.jsonl");

        // Three ledger lines, two sharing the same key with different `messages`.
        let content = format!(
            "{}\n{}\n{}\n",
            ledger_line("p1", "s1", "m1", "skill1", 10),
            ledger_line("p1", "s2", "m1", "skill1", 20), // different session — different key
            ledger_line("p1", "s1", "m1", "skill1", 30), // same key as first — wins
        );
        fs::write(&store, &content).unwrap();

        let args = CompactArgs {
            config_path: &config,
            telemetry_path: Some(&store),
            ts: 1_717_000_000_000,
            dry_run: false,
        };
        let outcome = compact_store(&args).unwrap();

        assert_eq!(outcome.output_lines, 2);
        // The surviving duplicate for key (p1,s1,m1,skill1) is the later one (messages=30).
        let output_content = fs::read_to_string(&store).unwrap();
        assert!(
            output_content.contains("\"messages\":30"),
            "the later ledger (messages=30) must survive, not the earlier one"
        );
        assert!(
            !output_content.contains("\"messages\":10"),
            "the earlier ledger (messages=10) must be dropped"
        );
    }

    #[test]
    fn compact_keeps_only_the_last_activity_per_key() {
        let dir = TempDir::new().unwrap();
        let config = make_config(&dir);
        let store = dir.path().join("telemetry").join("phase_runs.jsonl");

        // Three activity lines, two sharing the same key with different `outcome`.
        let content = format!(
            "{}\n{}\n{}\n",
            activity_line("phase-01", "design", 1000, "first"),
            activity_line("phase-02", "design", 1000, "other"), // different phase — different key
            activity_line("phase-01", "design", 1000, "last"),  // same key as first — wins
        );
        fs::write(&store, &content).unwrap();

        let args = CompactArgs {
            config_path: &config,
            telemetry_path: Some(&store),
            ts: 1_717_000_000_000,
            dry_run: false,
        };
        let outcome = compact_store(&args).unwrap();

        assert_eq!(outcome.output_lines, 2);
        // The surviving duplicate for key (phase-01, design, 1000) is the later one (outcome="last").
        let output_content = fs::read_to_string(&store).unwrap();
        assert!(
            output_content.contains("\"outcome\":\"last\""),
            "the later activity (outcome=last) must survive, not the earlier one"
        );
        assert!(
            !output_content.contains("\"outcome\":\"first\""),
            "the earlier activity (outcome=first) must be dropped"
        );
    }

    #[test]
    fn compact_keeps_all_reviews_and_stamped_runs() {
        let dir = TempDir::new().unwrap();
        let config = make_config(&dir);
        let store = dir.path().join("telemetry").join("phase_runs.jsonl");

        let content = format!("{}\n{}\n", review_line("phase-01"), stamped_run_line());
        fs::write(&store, &content).unwrap();

        let args = CompactArgs {
            config_path: &config,
            telemetry_path: Some(&store),
            ts: 1_717_000_000_000,
            dry_run: true,
        };
        let outcome = compact_store(&args).unwrap();

        assert_eq!(outcome.output_lines, 2);
        assert_eq!(outcome.dropped_blank, 0);
        assert_eq!(outcome.dropped_malformed, 0);
        assert_eq!(outcome.dropped_legacy_run, 0);
    }

    #[test]
    fn compact_drops_unversioned_phase_runs() {
        let dir = TempDir::new().unwrap();
        let config = make_config(&dir);
        let store = dir.path().join("telemetry").join("phase_runs.jsonl");

        let content = format!("{}\n{}\n", stamped_run_line(), unstamped_run_line());
        fs::write(&store, &content).unwrap();

        let args = CompactArgs {
            config_path: &config,
            telemetry_path: Some(&store),
            ts: 1_717_000_000_000,
            dry_run: true,
        };
        let outcome = compact_store(&args).unwrap();

        assert_eq!(outcome.output_lines, 1);
        assert_eq!(outcome.dropped_legacy_run, 1);
    }

    #[test]
    fn compact_drops_malformed_and_blank_lines() {
        let dir = TempDir::new().unwrap();
        let config = make_config(&dir);
        let store = dir.path().join("telemetry").join("phase_runs.jsonl");

        // A blank line and a line with two concatenated JSON objects.
        let content = format!(
            "{}\n{}\n{}\n",
            "",                  // blank
            r#"{"a":1}{"b":2}"#, // malformed: two objects on one line
            review_line("phase-01"),
        );
        fs::write(&store, &content).unwrap();

        let args = CompactArgs {
            config_path: &config,
            telemetry_path: Some(&store),
            ts: 1_717_000_000_000,
            dry_run: true,
        };
        let outcome = compact_store(&args).unwrap();

        assert_eq!(outcome.output_lines, 1);
        assert_eq!(outcome.dropped_blank, 1);
        assert_eq!(outcome.dropped_malformed, 1);
    }

    #[test]
    fn compact_preserves_kept_lines_byte_for_byte() {
        let dir = TempDir::new().unwrap();
        let config = make_config(&dir);
        let store = dir.path().join("telemetry").join("phase_runs.jsonl");

        // A ledger line with deliberately unusual key order and extra whitespace.
        let original_line = r#"  { "record" : "architect_ledger" , "schema_version" : 1 , "project_id" : "p1" , "session_id" : "s1" , "model" : "m1" , "skill" : "skill1" , "tokens" : { "input" : 0 , "cache_creation" : 0 , "cache_read" : 0 , "output" : 0 } , "cache_creation_5m" : 0 , "cache_creation_1h" : 0 , "messages" : 5 , "last_ts" : 0 }  "#;
        fs::write(&store, format!("{}\n", original_line)).unwrap();

        let args = CompactArgs {
            config_path: &config,
            telemetry_path: Some(&store),
            ts: 1_717_000_000_000,
            dry_run: true,
        };
        let outcome = compact_store(&args).unwrap();

        assert_eq!(outcome.output_lines, 1);

        // Read back the store (unchanged in dry-run) and verify the line is byte-identical.
        let output_content = fs::read_to_string(&store).unwrap();
        let output_lines: Vec<&str> = output_content.split('\n').collect();
        assert_eq!(
            output_lines[0], original_line,
            "kept line must be byte-for-byte identical (no re-serialize)"
        );
    }

    #[test]
    fn compact_output_preserves_file_order() {
        let dir = TempDir::new().unwrap();
        let config = make_config(&dir);
        let store = dir.path().join("telemetry").join("phase_runs.jsonl");

        // Interleave ledgers with reviews so fold winners are not contiguous.
        let content = format!(
            "{}\n{}\n{}\n{}\n{}\n",
            ledger_line("p1", "s1", "m1", "skill1", 10),
            review_line("phase-01"),
            ledger_line("p1", "s2", "m1", "skill1", 20),
            review_line("phase-02"),
            ledger_line("p1", "s1", "m1", "skill1", 30), // wins over first
        );
        fs::write(&store, &content).unwrap();

        let args = CompactArgs {
            config_path: &config,
            telemetry_path: Some(&store),
            ts: 1_717_000_000_000,
            dry_run: false,
        };
        let outcome = compact_store(&args).unwrap();

        assert_eq!(outcome.output_lines, 4);

        // Output order follows original file order of kept lines:
        // idx 1: review(phase-01), idx 2: ledger(s2), idx 3: review(phase-02),
        // idx 4: ledger(s1-winner, messages=30)
        let output_content = fs::read_to_string(&store).unwrap();
        let output_lines: Vec<&str> = output_content
            .split('\n')
            .filter(|l| !l.is_empty())
            .collect();

        assert!(output_lines[0].contains("phase-01")); // review at idx 1
        assert!(output_lines[1].contains("s2")); // ledger at idx 2
        assert!(output_lines[2].contains("phase-02")); // review at idx 3
        assert!(output_lines[3].contains("messages\":30")); // winning ledger at idx 4
    }

    #[test]
    fn compact_dry_run_writes_nothing() {
        let dir = TempDir::new().unwrap();
        let config = make_config(&dir);
        let store = dir.path().join("telemetry").join("phase_runs.jsonl");

        let content = format!("{}\n{}\n", ledger_line("p1", "s1", "m1", "skill1", 10), "");
        fs::write(&store, &content).unwrap();
        let original_bytes = fs::read(&store).unwrap();

        let args = CompactArgs {
            config_path: &config,
            telemetry_path: Some(&store),
            ts: 1_717_000_000_000,
            dry_run: true,
        };
        let outcome = compact_store(&args).unwrap();

        // Store is byte-identical.
        assert_eq!(fs::read(&store).unwrap(), original_bytes);

        // No backup or temp file created.
        let telemetry_dir = dir.path().join("telemetry");
        let entries: Vec<_> = fs::read_dir(&telemetry_dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().to_string())
            .collect();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0], "phase_runs.jsonl");
        assert!(outcome.backup_path.is_none());
    }

    #[test]
    fn compact_writes_backup_before_replacing() {
        let dir = TempDir::new().unwrap();
        let config = make_config(&dir);
        let store = dir.path().join("telemetry").join("phase_runs.jsonl");

        let content = format!("{}\n{}\n", ledger_line("p1", "s1", "m1", "skill1", 10), "");
        fs::write(&store, &content).unwrap();
        let original_bytes = fs::read(&store).unwrap();

        let ts = 1_717_000_000_000u64;
        let args = CompactArgs {
            config_path: &config,
            telemetry_path: Some(&store),
            ts,
            dry_run: false,
        };
        let outcome = compact_store(&args).unwrap();

        // Backup exists and is byte-identical to original.
        let backup_path = outcome.backup_path.as_ref().unwrap();
        assert!(backup_path.exists());
        assert_eq!(fs::read(backup_path).unwrap(), original_bytes);

        // Backup name carries the injected ts.
        assert!(
            backup_path
                .file_name()
                .unwrap()
                .to_string_lossy()
                .contains(&ts.to_string())
        );
    }

    #[test]
    fn compact_preserves_bytes_appended_during_the_run() {
        // Test copy_tail directly: write a file, record a length, append past
        // that length, call copy_tail, and assert the appended bytes landed in
        // the temp file verbatim.
        let dir = TempDir::new().unwrap();
        let store = dir.path().join("store.jsonl");
        let tmp_path = dir.path().join("tmp.jsonl");

        let initial = "line1\nline2\n";
        fs::write(&store, initial).unwrap();
        let initial_len = initial.len() as u64;

        // Append after the initial content.
        let appended = format!("{}\n", stamped_run_line());
        let mut file = fs::OpenOptions::new().append(true).open(&store).unwrap();
        file.write_all(appended.as_bytes()).unwrap();
        drop(file);

        // Verify the store now has both.
        let store_content = fs::read_to_string(&store).unwrap();
        assert!(store_content.contains("line1"));
        assert!(store_content.contains("\"turns\":10"));

        // Now copy_tail should bring the appended bytes into the temp file.
        let mut tmp = fs::File::create(&tmp_path).unwrap();
        let new_offset = copy_tail(&store, &mut tmp, initial_len).unwrap();
        drop(tmp);

        // The tail offset advanced past the appended data.
        assert!(new_offset > initial_len);

        // The temp file contains the appended run.
        let tmp_content = fs::read_to_string(&tmp_path).unwrap();
        assert!(
            tmp_content.contains("\"turns\":10"),
            "appended bytes must appear in the temp file after copy_tail"
        );
        assert!(
            !tmp_content.contains("line1"),
            "copy_tail must not re-copy the initial content"
        );
    }

    #[test]
    fn compact_on_missing_store_is_an_error_not_a_panic() {
        let dir = TempDir::new().unwrap();
        let config = make_config(&dir);
        let nonexistent = dir.path().join("telemetry").join("nonexistent.jsonl");

        let args = CompactArgs {
            config_path: &config,
            telemetry_path: Some(&nonexistent),
            ts: 1_717_000_000_000,
            dry_run: false,
        };

        let result = compact_store(&args);
        assert!(result.is_err());
        // No files created.
        assert!(!nonexistent.exists());
    }
}
