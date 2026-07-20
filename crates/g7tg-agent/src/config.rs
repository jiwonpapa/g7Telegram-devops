//! Agent 설정 계약과 검증입니다.

use std::{fs, path::Path};

use anyhow::{Context, ensure};
use serde::{Deserialize, Serialize};
use url::Url;

/// Agent의 정적 설정입니다.
#[derive(Debug, Clone, Deserialize, Serialize)]
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
    /// 자동 탐지 외에 표시할 root 관리 unit입니다.
    #[serde(default)]
    pub extra_service_units: Vec<String>,
    /// Telegram 서비스 재시작 기능을 표시할지 결정합니다.
    #[serde(default)]
    pub service_actions_enabled: bool,
    /// root-owned action executor 경로입니다.
    #[serde(default = "default_action_executor")]
    pub action_executor: String,
    /// 재시작 승인 token의 유효시간입니다.
    #[serde(default = "default_approval_ttl")]
    pub approval_ttl_seconds: u64,
    /// Telegram을 통한 서버 전체 재시작을 로컬에서 허용했는지 표시합니다.
    #[serde(default)]
    pub server_reboot_enabled: bool,
    /// 공개 웹 endpoint의 최소 가용성 검사입니다.
    #[serde(default)]
    pub web_checks: Vec<WebCheckConfig>,
    /// 상태·서비스·웹 검사를 반복할 주기입니다.
    #[serde(default = "default_monitor_interval")]
    pub monitor_interval_seconds: u64,
    /// 동일 문제를 장애로 확정할 연속 횟수입니다.
    #[serde(default = "default_confirmation_count")]
    pub incident_confirmation_count: u32,
    /// CPU 사용률 경고 기준입니다.
    #[serde(default = "default_cpu_warning_percent")]
    pub cpu_warning_percent: f64,
    /// 논리 CPU 한 개당 1분 load average 경고 기준입니다.
    #[serde(default = "default_load_warning_per_cpu")]
    pub load_warning_per_cpu: f64,
    /// 메모리 사용률 경고 기준입니다.
    #[serde(default = "default_memory_warning_percent")]
    pub memory_warning_percent: f64,
    /// 메모리 경고가 함께 발생할 때 적용할 swap 사용률 경고 기준입니다.
    #[serde(default = "default_swap_warning_percent")]
    pub swap_warning_percent: f64,
    /// 디스크 사용률 경고 기준입니다.
    #[serde(default = "default_disk_warning_percent")]
    pub disk_warning_percent: f64,
}

/// 공개 웹 endpoint의 최소 검사 설정입니다.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct WebCheckConfig {
    /// Telegram 화면과 incident key에 쓰는 이름입니다.
    pub name: String,
    /// query와 credential이 없는 HTTP(S) URL입니다.
    pub url: String,
    /// 허용할 최소 HTTP status입니다.
    #[serde(default = "default_status_min")]
    pub expected_status_min: u16,
    /// 허용할 최대 HTTP status입니다.
    #[serde(default = "default_status_max")]
    pub expected_status_max: u16,
    /// 요청 timeout입니다.
    #[serde(default = "default_web_timeout")]
    pub timeout_seconds: u64,
    /// HTTPS 인증서 만료 경고 기준입니다.
    #[serde(default = "default_tls_warning_days")]
    pub tls_warning_days: i64,
}

impl AgentConfig {
    /// TOML 파일을 읽고 역직렬화합니다.
    pub fn load(path: &Path) -> anyhow::Result<Self> {
        let body = fs::read_to_string(path).context("설정 파일 read 실패")?;
        toml::from_str(&body).context("설정 TOML parse 실패")
    }

    /// 현재 설정을 pretty TOML로 serialize합니다.
    pub fn to_toml(&self) -> anyhow::Result<String> {
        toml::to_string_pretty(self).context("설정 TOML serialize 실패")
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
        ensure!(
            self.extra_service_units.len() <= 32,
            "extra_service_units는 최대 32개입니다"
        );
        for unit in &self.extra_service_units {
            ensure!(
                crate::services::valid_unit_name(unit),
                "허용되지 않는 systemd unit 이름입니다: {unit}"
            );
        }
        ensure!(
            Path::new(&self.action_executor).is_absolute(),
            "action_executor는 절대 경로여야 합니다"
        );
        ensure!(
            (20..=120).contains(&self.approval_ttl_seconds),
            "approval_ttl_seconds는 20~120이어야 합니다"
        );
        ensure!(self.web_checks.len() <= 8, "web_checks는 최대 8개입니다");
        let mut names = std::collections::BTreeSet::new();
        for check in &self.web_checks {
            check.validate()?;
            ensure!(
                names.insert(check.name.as_str()),
                "web check 이름이 중복됩니다"
            );
        }
        ensure!(
            (30..=300).contains(&self.monitor_interval_seconds),
            "monitor_interval_seconds는 30~300이어야 합니다"
        );
        ensure!(
            (1..=5).contains(&self.incident_confirmation_count),
            "incident_confirmation_count는 1~5여야 합니다"
        );
        ensure!(
            (50.0..=99.0).contains(&self.cpu_warning_percent),
            "cpu_warning_percent는 50~99여야 합니다"
        );
        ensure!(
            (0.5..=10.0).contains(&self.load_warning_per_cpu),
            "load_warning_per_cpu는 0.5~10이어야 합니다"
        );
        ensure!(
            (50.0..=99.0).contains(&self.memory_warning_percent),
            "memory_warning_percent는 50~99여야 합니다"
        );
        ensure!(
            (50.0..=99.0).contains(&self.swap_warning_percent),
            "swap_warning_percent는 50~99여야 합니다"
        );
        ensure!(
            (50.0..=99.0).contains(&self.disk_warning_percent),
            "disk_warning_percent는 50~99여야 합니다"
        );
        Ok(())
    }
}

