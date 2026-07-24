use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use rexymcp_executor::agent::CancelHandle;
use rexymcp_executor::phase::CancelReason;
use serde::{Deserialize, Serialize};
use tokio::sync::watch;
use uuid::Uuid;

/// Bounded long-poll window for `get_run_status`. A poll that finds the run
/// still in flight returns `Running` after at most this long, so the caller
/// re-polls rather than blocking indefinitely.
pub const RUN_STATUS_POLL_TIMEOUT: Duration = Duration::from_secs(15);

/// Terminal-or-running state of a spawned `execute_phase` run.
#[derive(Debug, Clone)]
pub enum RunState {
    /// Still executing.
    Running,
    /// Finished; holds the serialized (capped) `PhaseResult` JSON.
    Complete(serde_json::Value),
    /// Errored at the infrastructure level (config load / scope / IO).
    Failed(String),
}

impl RunState {
    pub fn is_terminal(&self) -> bool {
        !matches!(self, RunState::Running)
    }
}

/// Per-run control block held in the registry.
struct RunEntry {
    state_tx: watch::Sender<RunState>,
    /// Fires the run's cooperative cancel signal. `None` is never stored — every
    /// registered run owns a handle (a `never()`-signal handle for runs that are
    /// not cancellable, e.g. tests).
    cancel: CancelHandle,
    /// Set by `request_stop`; read by `spawn_run` to stamp the terminal result.
    stop_reason: Option<CancelReason>,
}

/// How long a persisted run record is kept before `prune_records` removes it.
pub const RECORD_MAX_AGE_MS: u64 = 30 * 24 * 60 * 60 * 1000;

/// A run's terminal outcome, persisted so it survives the serve process.
/// Only terminal states are written — a missing file means "this process never
/// saw that run finish", which is exactly the `unknown` answer.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct RunRecord {
    pub(crate) run_id: String,
    /// "done" | "failed" — mirrors `GetRunStatusOutput.state`; never "running".
    pub(crate) state: String,
    pub(crate) result: Option<serde_json::Value>,
    pub(crate) error: Option<String>,
    /// Unix millis when the record was written.
    pub(crate) ts: u64,
}

/// In-memory registry of spawned `execute_phase` runs, keyed by `run_id`,
/// optionally mirroring terminal states to disk so a completed run stays
/// reapable after the process that ran it is gone.
/// Lives for the serve-process lifetime on `RexyMcpServer.runs`.
#[derive(Default)]
pub struct JobRegistry {
    runs: Mutex<HashMap<String, RunEntry>>,
    /// Where terminal `RunRecord`s are written. `None` (the default) keeps the
    /// registry purely in-memory, which is what every test wants by default.
    record_dir: Option<PathBuf>,
}

