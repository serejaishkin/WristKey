//! Files the sign-in session shares with the (non-elevated) app. The service
//! can be a SYSTEM process whose pipe the app cannot open, so it publishes a
//! status snapshot and an audit trail into the user data directory instead.

use serde_json::{json, Value};
use std::io::Write;
use std::path::PathBuf;

const STATUS_FILE: &str = "session_state.json";
const AUDIT_FILE: &str = "session_audit.jsonl";
const AUDIT_MAX_BYTES: u64 = 128 * 1024;
const AUDIT_KEEP_LINES: usize = 200;
/// Status older than this means the service is not running.
pub const STATUS_STALE_MS: i64 = 6_000;

fn now_ms() -> i64 { chrono::Utc::now().timestamp_millis() }

#[derive(Clone, Debug)]
pub struct SessionFiles { dir: PathBuf }

impl SessionFiles {
    pub fn new(dir: PathBuf) -> Self { Self { dir } }

    /// Same directory resolution as `SqliteStorage::open_default`.
    pub fn default_dir() -> Option<Self> {
        if let Ok(d) = std::env::var("WRISTKEY_DATA_DIR") {
            if !d.is_empty() { return Some(Self::new(PathBuf::from(d))); }
        }
        directories::ProjectDirs::from("", "", "WristKey").map(|p| Self::new(p.data_dir().to_path_buf()))
    }

    fn status_path(&self) -> PathBuf { self.dir.join(STATUS_FILE) }
    fn audit_path(&self) -> PathBuf { self.dir.join(AUDIT_FILE) }

    /// Atomic replace; stamps `updated_ms`. Errors are swallowed on purpose (diagnostics only).
    pub fn write_status(&self, status: &Value) {
        let mut v = status.clone();
        if let Some(o) = v.as_object_mut() { o.insert("updated_ms".into(), json!(now_ms())); }
        let _ = std::fs::create_dir_all(&self.dir);
        let tmp = self.dir.join(format!("{}.tmp", STATUS_FILE));
        if std::fs::write(&tmp, v.to_string()).is_ok() {
            let _ = std::fs::rename(&tmp, self.status_path());
        }
    }

    /// Latest status plus `stale` (true when the service stopped refreshing it).
    pub fn read_status(&self) -> Option<Value> {
        let text = std::fs::read_to_string(self.status_path()).ok()?;
        let mut v: Value = serde_json::from_str(&text).ok()?;
        let updated = v.get("updated_ms").and_then(|x| x.as_i64()).unwrap_or(0);
        let stale = now_ms() - updated > STATUS_STALE_MS;
        if let Some(o) = v.as_object_mut() { o.insert("stale".into(), json!(stale)); }
        Some(v)
    }

    /// Never stores secrets: callers pass a short event name and detail only.
    pub fn audit(&self, event: &str, detail: &str) {
        let _ = std::fs::create_dir_all(&self.dir);
        let path = self.audit_path();
        if std::fs::metadata(&path).map(|m| m.len() > AUDIT_MAX_BYTES).unwrap_or(false) {
            self.trim(&path);
        }
        let line = json!({"ts_ms": now_ms(), "event": event, "detail": detail}).to_string();
        if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&path) {
            let _ = writeln!(f, "{}", line);
        }
    }

    fn trim(&self, path: &PathBuf) {
        if let Ok(text) = std::fs::read_to_string(path) {
            let lines: Vec<&str> = text.lines().collect();
            let keep = &lines[lines.len().saturating_sub(AUDIT_KEEP_LINES)..];
            let _ = std::fs::write(path, keep.join("\n") + "\n");
        }
    }

    /// Newest first.
    pub fn recent(&self, n: usize) -> Vec<Value> {
        let text = std::fs::read_to_string(self.audit_path()).unwrap_or_default();
        text.lines().rev().filter_map(|l| serde_json::from_str(l).ok()).take(n).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp() -> SessionFiles {
        let d = std::env::temp_dir().join(format!("wk-audit-{}", uuid::Uuid::new_v4()));
        SessionFiles::new(d)
    }

    #[test]
    fn status_round_trip_and_staleness() {
        let f = tmp();
        assert!(f.read_status().is_none());
        f.write_status(&json!({"state":"locked"}));
        let s = f.read_status().unwrap();
        assert_eq!(s["state"], "locked");
        assert_eq!(s["stale"], false);
        // age it
        let old = json!({"state":"locked","updated_ms": now_ms() - STATUS_STALE_MS - 1});
        std::fs::write(f.status_path(), old.to_string()).unwrap();
        assert_eq!(f.read_status().unwrap()["stale"], true);
    }

    #[test]
    fn audit_appends_newest_first() {
        let f = tmp();
        f.audit("ui_shown", "a");
        f.audit("consume", "ok");
        let r = f.recent(10);
        assert_eq!(r.len(), 2);
        assert_eq!(r[0]["event"], "consume");
        assert_eq!(f.recent(1).len(), 1);
    }

    #[test]
    fn audit_is_trimmed_when_large() {
        let f = tmp();
        let big = "x".repeat(1000);
        for i in 0..200 { f.audit("e", &format!("{}{}", i, big)); }
        assert!(std::fs::metadata(f.audit_path()).unwrap().len() > AUDIT_MAX_BYTES);
        f.audit("last", "");
        let r = f.recent(1000);
        assert!(r.len() <= AUDIT_KEEP_LINES + 1);
        assert_eq!(r[0]["event"], "last");
    }
}
