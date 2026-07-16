//! 주기적 상태 대조와 Telegram 장애·복구 알림입니다.

use g7tg_core::{SystemSnapshot, WebCheckResult};

use crate::{
    config::AgentConfig,
    services,
    storage::{ObservedIncident, Store},
    system,
    telegram::TelegramClient,
    web,
};

/// 한 번의 monitoring, incident 대조, outbox 전송을 수행합니다.
pub async fn cycle(
    config: &AgentConfig,
    store: &Store,
    telegram: &TelegramClient,
) -> anyhow::Result<()> {
    let server_name = config.server_name.clone();
    let snapshot = tokio::task::spawn_blocking(move || system::collect(&server_name)).await?;
    let service_statuses = services::discover(&config.extra_service_units).await?;
    let web_results = web::check_all(&config.web_checks).await?;
    let observed = observe(config, &snapshot, &service_statuses, &web_results);
    store.reconcile_incidents(&observed, config.incident_confirmation_count)?;

    let Some(owner) = store.owner()? else {
        return Ok(());
    };
    if store.silence_until()?.is_some() {
        return Ok(());
    }
    for notification in store.pending_notifications(20)? {
        let heading = if notification.kind == "opened" {
            "장애 발생"
        } else {
            "복구"
        };
        let text = format!(
            "[{heading}] {}\n등급: {}\n{}",
            config.server_name, notification.severity, notification.summary
        );
        telegram.send_message(owner.chat_id, &text, None).await?;
        store.mark_notification_sent(notification.id)?;
    }
    Ok(())
}

fn observe(
    config: &AgentConfig,
    snapshot: &SystemSnapshot,
    service_statuses: &[g7tg_core::ServiceStatus],
    web_results: &[WebCheckResult],
) -> Vec<ObservedIncident> {
    let mut incidents = Vec::new();
    let memory_percent = percent(snapshot.memory_used_bytes, snapshot.memory_total_bytes);
    if memory_percent >= config.memory_warning_percent {
        incidents.push(ObservedIncident {
            key: "system:memory".to_owned(),
            severity: "warning".to_owned(),
            summary: format!("메모리 사용률 {memory_percent:.1}%"),
        });
    }
    for disk in &snapshot.disks {
        let used = disk.total_bytes.saturating_sub(disk.available_bytes);
        let usage_percent = percent(used, disk.total_bytes);
        if usage_percent >= config.disk_warning_percent {
            incidents.push(ObservedIncident {
                key: format!("disk:{}", disk.mount_point),
                severity: if usage_percent >= 95.0 {
                    "critical".to_owned()
                } else {
                    "warning".to_owned()
                },
                summary: format!("디스크 {} 사용률 {usage_percent:.1}%", disk.mount_point),
            });
        }
    }
    for service in service_statuses {
        if !service.is_healthy() {
            incidents.push(ObservedIncident {
                key: format!("service:{}", service.unit),
                severity: "critical".to_owned(),
                summary: format!(
                    "서비스 {} 상태 {}/{}",
                    service.unit, service.active_state, service.sub_state
                ),
            });
        }
    }
    for result in web_results {
        if !result.healthy {
            incidents.push(ObservedIncident {
                key: format!("web:{}", result.name),
                severity: "critical".to_owned(),
                summary: format!(
                    "웹 검사 {} 실패 ({})",
                    result.name,
                    result.error_code.as_deref().unwrap_or("unhealthy")
                ),
            });
        }
        let warning_days = config
            .web_checks
            .iter()
            .find(|check| check.name == result.name)
            .map_or(14, |check| check.tls_warning_days);
        if let Some(days) = result.tls_days_remaining
            && days <= warning_days
            && days > 0
        {
            incidents.push(ObservedIncident {
                key: format!("tls:{}", result.name),
                severity: "warning".to_owned(),
                summary: format!("TLS 인증서 {}일 후 만료: {}", days, result.name),
            });
        }
    }
    incidents
}

fn percent(used: u64, total: u64) -> f64 {
    if total == 0 {
        0.0
    } else {
        used as f64 * 100.0 / total as f64
    }
}