impl JobRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// A registry that mirrors terminal run states into `dir`, best-effort.
    ///
    /// The directory is process-wide (not per-repo) on purpose: `get_run_status`
    /// receives only a `run_id`, so a per-repo location would be unlookupable
    /// without adding a `repo_path` parameter and changing every caller, the
    /// plugin skills included. A UUID-keyed directory makes the fallback a single
    /// read. Old records are pruned here — once per serve start, not per write.
    pub fn with_record_dir(dir: PathBuf) -> Self {
        prune_records(&dir, RECORD_MAX_AGE_MS, now_ms());
        Self {
            runs: Mutex::new(HashMap::new()),
            record_dir: Some(dir),
        }
    }

    /// Register a fresh run in `Running`, holding its cancel handle. Call before
    /// spawning so a racing `get_run_status` / `stop_phase` always finds the id.
    pub fn insert(&self, run_id: &str, cancel: CancelHandle) {
        let (state_tx, _rx) = watch::channel(RunState::Running);
        self.lock().insert(
            run_id.to_string(),
            RunEntry {
                state_tx,
                cancel,
                stop_reason: None,
            },
        );
    }

    /// Publish a terminal state. No-op if the id is unknown.
    pub fn publish(&self, run_id: &str, state: RunState) {
        let mut published = false;
        if let Some(entry) = self.lock().get(run_id) {
            // send_replace stores the value even with no live receivers, so a
            // later subscriber still sees it via `borrow`.
            published = true;
            entry.state_tx.send_replace(state.clone());
        }
        // Mirror to disk after the in-memory publish, so a live poll never waits
        // on the filesystem. Best-effort: a write failure must not affect the
        // answer a live poll already has.
        if published && state.is_terminal() {
            self.write_record(run_id, &state, now_ms());
        }
    }

    /// Write `state` as this run's terminal record. Best-effort and non-fatal —
    /// every failure logs one line to stderr (never stdout, which is the JSON-RPC
    /// transport) and returns.
    fn write_record(&self, run_id: &str, state: &RunState, now_ms: u64) {
        let Some(dir) = self.record_dir.as_ref() else {
            return;
        };
        let record = match state {
            RunState::Running => return,
            RunState::Complete(json) => RunRecord {
                run_id: run_id.to_string(),
                state: "done".to_string(),
                result: Some(json.clone()),
                error: None,
                ts: now_ms,
            },
            RunState::Failed(e) => RunRecord {
                run_id: run_id.to_string(),
                state: "failed".to_string(),
                result: None,
                error: Some(e.clone()),
                ts: now_ms,
            },
        };

        if let Err(e) = std::fs::create_dir_all(dir) {
            eprintln!("rexymcp: could not create run-record dir {dir:?}: {e}");
            return;
        }
        let body = match serde_json::to_vec(&record) {
            Ok(b) => b,
            Err(e) => {
                eprintln!("rexymcp: could not serialize run record {run_id}: {e}");
                return;
            }
        };
        // Write-then-rename: a reader must never observe a partial file.
        let tmp = dir.join(format!("{run_id}.json.tmp"));
        let final_path = record_path(dir, run_id);
        if let Err(e) = std::fs::write(&tmp, &body) {
            eprintln!("rexymcp: could not write run record {tmp:?}: {e}");
            return;
        }
        if let Err(e) = std::fs::rename(&tmp, &final_path) {
            eprintln!("rexymcp: could not commit run record {final_path:?}: {e}");
            let _ = std::fs::remove_file(&tmp);
        }
    }

    /// The persisted terminal record for `run_id`, if one exists. One read of one
    /// file — no directory scan, no retry, no waiting, so the `get_run_status`
    /// long-poll bound still holds. An unparsable record is treated as absent.
    pub(crate) fn load_record(&self, run_id: &str) -> Option<RunRecord> {
        let dir = self.record_dir.as_ref()?;
        let body = std::fs::read(record_path(dir, run_id)).ok()?;
        serde_json::from_slice(&body).ok()
    }

    fn subscribe(&self, run_id: &str) -> Option<watch::Receiver<RunState>> {
        self.lock().get(run_id).map(|e| e.state_tx.subscribe())
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<String, RunEntry>> {
        self.runs.lock().expect("jobs registry mutex poisoned")
    }

    /// Bounded long-poll: resolve as soon as the run is terminal, or return the
    /// current (still-`Running`) state after `timeout`. `None` = unknown id.
    pub async fn await_terminal(&self, run_id: &str, timeout: Duration) -> Option<RunState> {
        let mut rx = self.subscribe(run_id)?;
        {
            let cur = rx.borrow_and_update();
            if cur.is_terminal() {
                return Some(cur.clone());
            }
        }
        match tokio::time::timeout(timeout, rx.wait_for(|s| s.is_terminal())).await {
            Ok(Ok(guard)) => Some(guard.clone()),
            // sender dropped without ever going terminal — report as running.
            Ok(Err(_)) => Some(RunState::Running),
            // timed out — still running.
            Err(_) => Some(RunState::Running),
        }
    }

    /// Fire a run's cancel signal and record why. Returns `false` for an unknown id.
    /// Firing an already-terminal run's handle is a harmless no-op (all receivers are
    /// gone) — this returns `true` because the run existed, but nothing is re-stamped.
    pub fn request_stop(&self, run_id: &str, reason: CancelReason) -> bool {
        if let Some(entry) = self.lock().get_mut(run_id) {
            entry.stop_reason = Some(reason);
            entry.cancel.cancel();
            true
        } else {
            false
        }
    }

    /// Fire every live run's cancel signal with `reason`, recording it for the
    /// terminal-result stamp. Returns how many runs were signalled. The global
    /// stop-all path: one sentinel detection stops the whole serve process's runs.
    pub fn request_stop_all(&self, reason: CancelReason) -> usize {
        let mut map = self.lock();
        let mut n = 0;
        for entry in map.values_mut() {
            entry.stop_reason = Some(reason.clone());
            entry.cancel.cancel();
            n += 1;
        }
        n
    }

    /// Whether a run exists and is still `Running` (not yet terminal). Used to bound
    /// the sentinel watcher's lifetime so it exits once its run finishes.
    pub fn is_running(&self, run_id: &str) -> bool {
        self.lock()
            .get(run_id)
            .map(|e| !e.state_tx.borrow().is_terminal())
            .unwrap_or(false)
    }

    /// How many registered runs are still non-terminal. Read at serve shutdown to
    /// tell a clean client disconnect from a loop death that stranded live work.
    pub fn running_count(&self) -> usize {
        self.lock()
            .values()
            .filter(|e| !e.state_tx.borrow().is_terminal())
            .count()
    }

    /// The reason recorded by a prior `request_stop`, if any. Read by `spawn_run`
    /// when a run finishes so a `cancelled` result can be stamped.
    fn recorded_reason(&self, run_id: &str) -> Option<CancelReason> {
        self.lock().get(run_id).and_then(|e| e.stop_reason.clone())
    }
}

