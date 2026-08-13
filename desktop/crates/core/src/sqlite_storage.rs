use async_trait::async_trait;
use chrono::{DateTime, Utc};
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::info;
use uuid::Uuid;

use crate::{Storage, PairedDevice, Config, WristKeyError, Result};

/// SQLite-backed storage — supports multi-process access.
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
            "PRAGMA journal_mode = WAL;
             PRAGMA foreign_keys = ON;
             CREATE TABLE IF NOT EXISTS devices (
                 id TEXT PRIMARY KEY,
                 name TEXT NOT NULL,
                 public_key BLOB NOT NULL,
                 device_id BLOB,
                 paired_at TEXT NOT NULL,
                 baseline_rssi INTEGER NOT NULL,
                 address TEXT NOT NULL,
                 windows_password BLOB
             );
             CREATE TABLE IF NOT EXISTS config (
                 key TEXT PRIMARY KEY,
                 value BLOB NOT NULL
             );
             INSERT OR IGNORE INTO config (key, value) VALUES ('main', ?);",
            params![&serde_json::to_vec(&Config::default()).unwrap()],
        ).map_err(|e| WristKeyError::Storage(format!("sqlite init: {}", e)))?;
        Ok(Self { conn: Arc::new(Mutex::new(conn)) })
    }

    pub fn open_default() -> Result<Self> {
        let dirs = directories::ProjectDirs::from("", "", "WristKey")
            .ok_or_else(|| WristKeyError::Storage("cannot determine data directory".into()))?;
        let path = dirs.data_dir();
        std::fs::create_dir_all(path).map_err(|e| {
            WristKeyError::Storage(format!("failed to create data dir: {}", e))
        })?;
        Self::new(path.join("wristkey.sqlite"))
    }

    fn row_to_device(row: &rusqlite::Row) -> rusqlite::Result<PairedDevice> {
        let id_str: String = row.get(0)?;
        let id = Uuid::parse_str(&id_str).map_err(|e| rusqlite::Error::FromSqlConversionFailure(
            0, rusqlite::types::Type::Text, Box::new(e),
        ))?;
        Ok(PairedDevice {
            id,
            name: row.get(1)?,
            public_key: row.get(2)?,
            device_id: row.get(3)?,
            paired_at: row.get(4)?,
            baseline_rssi: row.get(5)?,
            address: row.get(6)?,
            windows_password: row.get(7)?,
        })
    }
}

#[async_trait]
impl Storage for SqliteStorage {
    async fn save_device(&self, device: &PairedDevice) -> Result<()> {
        let conn = self.conn.clone();
        let device = device.clone();
        tokio::task::spawn_blocking(move || {
            let mut stmt = conn.blocking_lock().prepare_cached(
                "INSERT OR REPLACE INTO devices 
                 (id, name, public_key, device_id, paired_at, baseline_rssi, address, windows_password)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)"
            ).map_err(|e| WristKeyError::Storage(format!("sqlite prepare: {}", e)))?;
            stmt.execute(params![
                device.id.to_string(), device.name, device.public_key,
                device.device_id, device.paired_at.to_rfc3339(), device.baseline_rssi,
                device.address, device.windows_password,
            ]).map_err(|e| WristKeyError::Storage(format!("sqlite insert device: {}", e)))?;
            info!("persisted device {} to sqlite", device.id);
            Ok(())
        }).await.map_err(|e| WristKeyError::Storage(format!("spawn_blocking: {}", e)))?
    }

    async fn load_device(&self, id: Uuid) -> Result<Option<PairedDevice>> {
        let conn = self.conn.clone();
        tokio::task::spawn_blocking(move || {
            let mut stmt = conn.blocking_lock().prepare_cached(
                "SELECT id, name, public_key, device_id, paired_at, baseline_rssi, address, windows_password
                 FROM devices WHERE id = ?1"
            ).map_err(|e| WristKeyError::Storage(format!("sqlite prepare: {}", e)))?;
            let device = stmt.query_row(params![id.to_string()], Self::row_to_device)
                .optional()
                .map_err(|e| WristKeyError::Storage(format!("sqlite query: {}", e)))?;
            Ok(device)
        }).await.map_err(|e| WristKeyError::Storage(format!("spawn_blocking: {}", e)))?
    }

    async fn list_devices(&self) -> Result<Vec<PairedDevice>> {
        let conn = self.conn.clone();
        tokio::task::spawn_blocking(move || {
            let conn = conn.blocking_lock();
            let mut stmt = conn.prepare_cached(
                "SELECT id, name, public_key, device_id, paired_at, baseline_rssi, address, windows_password
                 FROM devices"
            ).map_err(|e| WristKeyError::Storage(format!("sqlite prepare: {}", e)))?;
            let devices = stmt.query_map([], Self::row_to_device)
                .map_err(|e| WristKeyError::Storage(format!("sqlite query: {}", e)))?
                .collect::<Result<Vec<_>, _>>()
                .map_err(|e| WristKeyError::Storage(format!("sqlite map: {}", e)))?;
            Ok(devices)
        }).await.map_err(|e| WristKeyError::Storage(format!("spawn_blocking: {}", e)))?
    }

    async fn delete_device(&self, id: Uuid) -> Result<()> {
        let conn = self.conn.clone();
        tokio::task::spawn_blocking(move || {
            let mut stmt = conn.blocking_lock().prepare_cached(
                "DELETE FROM devices WHERE id = ?1"
            ).map_err(|e| WristKeyError::Storage(format!("sqlite prepare: {}", e)))?;
            stmt.execute(params![id.to_string()])
                .map_err(|e| WristKeyError::Storage(format!("sqlite delete: {}", e)))?;
            info!("deleted device {} from sqlite", id);
            Ok(())
        }).await.map_err(|e| WristKeyError::Storage(format!("spawn_blocking: {}", e)))?
    }

    async fn load_config(&self) -> Result<Config> {
        let conn = self.conn.clone();
        tokio::task::spawn_blocking(move || {
            let mut stmt = conn.blocking_lock().prepare_cached(
                "SELECT value FROM config WHERE key = 'main'"
            ).map_err(|e| WristKeyError::Storage(format!("sqlite prepare: {}", e)))?;
            let value: Vec<u8> = stmt.query_row([], |row| row.get(0))
                .optional()
                .map_err(|e| WristKeyError::Storage(format!("sqlite query: {}", e)))?
                .unwrap_or_else(|| serde_json::to_vec(&Config::default()).unwrap());
            let config: Config = serde_json::from_slice(&value)
                .map_err(|e| WristKeyError::Storage(format!("deserialize config: {}", e)))?;
            Ok(config)
        }).await.map_err(|e| WristKeyError::Storage(format!("spawn_blocking: {}", e)))?
    }

    async fn save_config(&self, config: &Config) -> Result<()> {
        let conn = self.conn.clone();
        let value = serde_json::to_vec(config).map_err(|e| {
            WristKeyError::Storage(format!("serialize config: {}", e))
        })?;
        tokio::task::spawn_blocking(move || {
            let mut stmt = conn.blocking_lock().prepare_cached(
                "INSERT OR REPLACE INTO config (key, value) VALUES ('main', ?1)"
            ).map_err(|e| WristKeyError::Storage(format!("sqlite prepare: {}", e)))?;
            stmt.execute(params![value])
                .map_err(|e| WristKeyError::Storage(format!("sqlite insert config: {}", e)))?;
            Ok(())
        }).await.map_err(|e| WristKeyError::Storage(format!("spawn_blocking: {}", e)))?
    }
}
