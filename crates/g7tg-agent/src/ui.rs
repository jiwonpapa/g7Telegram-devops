//! Telegram 화면에서 공통으로 사용하는 상태 아이콘입니다.

use g7tg_core::ServiceStatus;

/// 정상 상태입니다.
pub(crate) const HEALTHY: &str = "🟢";
/// 확인이 필요한 주의 상태입니다.
pub(crate) const WARNING: &str = "🟡";
/// 즉시 확인할 장애 상태입니다.
pub(crate) const CRITICAL: &str = "🔴";
/// 미설정 또는 미감지 상태입니다.
pub(crate) const INACTIVE: &str = "⚪";

/// 임계값 초과 여부에 맞는 자원 상태 아이콘입니다.
#[must_use]
pub(crate) const fn resource_status(warning: bool) -> &'static str {
    if warning { WARNING } else { HEALTHY }
}

/// 디스크 사용률에 맞는 상태 아이콘입니다.
#[must_use]
pub(crate) fn disk_status(usage_percent: f64, warning_percent: f64) -> &'static str {
    if usage_percent >= 95.0 {
        CRITICAL
    } else {
        resource_status(usage_percent >= warning_percent)
    }
}

/// systemd 서비스 상태에 맞는 상태 아이콘입니다.
#[must_use]
pub(crate) fn service_status(service: &ServiceStatus) -> &'static str {
    if service.is_healthy() {
        HEALTHY
    } else if service.active_state == "failed" {
        CRITICAL
    } else if service.load_state == "not-found" {
        INACTIVE
    } else {
        WARNING
    }
}

/// 장애 등급에 맞는 상태 아이콘입니다.
#[must_use]
pub(crate) fn severity_status(severity: &str) -> &'static str {
    match severity {
        "critical" => CRITICAL,
        "warning" => WARNING,
        _ => INACTIVE,
    }
}

/// 내부 장애 등급을 사용자용 한국어로 변환합니다.
#[must_use]
pub(crate) fn severity_label(severity: &str) -> &'static str {
    match severity {
        "critical" => "장애",
        "warning" => "주의",
        _ => "알림",
    }
}

#[cfg(test)]
mod tests {
    use g7tg_core::{ServiceCategory, ServiceStatus};

    use super::{CRITICAL, HEALTHY, INACTIVE, WARNING, disk_status, service_status};

    fn service(load: &str, active: &str) -> ServiceStatus {
        ServiceStatus {
            unit: "demo.service".to_owned(),
            description: "Demo".to_owned(),
            category: ServiceCategory::Application,
            load_state: load.to_owned(),
            active_state: active.to_owned(),
            sub_state: "running".to_owned(),
        }
    }

    #[test]
    fn service_icons_distinguish_operational_states() {
        assert_eq!(service_status(&service("loaded", "active")), HEALTHY);
        assert_eq!(service_status(&service("loaded", "failed")), CRITICAL);
        assert_eq!(service_status(&service("not-found", "inactive")), INACTIVE);
        assert_eq!(service_status(&service("loaded", "inactive")), WARNING);
    }

    #[test]
    fn disk_icons_distinguish_warning_and_critical_usage() {
        assert_eq!(disk_status(30.0, 85.0), HEALTHY);
        assert_eq!(disk_status(85.0, 85.0), WARNING);
        assert_eq!(disk_status(95.0, 85.0), CRITICAL);
    }
}