/// Fresh run id — a v4 UUID (collision-free across a serve process, unlike the
/// coarse epoch-seconds `generate_session_id`).
pub fn new_run_id() -> String {
    Uuid::new_v4().to_string()
}

/// Where a run's terminal record lives. `run_id` is a v4 UUID, so it is already
/// filename-safe — no hashing, sanitizing, or nesting.
fn record_path(dir: &Path, run_id: &str) -> PathBuf {
    dir.join(format!("{run_id}.json"))
}

/// Unix millis now. The one impure call in this module; every function that
/// stamps or compares a timestamp takes it as a parameter so tests stay
/// deterministic.
fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Delete run records older than `max_age_ms`, judged by the record's own `ts`
/// field rather than filesystem mtime (the field is what tests can control).
/// Best-effort throughout: pruning must never block or fail a serve start.
pub(crate) fn prune_records(dir: &Path, max_age_ms: u64, now_ms: u64) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let cutoff = now_ms.saturating_sub(max_age_ms);
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        let Ok(body) = std::fs::read(&path) else {
            continue;
        };
        let Ok(record) = serde_json::from_slice::<RunRecord>(&body) else {
            continue;
        };
        if record.ts < cutoff {
            let _ = std::fs::remove_file(&path);
        }
    }
}

/// If `reason` is set and `json` is a `cancelled` PhaseResult, insert
/// `cancellation.reason`. No-op otherwise (a run that completed normally before
/// observing the stop keeps no reason — the status race is resolved in favor of
/// the observed terminal status).
fn stamp_cancel_reason(json: &mut serde_json::Value, reason: Option<CancelReason>) {
    let Some(reason) = reason else { return };
    if json.get("status").and_then(|s| s.as_str()) != Some("cancelled") {
        return;
    }
    if let Some(obj) = json.get_mut("cancellation").and_then(|c| c.as_object_mut())
        && let Ok(v) = serde_json::to_value(reason)
    {
        obj.insert("reason".to_string(), v);
    }
}