impl WebCheckConfig {
    fn validate(&self) -> anyhow::Result<()> {
        ensure!(
            !self.name.trim().is_empty() && self.name.chars().count() <= 40,
            "web check 이름은 1~40자여야 합니다"
        );
        ensure!(
            self.name.chars().all(
                |character| character.is_alphanumeric() || matches!(character, '-' | '_' | ' ')
            ),
            "web check 이름에 허용되지 않는 문자가 있습니다"
        );
        let url = Url::parse(&self.url).context("web check URL parse 실패")?;
        ensure!(
            matches!(url.scheme(), "http" | "https"),
            "web check는 HTTP(S)만 허용합니다"
        );
        ensure!(url.host_str().is_some(), "web check host가 없습니다");
        ensure!(
            url.username().is_empty() && url.password().is_none(),
            "URL credential을 허용하지 않습니다"
        );
        ensure!(
            url.query().is_none() && url.fragment().is_none(),
            "URL query와 fragment를 허용하지 않습니다"
        );
        ensure!(
            (100..=599).contains(&self.expected_status_min)
                && self.expected_status_min <= self.expected_status_max
                && self.expected_status_max <= 599,
            "HTTP status 범위가 올바르지 않습니다"
        );
        ensure!(
            (1..=15).contains(&self.timeout_seconds),
            "web timeout은 1~15초여야 합니다"
        );
        ensure!(
            (1..=90).contains(&self.tls_warning_days),
            "TLS 경고일은 1~90일이어야 합니다"
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

fn default_action_executor() -> String {
    "/usr/lib/g7telegram-devops/g7tg-exec".to_owned()
}

const fn default_approval_ttl() -> u64 {
    45
}

const fn default_monitor_interval() -> u64 {
    60
}

const fn default_confirmation_count() -> u32 {
    2
}

const fn default_cpu_warning_percent() -> f64 {
    90.0
}

const fn default_load_warning_per_cpu() -> f64 {
    1.5
}

const fn default_memory_warning_percent() -> f64 {
    90.0
}

const fn default_swap_warning_percent() -> f64 {
    80.0
}

const fn default_disk_warning_percent() -> f64 {
    85.0
}

const fn default_status_min() -> u16 {
    200
}

const fn default_status_max() -> u16 {
    399
}

const fn default_web_timeout() -> u64 {
    5
}

const fn default_tls_warning_days() -> i64 {
    14
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
    fn pressure_thresholds_default_for_existing_configs() -> anyhow::Result<()> {
        let config = toml::from_str::<AgentConfig>(
            r#"
server_name = "demo"
bot_token_file = "/run/credentials/token"
"#,
        )?;
        assert_eq!(config.cpu_warning_percent, 90.0);
        assert_eq!(config.load_warning_per_cpu, 1.5);
        assert_eq!(config.memory_warning_percent, 90.0);
        assert_eq!(config.swap_warning_percent, 80.0);
        config.validate()
    }

    #[test]
    fn relative_secret_path_is_rejected() {
        let config = AgentConfig {
            server_name: "demo".to_owned(),
            bot_token_file: "token".to_owned(),
            state_database: "/tmp/state.sqlite3".to_owned(),
            poll_timeout_seconds: 40,
            retry_seconds: 2,
            extra_service_units: Vec::new(),
            service_actions_enabled: false,
            action_executor: "/usr/lib/g7telegram-devops/g7tg-exec".to_owned(),
            approval_ttl_seconds: 45,
            server_reboot_enabled: false,
            web_checks: Vec::new(),
            monitor_interval_seconds: 60,
            incident_confirmation_count: 2,
            cpu_warning_percent: 90.0,
            load_warning_per_cpu: 1.5,
            memory_warning_percent: 90.0,
            swap_warning_percent: 80.0,
            disk_warning_percent: 85.0,
        };
        assert!(config.validate().is_err());
    }

    #[test]
    fn web_check_rejects_query_secrets() {
        let check = super::WebCheckConfig {
            name: "main".to_owned(),
            url: "https://example.com/health?token=secret".to_owned(),
            expected_status_min: 200,
            expected_status_max: 399,
            timeout_seconds: 5,
            tls_warning_days: 14,
        };
        assert!(check.validate().is_err());
    }
}
