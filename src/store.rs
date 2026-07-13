use std::fs;
use std::io::Write as _;
use std::path::{Path, PathBuf};

use atomic_write_file::AtomicWriteFile;
use serde_json::Value;
use thiserror::Error;

use crate::model::{ForwardConfig, PersistedState, SshHost};

const APP_DIRECTORY: &str = "ssh-tunnel-panel";
const LEGACY_APP_DIRECTORY: &str = "zed-tunnel-panel";
const STATE_FILE: &str = "state.json";

#[derive(Debug, Error)]
pub enum StoreError {
    #[error("无法确定 Windows 配置目录")]
    ConfigDirectoryUnavailable,
    #[error("{operation}失败：{path}")]
    Io {
        operation: &'static str,
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("序列化转发配置失败")]
    Serialize(#[source] serde_json::Error),
}

#[derive(Debug, Clone)]
pub struct JsonStore {
    path: PathBuf,
}

impl JsonStore {
    /// Prepares the canonical application store and migrates the legacy location when needed.
    ///
    /// # Errors
    ///
    /// Returns an error when the Windows config directory is unavailable or migration fails.
    pub fn prepare_default() -> Result<Self, StoreError> {
        let config_dir = dirs::config_dir().ok_or(StoreError::ConfigDirectoryUnavailable)?;
        let current = config_dir.join(APP_DIRECTORY).join(STATE_FILE);
        let legacy = config_dir.join(LEGACY_APP_DIRECTORY).join(STATE_FILE);
        Self::prepare(current, legacy)
    }

    /// Prepares explicitly supplied current and legacy paths.
    ///
    /// # Errors
    ///
    /// Returns an error when migration directories or files cannot be created or copied.
    pub fn prepare(path: PathBuf, legacy_path: PathBuf) -> Result<Self, StoreError> {
        if !path.exists() && legacy_path.exists() {
            let parent = path.parent().ok_or_else(|| StoreError::Io {
                operation: "确定配置目录",
                path: path.clone(),
                source: std::io::Error::other("state path has no parent"),
            })?;
            create_dir_all(parent)?;
            fs::copy(&legacy_path, &path).map_err(|source| StoreError::Io {
                operation: "迁移旧配置",
                path: legacy_path,
                source,
            })?;
        }
        Ok(Self { path })
    }

    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Reads state, tolerating malformed entries and backing up malformed JSON.
    ///
    /// # Errors
    ///
    /// Returns an error when an existing state file cannot be read.
    pub fn read(&self) -> Result<PersistedState, StoreError> {
        let raw = match fs::read_to_string(&self.path) {
            Ok(raw) => raw,
            Err(source) if source.kind() == std::io::ErrorKind::NotFound => {
                return Ok(PersistedState::default());
            }
            Err(source) => {
                return Err(StoreError::Io {
                    operation: "读取配置",
                    path: self.path.clone(),
                    source,
                });
            }
        };

        let Ok(value) = serde_json::from_str::<Value>(&raw) else {
            self.backup_corrupt_file();
            return Ok(PersistedState::default());
        };

        Ok(parse_tolerant_state(&value))
    }

    /// Atomically writes the complete persisted state.
    ///
    /// # Errors
    ///
    /// Returns an error when serialization, directory creation, writing, or commit fails.
    pub fn write(&self, state: &PersistedState) -> Result<(), StoreError> {
        let parent = self.path.parent().ok_or_else(|| StoreError::Io {
            operation: "确定配置目录",
            path: self.path.clone(),
            source: std::io::Error::other("state path has no parent"),
        })?;
        create_dir_all(parent)?;

        let bytes = serde_json::to_vec_pretty(state).map_err(StoreError::Serialize)?;
        let mut file = AtomicWriteFile::options()
            .open(&self.path)
            .map_err(|source| StoreError::Io {
                operation: "打开临时配置",
                path: self.path.clone(),
                source,
            })?;
        file.write_all(&bytes).map_err(|source| StoreError::Io {
            operation: "写入配置",
            path: self.path.clone(),
            source,
        })?;
        file.write_all(b"\n").map_err(|source| StoreError::Io {
            operation: "写入配置",
            path: self.path.clone(),
            source,
        })?;
        file.commit().map_err(|source| StoreError::Io {
            operation: "提交配置",
            path: self.path.clone(),
            source,
        })
    }

