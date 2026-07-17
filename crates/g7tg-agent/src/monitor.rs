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
    match tokio::task::spawn_blocking(move || system::collect(&server_name)).await {
        Ok(snapshot) => {
            store.reconcile_incidents_scoped(
                &observe_system(config, &snapshot),
                config.incident_confirmation_count,
                &["system:", "disk:"],
            )?;
            reconcile_collector(store, config, "system", false)?;
        }
        Err(error) => {
            tracing::warn!(error = %error, "시스템 상태 collector 실패");
            reconcile_collector(store, config, "system", true)?;
        }
    }

    match services::discover(&config.extra_service_units).await {
        Ok(service_statuses) => {
            store.reconcile_incidents_scoped(
                &observe_services(&service_statuses),
                config.incident_confirmation_count,
                &["service:"],
            )?;
            reconcile_collector(store, config, "services", false)?;
        }
        Err(error) => {
            tracing::warn!(error = %error, "서비스 상태 collector 실패");
            reconcile_collector(store, config, "services", true)?;
        }
    }

    match web::check_all(&config.web_checks).await {
        Ok(web_results) => {
            store.reconcile_incidents_scoped(
                &observe_web(config, &web_results),
                config.incident_confirmation_count,
                &["web:", "tls:"],
            )?;
            reconcile_collector(store, config, "web", false)?;
        }
        Err(error) => {
            tracing::warn!(error = %error, "웹 상태 collector 실패");
            reconcile_collector(store, config, "web", true)?;
        }
    }

    if store.silence_until()?.is_some() {
        let discarded = store.discard_pending_notifications()?;
        tracing::debug!(discarded, "알림중지 중 대기 알림을 억제했습니다");
        return Ok(());
    }

    if store.expired_silence_until()?.is_some() {
        store.discard_pending_notifications()?;
        if let Some(owner) = store.owner()? {
            let incidents = store.current_incidents()?;
            let text = render_silence_digest(config, &incidents);
            telegram.send_message(owner.chat_id, &text, None).await?;
        }
        store.clear_silence()?;
        return Ok(());
    }

    let Some(owner) = store.owner()? else {
        return Ok(());
    };
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

fn reconcile_collector(
    store: &Store,
    config: &AgentConfig,
    collector: &str,
    failed: bool,
) -> anyhow::Result<()> {
    let key = format!("agent:collector:{collector}");
    let observed = failed
        .then(|| ObservedIncident {
            key: key.clone(),
            severity: "warning".to_owned(),
            summary: format!("{collector} 상태 수집기가 연속 실패했습니다"),
        })
        .into_iter()
        .collect::<Vec<_>>();
    store.reconcile_incidents_scoped(
        &observed,
        config.incident_confirmation_count,
        &[key.as_str()],
    )
}

fn render_silence_digest(
    config: &AgentConfig,
    incidents: &[crate::storage::CurrentIncident],
) -> String {
    if incidents.is_empty() {
        return format!(
            "[알림중지 종료] {}\n현재 확인된 장애가 없습니다.",
            config.server_name
        );
    }
    let mut lines = vec![format!(
        "[알림중지 종료] {}\n현재 장애 {}건",
        config.server_name,
        incidents.len()
    )];
    lines.extend(
        incidents
            .iter()
            .take(10)
            .map(|incident| format!("- [{}] {}", incident.severity, incident.summary)),
    );
    if incidents.len() > 10 {
        lines.push(format!("- 그 외 {}건", incidents.len() - 10));
    }
    lines.join("\n")
}

#[cfg(test)]
fn observe(
    config: &AgentConfig,
    snapshot: &SystemSnapshot,
    service_statuses: &[g7tg_core::ServiceStatus],
    web_results: &[WebCheckResult],
) -> Vec<ObservedIncident> {
    let mut incidents = observe_system(config, snapshot);
    incidents.extend(observe_services(service_statuses));
    incidents.extend(observe_web(config, web_results));
    incidents
}

