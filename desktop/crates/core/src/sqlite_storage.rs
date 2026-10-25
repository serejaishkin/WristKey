use async_trait::async_trait;
use rusqlite::{Connection, OptionalExtension, params};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::info;
use uuid::Uuid;
use crate::{Storage, PairedDevice, Config, TouchPoint, WristKeyError, Result};

pub struct SqliteStorage {
    conn: Arc<Mutex<Connection>>,
}

impl SqliteStorage {
    pub fn new(path: impl Into<PathBuf>) -> Result<Self> {
        let path = path.into();
        let conn = Connection::open(&path).map_err(|e| {
            WristKeyError::Storage(format!("failed to open sqlite db at {:?}: {}", path, e))
        })?;
        conn.execute_batch(
            "PRAGMA journal_mode = WAL; PRAGMA busy_timeout = 5000; PRAGMA foreign_keys = ON;
            CREATE TABLE IF NOT EXISTS devices (
                id TEXT PRIMARY KEY, name TEXT NOT NULL, public_key BLOB NOT NULL,
                device_id BLOB, paired_at TEXT NOT NULL, baseline_rssi INTEGER NOT NULL,
                address TEXT NOT NULL, windows_password BLOB
            );
            CREATE TABLE IF NOT EXISTS config (key TEXT PRIMARY KEY, value BLOB NOT NULL);
            CREATE TABLE IF NOT EXISTS touch_points (
                device_id TEXT PRIMARY KEY, x REAL NOT NULL, y REAL NOT NULL
            );"
        ).map_err(|e| WristKeyError::Storage(format!("sqlite init ddl: {}", e)))?;
        // Added after the initial release: CREATE TABLE IF NOT EXISTS above
        // leaves an existing database's `devices` table untouched, so an
        // upgrade needs its own migration. SQLite has no "ADD COLUMN IF NOT
        // EXISTS", so the duplicate-column error on a DB that already has it
        // is the expected, ignored case; any other failure is real.
        if let Err(e) = conn.execute("ALTER TABLE devices ADD COLUMN windows_username TEXT", []) {
            let msg = e.to_string();
            if !msg.contains("duplicate column name") {
                return Err(WristKeyError::Storage(format!("sqlite migrate windows_username: {}", e)));
            }
        }
        if let Err(e) = conn.execute("ALTER TABLE devices ADD COLUMN pairing_pin TEXT", []) {
            let msg = e.to_string();
            if !msg.contains("duplicate column name") {
                return Err(WristKeyError::Storage(format!("sqlite migrate pairing_pin: {}", e)));
            }
        }
        conn.execute("INSERT OR IGNORE INTO config (key, value) VALUES ('main', ?1)",
            params![&serde_json::to_vec(&Config::default()).unwrap()],
        ).map_err(|e| WristKeyError::Storage(format!("sqlite init insert: {}", e)))?;
        Ok(Self { conn: Arc::new(Mutex::new(conn)) })
    }
    pub fn open_default() -> Result<Self> {
        if let Ok(dir) = std::env::var("WRISTKEY_DATA_DIR") {
            if !dir.is_empty() {
                let path = PathBuf::from(dir);
                std::fs::create_dir_all(&path).map_err(|e| WristKeyError::Storage(format!("failed to create data dir: {}", e)))?;
                return Self::new(path.join("wristkey.sqlite"));
            }
        }
        let dirs = directories::ProjectDirs::from("", "", "WristKey")
            .ok_or_else(|| WristKeyError::Storage("cannot determine data directory".into()))?;
        let path = dirs.data_dir();
        std::fs::create_dir_all(path).map_err(|e| WristKeyError::Storage(format!("failed to create data dir: {}", e)))?;
        Self::new(path.join("wristkey.sqlite"))
    }
    fn row_to_device(row: &rusqlite::Row) -> rusqlite::Result<PairedDevice> {
        let id_str: String = row.get(0)?;
        let id = Uuid::parse_str(&id_str).map_err(|e| rusqlite::Error::FromSqlConversionFailure(
            0, rusqlite::types::Type::Text, Box::new(e),
        ))?;
        Ok(PairedDevice {
            id, name: row.get(1)?, public_key: row.get(2)?, device_id: row.get(3)?,
            paired_at: row.get(4)?, baseline_rssi: row.get(5)?, address: row.get(6)?,
            windows_password: row.get(7)?,
            windows_username: row.get(8)?,
            pairing_pin: row.get(9)?,
        })
    }
}