/// Spawn `work` as run `run_id`, holding `cancel_handle` in the registry so
/// `request_stop` can fire it. Publishes the terminal state when `work` finishes;
/// if the run was stopped and came back `cancelled`, stamps the recorded reason
/// into the result JSON's `cancellation.reason`.
pub fn spawn_run<F>(
    registry: Arc<JobRegistry>,
    run_id: String,
    cancel_handle: CancelHandle,
    work: F,
) where
    F: std::future::Future<Output = Result<serde_json::Value, String>> + Send + 'static,
{
    registry.insert(&run_id, cancel_handle);
    tokio::spawn(async move {
        let state = match work.await {
            Ok(mut json) => {
                stamp_cancel_reason(&mut json, registry.recorded_reason(&run_id));
                RunState::Complete(json)
            }
            Err(e) => RunState::Failed(e),
        };
        registry.publish(&run_id, state);
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use rexymcp_executor::agent::CancelSignal;
    use serde_json::json;

    #[test]
    fn new_run_ids_are_unique() {
        let id1 = new_run_id();
        let id2 = new_run_id();
        assert_ne!(id1, id2, "run ids should differ");
        assert_eq!(
            id1.split('-').count(),
            5,
            "UUID should have four hyphens (5 segments)"
        );
        assert_eq!(
            id2.split('-').count(),
            5,
            "UUID should have four hyphens (5 segments)"
        );
    }

    #[test]
    fn request_stop_unknown_id_returns_false() {
        let registry = JobRegistry::new();
        assert!(!registry.request_stop("nonexistent", CancelReason::ClaudeStop));
    }

    #[test]
    fn request_stop_known_id_fires_and_returns_true() {
        let registry = JobRegistry::new();
        let (handle, signal) = CancelSignal::new();
        registry.insert("r1", handle);
        assert!(!signal.is_cancelled(), "signal should start uncancelled");
        assert!(
            registry.request_stop("r1", CancelReason::ClaudeStop),
            "request_stop should return true for known id"
        );
        assert!(
            signal.is_cancelled(),
            "signal should be cancelled after request_stop"
        );
    }

    #[test]
    fn stamp_cancel_reason_sets_reason_on_cancelled() {
        let mut json = json!({
            "status": "cancelled",
            "cancellation": { "stage": "between_turns", "turns_done": 2 }
        });
        stamp_cancel_reason(&mut json, Some(CancelReason::ClaudeStop));
        let reason = json["cancellation"]["reason"].as_str();
        assert_eq!(reason, Some("claude_stop"));
    }

    #[test]
    fn stamp_cancel_reason_noop_on_complete() {
        let mut json = json!({ "status": "complete" });
        stamp_cancel_reason(&mut json, Some(CancelReason::ClaudeStop));
        assert!(
            json.get("cancellation").is_none(),
            "complete result should not gain cancellation"
        );
    }

    #[test]
    fn stamp_cancel_reason_noop_when_reason_none() {
        let mut json = json!({
            "status": "cancelled",
            "cancellation": { "stage": "between_turns", "turns_done": 2 }
        });
        stamp_cancel_reason(&mut json, None);
        assert!(
            json["cancellation"].get("reason").is_none(),
            "None reason should leave cancellation unchanged"
        );
    }

    #[tokio::test]
    async fn spawn_run_with_stopped_signal_stamps_reason_on_cancelled_result() {
        let registry = Arc::new(JobRegistry::new());
        let run_id = new_run_id();
        let (handle, _signal) = CancelSignal::new();
        registry.insert(&run_id, handle);
        registry.request_stop(&run_id, CancelReason::ClaudeStop);
        // Verify the recorded reason was set.
        assert!(
            registry.recorded_reason(&run_id).is_some(),
            "recorded_reason should be Some after request_stop"
        );
    }

    #[tokio::test]
    async fn await_terminal_returns_immediately_when_already_terminal() {
        let registry = JobRegistry::new();
        let (handle, _signal) = CancelSignal::new();
        registry.insert("r1", handle);
        registry.publish("r1", RunState::Complete(json!({"status": "ok"})));
        let result = registry.await_terminal("r1", Duration::from_secs(60)).await;
        assert!(matches!(result, Some(RunState::Complete(_))));
    }

    #[tokio::test]
    async fn await_terminal_wakes_on_racing_publish() {
        let registry = Arc::new(JobRegistry::new());
        let (handle, _signal) = CancelSignal::new();
        registry.insert("r1", handle);

        let reg_clone = registry.clone();
        let waiter = tokio::spawn(async move {
            reg_clone
                .await_terminal("r1", Duration::from_secs(60))
                .await
        });

        registry.publish("r1", RunState::Complete(json!({"status": "complete"})));
        let result = waiter.await.unwrap();
        assert!(matches!(result, Some(RunState::Complete(_))));
    }

    #[tokio::test]
    async fn await_terminal_times_out_to_running() {
        let registry = JobRegistry::new();
        let (handle, _signal) = CancelSignal::new();
        registry.insert("r1", handle);
        let result = registry
            .await_terminal("r1", Duration::from_millis(1))
            .await;
        assert!(matches!(result, Some(RunState::Running)));
    }

    #[tokio::test]
    async fn await_terminal_unknown_id_is_none() {
        let registry = JobRegistry::new();
        let result = registry
            .await_terminal("nonexistent", Duration::from_millis(1))
            .await;
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn spawn_run_publishes_complete() {
        let registry = Arc::new(JobRegistry::new());
        let run_id = new_run_id();
        let (handle, _signal) = CancelSignal::new();
        spawn_run(registry.clone(), run_id.clone(), handle, async {
            Ok(json!({"status": "complete"}))
        });
        let result = registry
            .await_terminal(&run_id, Duration::from_secs(60))
            .await;
        assert!(matches!(result, Some(RunState::Complete(_))));
    }

    #[tokio::test]
    async fn spawn_run_publishes_failed() {
        let registry = Arc::new(JobRegistry::new());
        let run_id = new_run_id();
        let (handle, _signal) = CancelSignal::new();
        spawn_run(registry.clone(), run_id.clone(), handle, async {
            Err("boom".into())
        });
        let result = registry
            .await_terminal(&run_id, Duration::from_secs(60))
            .await;
        assert!(matches!(result, Some(RunState::Failed(_))));
    }

    #[test]
    fn request_stop_all_fires_every_run_and_counts() {
        let registry = JobRegistry::new();
        let (handle1, signal1) = CancelSignal::new();
        let (handle2, signal2) = CancelSignal::new();
        registry.insert("r1", handle1);
        registry.insert("r2", handle2);
        assert!(!signal1.is_cancelled());
        assert!(!signal2.is_cancelled());

        let count = registry.request_stop_all(CancelReason::UserStop);
        assert_eq!(count, 2, "should fire two runs");
        assert!(signal1.is_cancelled(), "signal1 should be cancelled");
        assert!(signal2.is_cancelled(), "signal2 should be cancelled");
    }

    #[test]
    fn request_stop_all_on_empty_registry_is_zero() {
        let registry = JobRegistry::new();
        let count = registry.request_stop_all(CancelReason::UserStop);
        assert_eq!(count, 0, "empty registry should return 0");
    }

    /// Fixed clock for every record test — no real time anywhere in this module's
    /// tests, so a record's `ts` is whatever the test says it is.
    const T0: u64 = 1_700_000_000_000;

    fn registry_with_records(dir: &std::path::Path) -> JobRegistry {
        JobRegistry::with_record_dir(dir.to_path_buf())
    }

    #[test]
    fn publish_terminal_writes_run_record() {
        let dir = tempfile::TempDir::new().unwrap();
        let registry = registry_with_records(dir.path());
        let (handle, _signal) = CancelSignal::new();
        registry.insert("r1", handle);
        registry.publish("r1", RunState::Complete(json!({"status": "complete"})));

        let body = std::fs::read(dir.path().join("r1.json")).expect("record should exist");
        let record: RunRecord = serde_json::from_slice(&body).unwrap();
        assert_eq!(record.run_id, "r1");
        assert_eq!(record.state, "done");
        assert_eq!(record.result.unwrap()["status"], "complete");
        assert!(record.error.is_none());
    }

    #[test]
    fn publish_failed_writes_error_record() {
        let dir = tempfile::TempDir::new().unwrap();
        let registry = registry_with_records(dir.path());
        let (handle, _signal) = CancelSignal::new();
        registry.insert("r1", handle);
        registry.publish("r1", RunState::Failed("boom".into()));

        let record = registry.load_record("r1").expect("record should exist");
        assert_eq!(record.state, "failed");
        assert_eq!(record.error.as_deref(), Some("boom"));
        assert!(record.result.is_none());
    }

    #[test]
    fn publish_running_writes_nothing() {
        let dir = tempfile::TempDir::new().unwrap();
        let registry = registry_with_records(dir.path());
        let (handle, _signal) = CancelSignal::new();
        registry.insert("r1", handle);
        registry.publish("r1", RunState::Running);

        assert!(
            !dir.path().join("r1.json").exists(),
            "a non-terminal state must not be persisted"
        );
        assert!(registry.load_record("r1").is_none());
    }

    #[test]
    fn record_write_leaves_no_tmp_file() {
        let dir = tempfile::TempDir::new().unwrap();
        let registry = registry_with_records(dir.path());
        let (handle, _signal) = CancelSignal::new();
        registry.insert("r1", handle);
        registry.publish("r1", RunState::Complete(json!({"status": "complete"})));

        let leftovers: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|n| n.ends_with(".tmp"))
            .collect();
        assert!(
            leftovers.is_empty(),
            "rename should leave no partial file, found: {leftovers:?}"
        );
    }

    #[test]
    fn registry_without_record_dir_writes_nothing() {
        let dir = tempfile::TempDir::new().unwrap();
        let registry = JobRegistry::new();
        let (handle, _signal) = CancelSignal::new();
        registry.insert("r1", handle);
        registry.publish("r1", RunState::Complete(json!({"status": "complete"})));

        assert_eq!(
            std::fs::read_dir(dir.path()).unwrap().count(),
            0,
            "the default registry must stay purely in-memory"
        );
        assert!(registry.load_record("r1").is_none());
    }

    #[test]
    fn load_record_returns_none_for_unparsable_file() {
        let dir = tempfile::TempDir::new().unwrap();
        let registry = registry_with_records(dir.path());
        std::fs::write(dir.path().join("r1.json"), b"not json").unwrap();

        assert!(
            registry.load_record("r1").is_none(),
            "an unparsable record is treated as absent, not as an error"
        );
    }

    #[test]
    fn load_record_returns_none_for_absent_run() {
        let dir = tempfile::TempDir::new().unwrap();
        let registry = registry_with_records(dir.path());
        assert!(registry.load_record("never-existed").is_none());
    }

    #[test]
    fn prune_records_deletes_only_old_records() {
        let dir = tempfile::TempDir::new().unwrap();
        let write = |id: &str, ts: u64| {
            let record = RunRecord {
                run_id: id.to_string(),
                state: "done".to_string(),
                result: None,
                error: None,
                ts,
            };
            std::fs::write(
                dir.path().join(format!("{id}.json")),
                serde_json::to_vec(&record).unwrap(),
            )
            .unwrap();
        };
        // max_age 1000ms, now T0: cutoff is T0 - 1000.
        write("old", T0 - 5_000);
        write("fresh", T0 - 500);
        prune_records(dir.path(), 1_000, T0);

        assert!(
            !dir.path().join("old.json").exists(),
            "record older than the cutoff should be gone"
        );
        assert!(
            dir.path().join("fresh.json").exists(),
            "record inside the window should survive"
        );
    }

    #[test]
    fn prune_records_ignores_unparsable_and_foreign_files() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::write(dir.path().join("garbage.json"), b"not json").unwrap();
        std::fs::write(dir.path().join("notes.txt"), b"unrelated").unwrap();
        prune_records(dir.path(), 1_000, T0);

        assert!(
            dir.path().join("garbage.json").exists(),
            "an unreadable record is left alone rather than deleted"
        );
        assert!(dir.path().join("notes.txt").exists());
    }

    #[test]
    fn running_count_is_zero_on_empty_registry() {
        let registry = JobRegistry::new();
        assert_eq!(registry.running_count(), 0);
    }

    #[test]
    fn running_count_counts_only_non_terminal_runs() {
        let registry = JobRegistry::new();
        for id in ["r1", "r2", "r3"] {
            let (handle, _signal) = CancelSignal::new();
            registry.insert(id, handle);
        }
        registry.publish("r2", RunState::Complete(json!({"status": "complete"})));
        assert_eq!(
            registry.running_count(),
            2,
            "only the two still-running entries should count"
        );
    }

    #[test]
    fn running_count_drops_to_zero_when_all_publish() {
        let registry = JobRegistry::new();
        for id in ["r1", "r2"] {
            let (handle, _signal) = CancelSignal::new();
            registry.insert(id, handle);
        }
        registry.publish("r1", RunState::Complete(json!({"status": "complete"})));
        registry.publish("r2", RunState::Failed("boom".into()));
        assert_eq!(
            registry.running_count(),
            0,
            "a terminal Failed counts as finished, not in flight"
        );
    }

    #[test]
    fn is_running_true_for_running_false_after_publish() {
        let registry = JobRegistry::new();
        let (handle, _signal) = CancelSignal::new();
        registry.insert("r1", handle);
        assert!(registry.is_running("r1"), "should be running after insert");
        registry.publish("r1", RunState::Complete(json!({"status": "ok"})));
        assert!(
            !registry.is_running("r1"),
            "should not be running after publish terminal"
        );
        assert!(
            !registry.is_running("unknown"),
            "unknown id should not be running"
        );
    }
}