fn observe_system(config: &AgentConfig, snapshot: &SystemSnapshot) -> Vec<ObservedIncident> {
    let mut incidents = Vec::new();
    let cpu_percent = f64::from(snapshot.cpu_usage_percent);
    if cpu_percent >= config.cpu_warning_percent {
        incidents.push(ObservedIncident {
            key: "system:cpu".to_owned(),
            severity: "warning".to_owned(),
            summary: format!("CPU 사용률 {cpu_percent:.1}%"),
        });
    }
    let logical_cpu_count = f64::from(snapshot.logical_cpu_count.max(1));
    let load_per_cpu = snapshot.load_one / logical_cpu_count;
    if load_per_cpu >= config.load_warning_per_cpu {
        incidents.push(ObservedIncident {
            key: "system:load".to_owned(),
            severity: "warning".to_owned(),
            summary: format!(
                "1분 Load {:.2} ({load_per_cpu:.2}/CPU, {}CPU)",
                snapshot.load_one, snapshot.logical_cpu_count
            ),
        });
    }
    let memory_percent = percent(snapshot.memory_used_bytes, snapshot.memory_total_bytes);
    if memory_percent >= config.memory_warning_percent {
        incidents.push(ObservedIncident {
            key: "system:memory".to_owned(),
            severity: "warning".to_owned(),
            summary: format!("메모리 사용률 {memory_percent:.1}%"),
        });
    }
    let swap_percent = percent(snapshot.swap_used_bytes, snapshot.swap_total_bytes);
    if snapshot.swap_total_bytes > 0
        && swap_percent >= config.swap_warning_percent
        && memory_percent >= config.memory_warning_percent
    {
        incidents.push(ObservedIncident {
            key: "system:swap_pressure".to_owned(),
            severity: "warning".to_owned(),
            summary: format!(
                "메모리/Swap 압박: 메모리 {memory_percent:.1}%, Swap {swap_percent:.1}%"
            ),
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
    incidents
}

fn observe_services(service_statuses: &[g7tg_core::ServiceStatus]) -> Vec<ObservedIncident> {
    let mut incidents = Vec::new();
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
    incidents
}

fn observe_web(config: &AgentConfig, web_results: &[WebCheckResult]) -> Vec<ObservedIncident> {
    let mut incidents = Vec::new();
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

#[cfg(test)]
mod tests {
    use g7tg_core::SystemSnapshot;

    use super::observe;
    use crate::config::AgentConfig;

    fn config() -> AgentConfig {
        AgentConfig {
            server_name: "demo".to_owned(),
            bot_token_file: "/run/credentials/token".to_owned(),
            state_database: "/tmp/state.sqlite3".to_owned(),
            poll_timeout_seconds: 40,
            retry_seconds: 2,
            extra_service_units: Vec::new(),
            service_actions_enabled: false,
            action_executor: "/usr/lib/g7telegram-devops/g7tg-exec".to_owned(),
            approval_ttl_seconds: 45,
            web_checks: Vec::new(),
            monitor_interval_seconds: 60,
            incident_confirmation_count: 2,
            cpu_warning_percent: 90.0,
            load_warning_per_cpu: 1.5,
            memory_warning_percent: 90.0,
            swap_warning_percent: 80.0,
            disk_warning_percent: 85.0,
        }
    }

    fn snapshot() -> SystemSnapshot {
        SystemSnapshot {
            server_name: "demo".to_owned(),
            hostname: "demo".to_owned(),
            os_name: "Ubuntu".to_owned(),
            kernel_version: "6.8".to_owned(),
            uptime_seconds: 600,
            cpu_usage_percent: 10.0,
            logical_cpu_count: 2,
            load_one: 0.2,
            memory_total_bytes: 100,
            memory_used_bytes: 50,
            swap_total_bytes: 100,
            swap_used_bytes: 0,
            disks: Vec::new(),
        }
    }

    #[test]
    fn sustained_resource_thresholds_create_stable_incident_keys() {
        let mut snapshot = snapshot();
        snapshot.cpu_usage_percent = 95.0;
        snapshot.load_one = 3.2;
        snapshot.memory_used_bytes = 95;
        snapshot.swap_used_bytes = 85;

        let incidents = observe(&config(), &snapshot, &[], &[]);
        let keys: Vec<_> = incidents
            .iter()
            .map(|incident| incident.key.as_str())
            .collect();
        assert_eq!(
            keys,
            [
                "system:cpu",
                "system:load",
                "system:memory",
                "system:swap_pressure"
            ]
        );
    }

    #[test]
    fn swap_pages_without_memory_pressure_do_not_alert() {
        let mut snapshot = snapshot();
        snapshot.swap_used_bytes = 95;

        let incidents = observe(&config(), &snapshot, &[], &[]);
        assert!(incidents.is_empty());
    }

    #[test]
    fn load_threshold_is_normalized_by_logical_cpu_count() {
        let mut snapshot = snapshot();
        snapshot.logical_cpu_count = 4;
        snapshot.load_one = 5.9;
        assert!(observe(&config(), &snapshot, &[], &[]).is_empty());

        snapshot.load_one = 6.0;
        let incidents = observe(&config(), &snapshot, &[], &[]);
        assert_eq!(incidents.len(), 1);
        assert_eq!(incidents[0].key, "system:load");
    }
}