#[async_trait]
impl Storage for SqliteStorage {
    async fn save_device(&self, device: &PairedDevice) -> Result<()> {
        let conn = self.conn.clone(); let device = device.clone();
        tokio::task::spawn_blocking(move || {
            let binding = conn.blocking_lock();
            let mut stmt = binding.prepare_cached(
                "INSERT OR REPLACE INTO devices (id, name, public_key, device_id, paired_at, baseline_rssi, address, windows_password, windows_username, pairing_pin) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)"
            ).map_err(|e| WristKeyError::Storage(format!("sqlite prepare: {}", e)))?;
            stmt.execute(params![device.id.to_string(), device.name, device.public_key, device.device_id, device.paired_at.to_rfc3339(), device.baseline_rssi, device.address, device.windows_password, device.windows_username, device.pairing_pin])
                .map_err(|e| WristKeyError::Storage(format!("sqlite insert device: {}", e)))?;
            info!("persisted device {} to sqlite", device.id); Ok(())
        }).await.map_err(|e| WristKeyError::Storage(format!("spawn_blocking: {}", e)))?
    }
    async fn load_device(&self, id: Uuid) -> Result<Option<PairedDevice>> {
        let conn = self.conn.clone();
        tokio::task::spawn_blocking(move || {
            let binding = conn.blocking_lock();
            let mut stmt = binding.prepare_cached(
                "SELECT id, name, public_key, device_id, paired_at, baseline_rssi, address, windows_password, windows_username, pairing_pin FROM devices WHERE id = ?1"
            ).map_err(|e| WristKeyError::Storage(format!("sqlite prepare: {}", e)))?;
            let device = stmt.query_row(params![id.to_string()], Self::row_to_device).optional()
                .map_err(|e| WristKeyError::Storage(format!("sqlite query: {}", e)))?;
            Ok(device)
        }).await.map_err(|e| WristKeyError::Storage(format!("spawn_blocking: {}", e)))?
    }
    async fn list_devices(&self) -> Result<Vec<PairedDevice>> {
        let conn = self.conn.clone();
        tokio::task::spawn_blocking(move || {
            let binding = conn.blocking_lock();
            let mut stmt = binding.prepare_cached(
                "SELECT id, name, public_key, device_id, paired_at, baseline_rssi, address, windows_password, windows_username, pairing_pin FROM devices"
            ).map_err(|e| WristKeyError::Storage(format!("sqlite prepare: {}", e)))?;
            let mut rows = stmt.query([]).map_err(|e| WristKeyError::Storage(format!("sqlite query: {}", e)))?;
            let mut devices = Vec::new();
            while let Some(row) = rows.next().map_err(|e| WristKeyError::Storage(format!("sqlite row: {}", e)))? {
                devices.push(Self::row_to_device(row).map_err(|e| WristKeyError::Storage(format!("sqlite row map: {}", e)))?);
            } Ok(devices)
        }).await.map_err(|e| WristKeyError::Storage(format!("spawn_blocking: {}", e)))?
    }
    async fn delete_device(&self, id: Uuid) -> Result<()> {
        let conn = self.conn.clone();
        tokio::task::spawn_blocking(move || {
            let binding = conn.blocking_lock();
            let mut stmt = binding.prepare_cached("DELETE FROM devices WHERE id = ?1")
                .map_err(|e| WristKeyError::Storage(format!("sqlite prepare: {}", e)))?;
            stmt.execute(params![id.to_string()]).map_err(|e| WristKeyError::Storage(format!("sqlite delete: {}", e)))?;
            info!("deleted device {} from sqlite", id); Ok(())
        }).await.map_err(|e| WristKeyError::Storage(format!("spawn_blocking: {}", e)))?
    }
    async fn load_config(&self) -> Result<Config> {
        let conn = self.conn.clone();
        tokio::task::spawn_blocking(move || {
            let binding = conn.blocking_lock();
            let mut stmt = binding.prepare_cached("SELECT value FROM config WHERE key = 'main'")
                .map_err(|e| WristKeyError::Storage(format!("sqlite prepare: {}", e)))?;
            let value: Vec<u8> = stmt.query_row([], |row| row.get(0)).optional()
                .map_err(|e| WristKeyError::Storage(format!("sqlite query: {}", e)))?
                .unwrap_or_else(|| serde_json::to_vec(&Config::default()).unwrap());
            let config: Config = serde_json::from_slice(&value).map_err(|e| WristKeyError::Storage(format!("deserialize config: {}", e)))?;
            Ok(config)
        }).await.map_err(|e| WristKeyError::Storage(format!("spawn_blocking: {}", e)))?
    }
    async fn save_config(&self, config: &Config) -> Result<()> {
        let conn = self.conn.clone();
        let value = serde_json::to_vec(config).map_err(|e| WristKeyError::Storage(format!("serialize config: {}", e)))?;
        tokio::task::spawn_blocking(move || {
            let binding = conn.blocking_lock();
            let mut stmt = binding.prepare_cached("INSERT OR REPLACE INTO config (key, value) VALUES ('main', ?1)")
                .map_err(|e| WristKeyError::Storage(format!("sqlite prepare: {}", e)))?;
            stmt.execute(params![value]).map_err(|e| WristKeyError::Storage(format!("sqlite insert config: {}", e)))?;
            Ok(())
        }).await.map_err(|e| WristKeyError::Storage(format!("spawn_blocking: {}", e)))?
    }
    async fn save_touch_point(&self, device_id: &str, point: &TouchPoint) -> Result<()> {
        let conn = self.conn.clone();
        let device_id = device_id.to_string();
        let point = point.clone();
        tokio::task::spawn_blocking(move || {
            let binding = conn.blocking_lock();
            let mut stmt = binding.prepare_cached("INSERT OR REPLACE INTO touch_points (device_id, x, y) VALUES (?1, ?2, ?3)")
                .map_err(|e| WristKeyError::Storage(format!("sqlite prepare: {}", e)))?;
            stmt.execute(params![device_id, point.x, point.y])
                .map_err(|e| WristKeyError::Storage(format!("sqlite insert touch: {}", e)))?;
            info!("persisted PC touch point for {} to sqlite", device_id); Ok(())
        }).await.map_err(|e| WristKeyError::Storage(format!("spawn_blocking: {}", e)))?
    }
    async fn load_touch_point(&self, device_id: &str) -> Result<Option<TouchPoint>> {
        let conn = self.conn.clone();
        let device_id = device_id.to_string();
        tokio::task::spawn_blocking(move || {
            let binding = conn.blocking_lock();
            let mut stmt = binding.prepare_cached("SELECT x, y FROM touch_points WHERE device_id = ?1")
                .map_err(|e| WristKeyError::Storage(format!("sqlite prepare: {}", e)))?;
            let point = stmt.query_row(params![device_id], |row| {
                Ok(TouchPoint { x: row.get(0)?, y: row.get(1)? })
            }).optional().map_err(|e| WristKeyError::Storage(format!("sqlite query: {}", e)))?;
            Ok(point)
        }).await.map_err(|e| WristKeyError::Storage(format!("spawn_blocking: {}", e)))?
    }
    async fn delete_touch_point(&self, device_id: &str) -> Result<()> {
        let conn = self.conn.clone();
        let device_id = device_id.to_string();
        tokio::task::spawn_blocking(move || {
            let binding = conn.blocking_lock();
            let mut stmt = binding.prepare_cached("DELETE FROM touch_points WHERE device_id = ?1")
                .map_err(|e| WristKeyError::Storage(format!("sqlite prepare: {}", e)))?;
            stmt.execute(params![device_id])
                .map_err(|e| WristKeyError::Storage(format!("sqlite delete touch: {}", e)))?;
            info!("deleted PC touch point for {} from sqlite", device_id); Ok(())
        }).await.map_err(|e| WristKeyError::Storage(format!("spawn_blocking: {}", e)))?
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    fn device(id: Uuid, username: Option<&str>) -> PairedDevice {
        PairedDevice {
            id, name: "Watch".into(), public_key: vec![1, 2, 3], device_id: None,
            paired_at: Utc::now(), baseline_rssi: -50, address: "AA:BB".into(),
            windows_password: Some(vec![9, 9]), windows_username: username.map(|s| s.to_string()),
        }
    }

    #[tokio::test]
    async fn windows_username_round_trips_through_save_and_load() {
        let dir = std::env::temp_dir().join(format!("wristkey-test-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let storage = SqliteStorage::new(dir.join("test.sqlite")).unwrap();

        let id = Uuid::new_v4();
        storage.save_device(&device(id, Some("DESKTOP-ABC\\sergej"))).await.unwrap();
        let loaded = storage.load_device(id).await.unwrap().unwrap();
        assert_eq!(loaded.windows_username.as_deref(), Some("DESKTOP-ABC\\sergej"));
        assert_eq!(loaded.windows_password, Some(vec![9, 9]));

        // None must round-trip as NULL, not an empty string.
        storage.save_device(&device(id, None)).await.unwrap();
        let loaded = storage.load_device(id).await.unwrap().unwrap();
        assert_eq!(loaded.windows_username, None);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn opening_a_pre_migration_database_adds_the_column_without_losing_data() {
        let dir = std::env::temp_dir().join(format!("wristkey-test-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let db_path = dir.join("legacy.sqlite");

        // Simulate a database created before windows_username existed.
        {
            let conn = Connection::open(&db_path).unwrap();
            conn.execute_batch(
                "CREATE TABLE devices (
                    id TEXT PRIMARY KEY, name TEXT NOT NULL, public_key BLOB NOT NULL,
                    device_id BLOB, paired_at TEXT NOT NULL, baseline_rssi INTEGER NOT NULL,
                    address TEXT NOT NULL, windows_password BLOB
                );"
            ).unwrap();
            conn.execute(
                "INSERT INTO devices (id, name, public_key, device_id, paired_at, baseline_rssi, address, windows_password) VALUES (?1, 'Old', X'0102', NULL, ?2, -50, 'AA:BB', NULL)",
                params![Uuid::new_v4().to_string(), Utc::now().to_rfc3339()],
            ).unwrap();
        }

        // Opening through SqliteStorage must migrate in place without error
        // and without dropping the pre-existing row.
        let storage = SqliteStorage::new(&db_path).unwrap();
        let devices = storage.list_devices().await.unwrap();
        assert_eq!(devices.len(), 1);
        assert_eq!(devices[0].name, "Old");
        assert_eq!(devices[0].windows_username, None);

        let _ = std::fs::remove_dir_all(&dir);
    }
}
