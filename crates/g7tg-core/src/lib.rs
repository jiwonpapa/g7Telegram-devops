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

#[cfg(test)]
mod tests {
    use super::Menu;

    #[test]
    fn menu_callbacks_are_fail_closed() {
        assert_eq!(Menu::from_callback("menu:system"), Some(Menu::System));
        assert_eq!(Menu::from_callback("menu:root"), None);
        assert_eq!(Menu::from_callback("action:restart"), None);
    }
}