    fn backup_corrupt_file(&self) {
        let suffix = chrono::Utc::now().format("%Y-%m-%dT%H-%M-%S%.3fZ");
        let backup = PathBuf::from(format!("{}.corrupt-{suffix}", self.path.display()));
        let _ = fs::copy(&self.path, backup);
    }
}

fn create_dir_all(path: &Path) -> Result<(), StoreError> {
    fs::create_dir_all(path).map_err(|source| StoreError::Io {
        operation: "创建配置目录",
        path: path.to_path_buf(),
        source,
    })
}

fn parse_tolerant_state(value: &Value) -> PersistedState {
    let manual_hosts = value
        .get("manualHosts")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|item| serde_json::from_value::<SshHost>(item.clone()).ok())
        .filter(SshHost::has_valid_port)
        .collect();
    let forwards = value
        .get("forwards")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|item| serde_json::from_value::<ForwardConfig>(item.clone()).ok())
        .filter(ForwardConfig::has_valid_ports)
        .collect();

    PersistedState {
        manual_hosts,
        forwards,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{ForwardId, ForwardMode, HostSource};

    fn sample_state() -> PersistedState {
        PersistedState {
            manual_hosts: vec![SshHost {
                id: "manual:test".into(),
                alias: "test".into(),
                host_name: None,
                user: None,
                port: None,
                identity_file: None,
                proxy_jump: None,
                source: HostSource::Manual,
            }],
            forwards: vec![ForwardConfig {
                id: ForwardId("forward-1".into()),
                name: "Web".into(),
                host_alias: "test".into(),
                mode: ForwardMode::L,
                bind_address: "127.0.0.1".into(),
                listen_port: 3000,
                target_host: "127.0.0.1".into(),
                target_port: 3000,
                created_at: "2026-01-01T00:00:00.000Z".into(),
                updated_at: "2026-01-01T00:00:00.000Z".into(),
            }],
        }
    }

    #[test]
    fn round_trips_existing_schema() -> Result<(), StoreError> {
        let directory = tempfile::tempdir().map_err(|source| StoreError::Io {
            operation: "创建测试目录",
            path: PathBuf::from("temp"),
            source,
        })?;
        let path = directory.path().join("state.json");
        let store = JsonStore::prepare(path, directory.path().join("legacy.json"))?;
        let expected = sample_state();

        store.write(&expected)?;

        assert_eq!(store.read()?, expected);
        Ok(())
    }

    #[test]
    fn ignores_only_malformed_entries() {
        let valid = serde_json::to_value(sample_state().forwards.remove(0));
        let value = serde_json::json!({
            "manualHosts": [42],
            "forwards": [valid.expect("sample config must serialize"), {"id": "broken"}]
        });

        let state = parse_tolerant_state(&value);

        assert!(state.manual_hosts.is_empty());
        assert_eq!(state.forwards.len(), 1);
    }

    #[test]
    fn migrates_legacy_state_only_when_current_is_absent() -> Result<(), StoreError> {
        let directory = tempfile::tempdir().map_err(|source| StoreError::Io {
            operation: "创建测试目录",
            path: PathBuf::from("temp"),
            source,
        })?;
        let current = directory.path().join("current").join("state.json");
        let legacy = directory.path().join("legacy").join("state.json");
        fs::create_dir_all(legacy.parent().expect("test path has a parent")).map_err(|source| {
            StoreError::Io {
                operation: "创建测试目录",
                path: legacy.clone(),
                source,
            }
        })?;
        fs::write(&legacy, "{\"manualHosts\":[],\"forwards\":[]}").map_err(|source| {
            StoreError::Io {
                operation: "写入测试配置",
                path: legacy.clone(),
                source,
            }
        })?;

        let store = JsonStore::prepare(current.clone(), legacy)?;

        assert_eq!(store.path(), current);
        assert!(store.path().exists());
        Ok(())
    }
}
