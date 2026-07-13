use std::collections::BTreeMap;
use std::path::Path;

use ssh2_config::{ParseRule, SshConfig};
use thiserror::Error;

use crate::model::{HostSource, SshHost};

#[derive(Debug, Error)]
pub enum HostDiscoveryError {
    #[error("解析 SSH config 失败")]
    Parse(#[source] ssh2_config::SshParserError),
}

/// Discovers concrete aliases from the default OpenSSH configuration.
///
/// # Errors
///
/// Returns an error when an existing OpenSSH configuration cannot be parsed.
pub fn read_ssh_config_hosts() -> Result<Vec<SshHost>, HostDiscoveryError> {
    let Some(home) = dirs::home_dir() else {
        return Ok(Vec::new());
    };
    let path = home.join(".ssh").join("config");
    if !path.exists() {
        return Ok(Vec::new());
    }

    let rules = ParseRule::ALLOW_UNKNOWN_FIELDS | ParseRule::ALLOW_UNSUPPORTED_FIELDS;
    let config = SshConfig::parse_default_file(rules).map_err(HostDiscoveryError::Parse)?;
    Ok(hosts_from_config(&config))
}

fn hosts_from_config(config: &SshConfig) -> Vec<SshHost> {
    let mut hosts = BTreeMap::new();

    for host in config.get_hosts() {
        for clause in &host.pattern {
            let alias = clause.pattern.trim();
            if clause.negated || !is_concrete_alias(alias) {
                continue;
            }

            let params = config.query(alias);
            let identity_file = params
                .identity_file
                .as_ref()
                .and_then(|paths| paths.first())
                .map(|path| path_to_string(path));
            let proxy_jump = params.proxy_jump.map(|jumps| jumps.join(","));
            hosts.entry(alias.to_owned()).or_insert_with(|| SshHost {
                id: format!("ssh-config:{alias}"),
                alias: alias.to_owned(),
                host_name: params.host_name,
                user: params.user,
                port: params.port,
                identity_file,
                proxy_jump,
                source: HostSource::SshConfig,
            });
        }
    }

    let mut hosts: Vec<_> = hosts.into_values().collect();
    hosts.sort_by(|left, right| {
        left.alias
            .to_lowercase()
            .cmp(&right.alias.to_lowercase())
            .then_with(|| left.alias.cmp(&right.alias))
    });
    hosts
}

fn is_concrete_alias(alias: &str) -> bool {
    !alias.is_empty()
        && !alias
            .chars()
            .any(|character| matches!(character, '*' | '?' | '!' | ' ' | '\t'))
}

fn path_to_string(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use super::*;

    #[test]
    fn discovers_concrete_aliases_and_excludes_patterns() {
        let source = br"
            Host dev wildcard-* !blocked
              HostName 10.0.0.5
              User william
              Port 2222
            Host build
              ProxyJump gateway
        ";
        let mut reader = Cursor::new(source);
        let config = SshConfig::default()
            .parse(
                &mut reader,
                ParseRule::ALLOW_UNKNOWN_FIELDS | ParseRule::ALLOW_UNSUPPORTED_FIELDS,
            )
            .expect("test SSH config must parse");

        let hosts = hosts_from_config(&config);

        assert_eq!(hosts.len(), 2);
        assert_eq!(hosts[0].alias, "build");
        assert_eq!(hosts[0].proxy_jump.as_deref(), Some("gateway"));
        assert_eq!(hosts[1].alias, "dev");
        assert_eq!(hosts[1].port, Some(2222));
    }
}
