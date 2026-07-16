//! Agent 설정 계약과 검증입니다.

use std::{fs, path::Path};

use anyhow::{Context, ensure};
use serde::Deserialize;

/// Agent의 정적 설정입니다.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AgentConfig {
    /// Telegram에 표시할 서버 이름입니다.
    pub server_name: String,
    /// Bot token credential 파일입니다.
    pub bot_token_file: String,
    /// SQLite 상태 DB 경로입니다.
    #[serde(default = "default_state_database")]
    pub state_database: String,
    /// long polling timeout입니다.
    #[serde(default = "default_poll_timeout")]
    pub poll_timeout_seconds: u64,
    /// Telegram 실패 뒤 최초 재시도 대기입니다.
    #[serde(default = "default_retry_seconds")]
    pub retry_seconds: u64,
}

impl AgentConfig {
    /// TOML 파일을 읽고 역직렬화합니다.
    pub fn load(path: &Path) -> anyhow::Result<Self> {
        let body = fs::read_to_string(path).context("설정 파일 read 실패")?;
        toml::from_str(&body).context("설정 TOML parse 실패")
    }

    /// 운영 가능한 최소 불변조건을 확인합니다.
    pub fn validate(&self) -> anyhow::Result<()> {
        ensure!(
            !self.server_name.trim().is_empty(),
            "server_name이 비어 있습니다"
        );
        ensure!(
            (10..=50).contains(&self.poll_timeout_seconds),
            "poll_timeout_seconds는 10~50이어야 합니다"
        );
        ensure!(
            Path::new(&self.bot_token_file).is_absolute(),
            "bot_token_file은 절대 경로여야 합니다"
        );
        ensure!(
            Path::new(&self.state_database).is_absolute(),
            "state_database는 절대 경로여야 합니다"
        );
        ensure!(
            (1..=30).contains(&self.retry_seconds),
            "retry_seconds는 1~30이어야 합니다"
        );
        Ok(())
    }
}

fn default_state_database() -> String {
    "/var/lib/g7telegram-devops/state.sqlite3".to_owned()
}

const fn default_poll_timeout() -> u64 {
    40
}

const fn default_retry_seconds() -> u64 {
    2
}

#[cfg(test)]
mod tests {
    use super::AgentConfig;

    #[test]
    fn unknown_fields_are_rejected() {
        let result = toml::from_str::<AgentConfig>(
            r#"
server_name = "demo"
bot_token_file = "/run/credentials/token"
unexpected = true
"#,
        );
        assert!(result.is_err());
    }

    #[test]
    fn relative_secret_path_is_rejected() {
        let config = AgentConfig {
            server_name: "demo".to_owned(),
            bot_token_file: "token".to_owned(),
            state_database: "/tmp/state.sqlite3".to_owned(),
            poll_timeout_seconds: 40,
            retry_seconds: 2,
        };
        assert!(config.validate().is_err());
    }
}
