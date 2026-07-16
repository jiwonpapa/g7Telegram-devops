//! G7Telegram DevOps의 transport와 무관한 공통 계약입니다.

use serde::{Deserialize, Serialize};

/// Telegram에서 이동할 수 있는 메뉴입니다.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Menu {
    /// 최상위 메뉴입니다.
    Main,
    /// 시스템 자원 상태입니다.
    System,
    /// systemd 서비스 목록입니다.
    Services,
    /// 웹 endpoint 상태입니다.
    Web,
    /// 현재 장애와 silence입니다.
    Alerts,
    /// Agent와 서버 정보입니다.
    Info,
}

impl Menu {
    /// callback payload에서 메뉴를 복원합니다.
    #[must_use]
    pub fn from_callback(value: &str) -> Option<Self> {
        match value {
            "menu:main" => Some(Self::Main),
            "menu:system" => Some(Self::System),
            "menu:services" => Some(Self::Services),
            "menu:web" => Some(Self::Web),
            "menu:alerts" => Some(Self::Alerts),
            "menu:info" => Some(Self::Info),
            _ => None,
        }
    }
}

/// 파일시스템 사용량입니다.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct DiskSnapshot {
    /// mount point입니다.
    pub mount_point: String,
    /// 전체 byte입니다.
    pub total_bytes: u64,
    /// 사용 가능한 byte입니다.
    pub available_bytes: u64,
}

/// 현재 서버의 저비용 시스템 snapshot입니다.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SystemSnapshot {
    /// 서버 표시 이름입니다.
    pub server_name: String,
    /// kernel hostname입니다.
    pub hostname: String,
    /// 운영체제 이름입니다.
    pub os_name: String,
    /// kernel 버전입니다.
    pub kernel_version: String,
    /// 부팅 후 경과 초입니다.
    pub uptime_seconds: u64,
    /// 최근 CPU 사용률입니다.
    pub cpu_usage_percent: f32,
    /// load average 정규화에 사용하는 논리 CPU 수입니다.
    pub logical_cpu_count: u32,
    /// 1분 load average입니다.
    pub load_one: f64,
    /// 전체 메모리 byte입니다.
    pub memory_total_bytes: u64,
    /// 사용 중인 메모리 byte입니다.
    pub memory_used_bytes: u64,
    /// 전체 swap byte입니다.
    pub swap_total_bytes: u64,
    /// 사용 중인 swap byte입니다.
    pub swap_used_bytes: u64,
    /// 실제 block filesystem 목록입니다.
    pub disks: Vec<DiskSnapshot>,
}

/// 운영 화면에서 사용하는 서비스 분류입니다.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ServiceCategory {
    /// Nginx, Apache, Caddy입니다.
    Web,
    /// PHP-FPM입니다.
    Php,
    /// MariaDB, MySQL, PostgreSQL입니다.
    Database,
    /// Redis, Memcached입니다.
    Cache,
    /// 명시적으로 발견된 G7/Laravel 장기 실행 서비스입니다.
    Application,
    /// 관리자가 설정에 추가한 서비스입니다.
    Extra,
}

impl ServiceCategory {
    /// 한국어 화면 label입니다.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Web => "웹서버",
            Self::Php => "PHP",
            Self::Database => "데이터베이스",
            Self::Cache => "캐시",
            Self::Application => "G7/앱",
            Self::Extra => "추가 서비스",
        }
    }
}

/// systemd에서 읽은 서비스 상태입니다.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct ServiceStatus {
    /// systemd unit ID입니다.
    pub unit: String,
    /// unit Description입니다.
    pub description: String,
    /// 운영 화면 분류입니다.
    pub category: ServiceCategory,
    /// systemd LoadState입니다.
    pub load_state: String,
    /// systemd ActiveState입니다.
    pub active_state: String,
    /// systemd SubState입니다.
    pub sub_state: String,
}

impl ServiceStatus {
    /// 정상 실행 여부입니다.
    #[must_use]
    pub fn is_healthy(&self) -> bool {
        self.load_state == "loaded" && self.active_state == "active"
    }

    /// 사용자 화면의 짧은 상태입니다.
    #[must_use]
    pub fn state_label(&self) -> &'static str {
        if self.is_healthy() {
            "정상"
        } else if self.active_state == "failed" {
            "장애"
        } else if self.load_state == "not-found" {
            "없음"
        } else {
            "중지"
        }
    }
}

/// Telegram에서 허용하는 서비스 동작입니다.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ServiceAction {
    /// systemd restart입니다.
    Restart,
}

/// 설정한 웹 endpoint의 최소 가용성 결과입니다.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct WebCheckResult {
    /// 화면에 표시할 check 이름입니다.
    pub name: String,
    /// 비밀 query를 허용하지 않는 공개 URL입니다.
    pub url: String,
    /// HTTP status입니다.
    pub status_code: Option<u16>,
    /// header 수신까지의 시간입니다.
    pub latency_ms: Option<u64>,
    /// HTTPS일 때 인증서 만료까지 남은 일수입니다.
    pub tls_days_remaining: Option<i64>,
    /// 기대 status와 TLS 유효성을 모두 만족했는지 표시합니다.
    pub healthy: bool,
    /// 비밀값을 포함하지 않는 안정적인 오류 코드입니다.
    pub error_code: Option<String>,
}

impl ServiceAction {
    /// 저장소와 executor에 사용하는 안정적인 ID입니다.
    #[must_use]
    pub const fn id(self) -> &'static str {
        match self {
            Self::Restart => "restart",
        }
    }

    /// 사용자 화면 label입니다.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Restart => "재시작",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Menu, ServiceCategory, ServiceStatus};

    #[test]
    fn menu_callbacks_are_fail_closed() {
        assert_eq!(Menu::from_callback("menu:system"), Some(Menu::System));
        assert_eq!(Menu::from_callback("menu:root"), None);
        assert_eq!(Menu::from_callback("action:restart"), None);
    }

    #[test]
    fn service_health_requires_loaded_and_active() {
        let mut service = ServiceStatus {
            unit: "nginx.service".to_owned(),
            description: "Nginx".to_owned(),
            category: ServiceCategory::Web,
            load_state: "loaded".to_owned(),
            active_state: "active".to_owned(),
            sub_state: "running".to_owned(),
        };
        assert!(service.is_healthy());
        service.active_state = "activating".to_owned();
        assert!(!service.is_healthy());
    }
}
