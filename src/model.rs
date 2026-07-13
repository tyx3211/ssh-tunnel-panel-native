use std::sync::Arc;

use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ForwardId(pub String);

impl ForwardId {
    #[must_use]
    pub fn new() -> Self {
        Self(uuid::Uuid::new_v4().to_string())
    }
}

impl Default for ForwardId {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ForwardMode {
    L,
    R,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ForwardStatus {
    Stopped,
    Starting,
    Running,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum HostSource {
    SshConfig,
    Manual,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SshHost {
    pub id: String,
    pub alias: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub host_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub port: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub identity_file: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub proxy_jump: Option<String>,
    pub source: HostSource,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ForwardConfig {
    pub id: ForwardId,
    pub name: String,
    pub host_alias: String,
    pub mode: ForwardMode,
    pub bind_address: String,
    pub listen_port: u16,
    pub target_host: String,
    pub target_port: u16,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ForwardDraft {
    pub id: Option<ForwardId>,
    pub name: String,
    pub host_alias: String,
    pub mode: ForwardMode,
    pub bind_address: String,
    pub listen_port: u16,
    pub target_host: String,
    pub target_port: u16,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct PersistedState {
    #[serde(default)]
    pub manual_hosts: Vec<SshHost>,
    #[serde(default)]
    pub forwards: Vec<ForwardConfig>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ForwardRuntime {
    pub status: ForwardStatus,
    pub pid: Option<u32>,
    pub error: Option<String>,
    pub actual_listen_port: Option<u16>,
    pub last_started_at: Option<String>,
    pub logs: Arc<Vec<Arc<str>>>,
}

impl Default for ForwardRuntime {
    fn default() -> Self {
        Self {
            status: ForwardStatus::Stopped,
            pid: None,
            error: None,
            actual_listen_port: None,
            last_started_at: None,
            logs: Arc::default(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ForwardView {
    pub config: ForwardConfig,
    pub runtime: ForwardRuntime,
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum ForwardValidationError {
    #[error("名称不能为空")]
    EmptyName,
    #[error("SSH 主机不能为空")]
    EmptyHostAlias,
    #[error("SSH 主机不能以 - 开头或包含空白字符")]
    InvalidHostAlias,
    #[error("转发地址不能包含空白字符")]
    InvalidForwardHost,
    #[error("端口必须在 1 到 65535 之间")]
    InvalidPort,
}

impl ForwardDraft {
    /// Trims and validates a draft while applying safe loopback defaults.
    ///
    /// # Errors
    ///
    /// Returns an error when required fields, hosts, or ports are invalid.
    pub fn normalize(mut self) -> Result<Self, ForwardValidationError> {
        self.name = self.name.trim().to_owned();
        self.host_alias = self.host_alias.trim().to_owned();
        self.bind_address = normalize_host(&self.bind_address, "127.0.0.1")?;
        self.target_host = normalize_host(&self.target_host, "127.0.0.1")?;

        if self.name.is_empty() {
            return Err(ForwardValidationError::EmptyName);
        }
        if self.host_alias.is_empty() {
            return Err(ForwardValidationError::EmptyHostAlias);
        }
        if self.host_alias.starts_with('-') || self.host_alias.chars().any(char::is_whitespace) {
            return Err(ForwardValidationError::InvalidHostAlias);
        }
        if self.listen_port == 0 || self.target_port == 0 {
            return Err(ForwardValidationError::InvalidPort);
        }

        Ok(self)
    }
}

impl ForwardConfig {
    #[must_use]
    pub fn has_valid_ports(&self) -> bool {
        self.listen_port != 0 && self.target_port != 0
    }
}

impl SshHost {
    #[must_use]
    pub fn has_valid_port(&self) -> bool {
        self.port.is_none_or(|port| port != 0)
    }
}

fn normalize_host(value: &str, fallback: &str) -> Result<String, ForwardValidationError> {
    let trimmed = value.trim();
    if trimmed.chars().any(char::is_whitespace) {
        return Err(ForwardValidationError::InvalidForwardHost);
    }
    Ok(if trimmed.is_empty() {
        fallback.to_owned()
    } else {
        trimmed.to_owned()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn draft() -> ForwardDraft {
        ForwardDraft {
            id: None,
            name: " Web ".into(),
            host_alias: " dev ".into(),
            mode: ForwardMode::L,
            bind_address: String::new(),
            listen_port: 3000,
            target_host: " localhost ".into(),
            target_port: 8080,
        }
    }

    #[test]
    fn normalizes_forward_draft() {
        let normalized = draft().normalize().expect("valid draft must normalize");

        assert_eq!(normalized.name, "Web");
        assert_eq!(normalized.host_alias, "dev");
        assert_eq!(normalized.bind_address, "127.0.0.1");
        assert_eq!(normalized.target_host, "localhost");
    }

    #[test]
    fn rejects_argument_like_host_alias() {
        let mut invalid = draft();
        invalid.host_alias = "-oProxyCommand=bad".into();

        assert_eq!(
            invalid.normalize(),
            Err(ForwardValidationError::InvalidHostAlias)
        );
    }
}
