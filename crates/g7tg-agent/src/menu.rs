//! Telegram 메뉴 구성과 읽기 전용 화면 rendering입니다.

use g7tg_core::{Menu, ServiceAction, ServiceStatus, SystemSnapshot};
use serde_json::{Value, json};

use crate::{
    config::{AgentConfig, WebCheckConfig},
    storage::CurrentIncident,
    telegram::{InlineKeyboardButton, InlineKeyboardMarkup},
    ui,
};

const SERVICE_PAGE_SIZE: usize = 8;

/// 메뉴 화면의 text와 inline keyboard입니다.
pub struct MenuView {
    /// 사용자에게 보낼 본문입니다.
    pub text: String,
    /// 이동 가능한 버튼입니다.
    pub keyboard: InlineKeyboardMarkup,
}

/// 하단에 고정할 `메뉴` 버튼입니다.
#[must_use]
pub fn persistent_menu_keyboard() -> Value {
    json!({
        "keyboard": [[{"text": "메뉴"}]],
        "resize_keyboard": true,
        "is_persistent": true,
        "input_field_placeholder": "메뉴를 선택하세요"
    })
}

/// 현재 메뉴를 render합니다.
#[must_use]
pub fn render(menu: Menu, snapshot: Option<&SystemSnapshot>, config: &AgentConfig) -> MenuView {
    match menu {
        Menu::Main => MenuView {
            text: "🛰️ 서버 관리 메뉴\n원하는 항목을 선택하세요.".to_owned(),
            keyboard: InlineKeyboardMarkup {
                inline_keyboard: vec![
                    vec![
                        button("🖥 서버 상태", "menu:system"),
                        button("⚙️ 서비스", "menu:services"),
                    ],
                    vec![
                        button("🌐 웹 상태", "menu:web"),
                        button("🚨 장애/알림", "menu:alerts"),
                    ],
                    vec![
                        button("🔧 설정", "menu:settings"),
                        button("ℹ️ Agent 정보", "menu:info"),
                    ],
                ],
            },
        },
        Menu::System => MenuView {
            text: snapshot.map_or_else(
                || "🔴 서버 상태를 수집하지 못했습니다.".to_owned(),
                |snapshot| format_system_snapshot(snapshot, config),
            ),
            keyboard: refresh_and_back("menu:system"),
        },
        Menu::Services => placeholder("서비스", "다음 배치에서 자동 탐지를 연결합니다."),
        Menu::Web => placeholder("웹 상태", "HTTP/TLS 검증 배치에서 연결합니다."),
        Menu::Alerts => placeholder("장애/알림", "현재 등록된 장애가 없습니다."),
        Menu::Settings => render_settings(None, config.server_reboot_enabled),
        Menu::Power => render_power_menu(&config.server_name),
        Menu::Info => MenuView {
            text: format!(
                "ℹ️ G7Telegram DevOps\nTelegram으로 서버 상태와 웹·서비스를 확인하고, 장애 알림과 승인형 재시작을 제공하는 서버 관리 앱입니다.\n버전: {}",
                env!("CARGO_PKG_VERSION")
            ),
            keyboard: back_only(),
        },
    }
}

/// 탐지한 서비스 목록을 분류해 render합니다.
#[must_use]
pub fn render_services(services: &[ServiceStatus]) -> MenuView {
    render_services_page(services, 0)
}

/// 탐지한 서비스 목록의 지정 페이지를 render합니다.
#[must_use]
pub fn render_services_page(services: &[ServiceStatus], requested_page: usize) -> MenuView {
    if services.is_empty() {
        return MenuView {
            text: "⚙️ 서비스\n⚪ 관리 대상 웹서비스를 발견하지 못했습니다.".to_owned(),
            keyboard: refresh_and_back("menu:services"),
        };
    }
    let healthy = services
        .iter()
        .filter(|service| service.is_healthy())
        .count();
    let page_count = services.len().div_ceil(SERVICE_PAGE_SIZE);
    let page = requested_page.min(page_count.saturating_sub(1));
    let start = page.saturating_mul(SERVICE_PAGE_SIZE);
    let end = start.saturating_add(SERVICE_PAGE_SIZE).min(services.len());
    let mut lines = vec![format!(
        "⚙️ 서비스 상태 · {}/{}\n🟢 정상 {healthy}개 · ⚠️ 확인 {}개",
        page + 1,
        page_count,
        services.len().saturating_sub(healthy)
    )];
    let mut rows = Vec::new();
    let mut previous_category = None;
    for service in &services[start..end] {
        if previous_category != Some(service.category) {
            lines.push(format!("\n[{}]", service.category.label()));
            previous_category = Some(service.category);
        }
        let status_icon = ui::service_status(service);
        lines.push(format!(
            "{status_icon} {} · {}",
            service.unit,
            service.state_label()
        ));
        rows.push(vec![button(
            &format!(
                "{status_icon} {} · {}",
                service.state_label(),
                short_unit(&service.unit)
            ),
            &format!("service:{}", crate::services::service_key(&service.unit)),
        )]);
    }
    let mut navigation = Vec::new();
    if page > 0 {
        navigation.push(button("⬅️ 이전", &format!("menu:services:{}", page - 1)));
    }
    if page + 1 < page_count {
        navigation.push(button("다음 ➡️", &format!("menu:services:{}", page + 1)));
    }
    if !navigation.is_empty() {
        rows.push(navigation);
    }
    rows.push(vec![
        button("🔄 새로고침", &format!("menu:services:{page}")),
        button("⬅️ 뒤로가기", "menu:main"),
    ]);
    MenuView {
        text: lines.join("\n"),
        keyboard: InlineKeyboardMarkup {
            inline_keyboard: rows,
        },
    }
}

/// 정기 상태 요약의 제한된 Telegram 설정 화면입니다.
#[must_use]
pub fn render_settings(
    status_digest_interval_seconds: Option<u64>,
    server_reboot_enabled: bool,
) -> MenuView {
    let current = match status_digest_interval_seconds {
        Some(21_600) => "6시간",
        Some(43_200) => "12시간",
        Some(86_400) => "24시간",
        _ => "꺼짐",
    };
    let selected = |seconds: Option<u64>, label: &str| {
        if status_digest_interval_seconds == seconds {
            format!("✓ {label}")
        } else {
            label.to_owned()
        }
    };
    let mut rows = vec![
        vec![
            button(&selected(None, "꺼짐"), "digest:off"),
            button(&selected(Some(21_600), "6시간"), "digest:21600"),
        ],
        vec![
            button(&selected(Some(43_200), "12시간"), "digest:43200"),
            button(&selected(Some(86_400), "24시간"), "digest:86400"),
        ],
    ];
    if server_reboot_enabled {
        rows.push(vec![button("⚡ 전원 관리", "menu:power")]);
    }
    rows.push(vec![button("⬅️ 뒤로가기", "menu:main")]);
    MenuView {
        text: format!(
            "🔧 설정\n📊 정기 상태 요약: {current}\n⚡ 서버 재시작: {}\n\n🚨 장애·복구 알림은 항상 즉시 전송합니다. 정기 요약은 선택한 간격으로 자원·서비스·웹 상태를 알려드립니다.",
            if server_reboot_enabled {
                "사용"
            } else {
                "사용 안 함"
            }
        ),
        keyboard: InlineKeyboardMarkup {
            inline_keyboard: rows,
        },
    }
}

/// 로컬 setup에서 허용한 서버 전원 관리 화면입니다.
#[must_use]
pub fn render_power_menu(server_name: &str) -> MenuView {
    MenuView {
        text: format!(
            "⚡ 전원 관리 · {server_name}\n\n서버 전체를 재시작하면 웹서비스와 Telegram Agent가 함께 중단됩니다. 버튼을 누른 뒤에도 일회용 확인문구를 직접 입력해야 실행됩니다."
        ),
        keyboard: InlineKeyboardMarkup {
            inline_keyboard: vec![
                vec![button("🔄 서버 재시작", "power:plan")],
                vec![button("⬅️ 뒤로가기", "menu:settings")],
            ],
        },
    }
}

/// 서버 재시작의 영향과 일회용 확인문구를 보여줍니다.
#[must_use]
pub fn render_reboot_confirmation(phrase: &str, ttl_seconds: u64) -> MenuView {
    MenuView {
        text: format!(
            "⚠️ 서버 전체 재시작 확인\n\n웹서비스와 Telegram Agent가 잠시 중단됩니다.\n{ttl_seconds}초 안에 아래 문구를 채팅창에 정확히 입력하세요.\n\n{phrase}\n\n버튼만으로는 재시작되지 않습니다."
        ),
        keyboard: InlineKeyboardMarkup {
            inline_keyboard: vec![vec![
                button("❌ 취소", "power:cancel"),
                button("⬅️ 뒤로가기", "menu:power"),
            ]],
        },
    }
}

/// 설정된 웹 endpoint의 최소 가용성 결과를 render합니다.
#[must_use]
pub fn render_web_checks(
    results: &[g7tg_core::WebCheckResult],
    checks: &[WebCheckConfig],
) -> MenuView {
    if results.is_empty() {
        return MenuView {
            text: "🌐 웹 상태\n⚪ 검사 대상이 설정되지 않았습니다.\n서비스 상태만으로 운영합니다."
                .to_owned(),
            keyboard: back_only(),
        };
    }
    let mut lines = vec!["🌐 웹 상태".to_owned()];
    for result in results {
        let warning_days = checks
            .iter()
            .find(|check| check.name == result.name)
            .map_or(14, |check| check.tls_warning_days);
        let tls_warning = result
            .tls_days_remaining
            .is_some_and(|days| days > 0 && days <= warning_days);
        let (status_icon, state) = if !result.healthy {
            (ui::CRITICAL, "장애")
        } else if tls_warning {
            (ui::WARNING, "주의")
        } else {
            (ui::HEALTHY, "정상")
        };
        let status = result
            .status_code
            .map_or_else(|| "-".to_owned(), |status| status.to_string());
        let latency = result
            .latency_ms
            .map_or_else(|| "-".to_owned(), |latency| format!("{latency}ms"));
        lines.push(format!("\n{status_icon} {} · {state}", result.name));
        lines.push(format!("HTTP: {status} · 응답: {latency}"));
        if let Some(days) = result.tls_days_remaining {
            lines.push(format!("TLS: {days}일 남음"));
        }
        if let Some(error) = &result.error_code {
            lines.push(format!("오류: {error}"));
        }
    }
    MenuView {
        text: lines.join("\n"),
        keyboard: refresh_and_back("menu:web"),
    }
}

/// 확인 횟수를 통과한 현재 장애를 render합니다.
#[must_use]
pub fn render_alerts(incidents: &[CurrentIncident], silence_until: Option<i64>) -> MenuView {
    let mut lines = vec!["🚨 장애/알림".to_owned()];
    if let Some(expires_at) = silence_until {
        let remaining_minutes = expires_at
            .saturating_sub(time::OffsetDateTime::now_utc().unix_timestamp())
            .saturating_add(59)
            / 60;
        lines.push(format!(
            "🔕 알림 일시중지 중 · 약 {remaining_minutes}분 남음"
        ));
    }
    if incidents.is_empty() {
        lines.push("🟢 현재 확인된 장애가 없습니다.".to_owned());
    } else {
        for incident in incidents {
            let icon = ui::severity_status(&incident.severity);
            let label = ui::severity_label(&incident.severity);
            lines.push(format!(
                "\n{icon} [{label}] {}\n{}",
                incident.key, incident.summary
            ));
        }
    }
    let mut rows = if silence_until.is_some() {
        vec![vec![button("🔔 알림중지 해제", "silence:clear")]]
    } else {
        vec![vec![
            button("🔕 1시간 중지", "silence:3600"),
            button("🔕 6시간 중지", "silence:21600"),
        ]]
    };
    rows.push(vec![
        button("🔄 새로고침", "menu:alerts"),
        button("⬅️ 뒤로가기", "menu:main"),
    ]);
    MenuView {
        text: lines.join("\n"),
        keyboard: InlineKeyboardMarkup {
            inline_keyboard: rows,
        },
    }
}

/// 단일 서비스의 systemd 상태를 render합니다.
#[must_use]
pub fn render_service_detail(service: &ServiceStatus, action_allowed: bool) -> MenuView {
    let mut rows = Vec::new();
    if action_allowed {
        rows.push(vec![button(
            "🔄 서비스 재시작",
            &format!(
                "action:plan:{}:{}",
                ServiceAction::Restart.id(),
                crate::services::service_key(&service.unit)
            ),
        )]);
    }
    rows.push(vec![
        button(
            "🔄 새로고침",
            &format!("service:{}", crate::services::service_key(&service.unit)),
        ),
        button("⬅️ 뒤로가기", "menu:services"),
    ]);
    MenuView {
        text: format!(
            "⚙️ 서비스 상세\n이름: {}\n설명: {}\n분류: {}\n상태: {} {}\nActiveState: {}\nSubState: {}\nLoadState: {}",
            service.unit,
            service.description,
            service.category.label(),
            ui::service_status(service),
            service.state_label(),
            service.active_state,
            service.sub_state,
            service.load_state
        ),
        keyboard: InlineKeyboardMarkup {
            inline_keyboard: rows,
        },
    }
}

/// 재시작의 대상과 영향, 만료를 보여주는 재승인 화면입니다.
#[must_use]
pub fn render_action_confirmation(
    service: &ServiceStatus,
    action: ServiceAction,
    token: &str,
    ttl_seconds: u64,
) -> MenuView {
    MenuView {
        text: format!(
            "⚠️ 서비스 {} 승인\n대상: {}\n현재 상태: {} {} / {}\n영향: 진행 중인 연결 또는 작업이 잠시 중단될 수 있습니다.\n유효시간: {ttl_seconds}초\n\n실행하시겠습니까?",
            action.label(),
            service.unit,
            ui::service_status(service),
            service.active_state,
            service.sub_state
        ),
        keyboard: InlineKeyboardMarkup {
            inline_keyboard: vec![vec![
                button("✅ 승인하고 실행", &format!("action:confirm:{token}")),
                button("❌ 취소", &format!("action:cancel:{token}")),
            ]],
        },
    }
}

/// 서비스 동작 결과 화면입니다.
#[must_use]
pub fn render_action_result(
    service: &ServiceStatus,
    action: ServiceAction,
    success: bool,
) -> MenuView {
    let (outcome_icon, outcome) = if success {
        ("✅", "성공")
    } else {
        ("❌", "실패")
    };
    MenuView {
        text: format!(
            "{outcome_icon} 서비스 {} {outcome}\n대상: {}\n현재 상태: {} {}\nActiveState: {}\nSubState: {}",
            action.label(),
            service.unit,
            ui::service_status(service),
            service.state_label(),
            service.active_state,
            service.sub_state
        ),
        keyboard: InlineKeyboardMarkup {
            inline_keyboard: vec![vec![
                button(
                    "⚙️ 서비스 상세",
                    &format!("service:{}", crate::services::service_key(&service.unit)),
                ),
                button("📋 목록", "menu:services"),
            ]],
        },
    }
}

/// 취소 또는 만료 결과입니다.
#[must_use]
pub fn render_action_cancelled(message: &str) -> MenuView {
    MenuView {
        text: format!("↩️ {message}"),
        keyboard: InlineKeyboardMarkup {
            inline_keyboard: vec![vec![button("📋 서비스 목록", "menu:services")]],
        },
    }
}

fn format_system_snapshot(snapshot: &SystemSnapshot, config: &AgentConfig) -> String {
    let memory_percent = percent(snapshot.memory_used_bytes, snapshot.memory_total_bytes);
    let swap_percent = percent(snapshot.swap_used_bytes, snapshot.swap_total_bytes);
    let disk_rows: Vec<_> = snapshot
        .disks
        .iter()
        .map(|disk| {
            let used = disk.total_bytes.saturating_sub(disk.available_bytes);
            (
                disk.mount_point.as_str(),
                format!(
                    "{}/{}",
                    format_bytes_compact(used),
                    format_bytes_compact(disk.total_bytes)
                ),
                percent(used, disk.total_bytes),
            )
        })
        .collect();
    let mount_width = disk_rows
        .iter()
        .map(|(mount, _, _)| mount.chars().count())
        .max()
        .unwrap_or(4)
        .max(4);
    let usage_width = disk_rows
        .iter()
        .map(|(_, usage, _)| usage.chars().count())
        .max()
        .unwrap_or(10)
        .max(10);
    let cpu_warning = f64::from(snapshot.cpu_usage_percent) >= config.cpu_warning_percent;
    let load_per_cpu = snapshot.load_one / f64::from(snapshot.logical_cpu_count.max(1));
    let load_warning = load_per_cpu >= config.load_warning_per_cpu;
    let memory_warning = memory_percent >= config.memory_warning_percent;
    let swap_warning = snapshot.swap_total_bytes > 0
        && swap_percent >= config.swap_warning_percent
        && memory_warning;
    let mut overall = ui::HEALTHY;
    if cpu_warning || load_warning || memory_warning || swap_warning {
        overall = ui::WARNING;
    }
    for (_, _, usage_percent) in &disk_rows {
        let disk_icon = ui::disk_status(*usage_percent, config.disk_warning_percent);
        if disk_icon == ui::CRITICAL {
            overall = ui::CRITICAL;
            break;
        }
        if disk_icon == ui::WARNING {
            overall = ui::WARNING;
        }
    }
    let mut lines = vec![
        format!(
            "{overall} 서버 상태 · {}",
            compact_text(&snapshot.server_name, 22)
        ),
        "------------------------------".to_owned(),
        format!(
            "{} CPU    {:>5.1}%",
            ui::resource_status(cpu_warning),
            snapshot.cpu_usage_percent
        ),
        format!(
            "{} LOAD   {:>5.2} / {} CPU",
            ui::resource_status(load_warning),
            snapshot.load_one,
            snapshot.logical_cpu_count
        ),
        format!(
            "{} RAM    {:>5} / {:<5} {:>5.1}%",
            ui::resource_status(memory_warning),
            format_bytes_compact(snapshot.memory_used_bytes),
            format_bytes_compact(snapshot.memory_total_bytes),
            memory_percent
        ),
        format!(
            "{} SWAP   {:>5} / {:<5} {:>5.1}%",
            if snapshot.swap_total_bytes == 0 {
                ui::INACTIVE
            } else {
                ui::resource_status(swap_warning)
            },
            format_bytes_compact(snapshot.swap_used_bytes),
            format_bytes_compact(snapshot.swap_total_bytes),
            swap_percent
        ),
        String::new(),
        format!(
            "{:<mount_width$} {:>usage_width$} {:>6}",
            "💾 DISK", "USED/TOTAL", "USE"
        ),
    ];
    for (mount, usage, usage_percent) in disk_rows {
        lines.push(format!(
            "{} {mount:<mount_width$} {usage:>usage_width$} {usage_percent:>5.1}%",
            ui::disk_status(usage_percent, config.disk_warning_percent)
        ));
    }
    lines.extend([
        String::new(),
        "------------------------------".to_owned(),
        format!("UP     {}", format_uptime(snapshot.uptime_seconds)),
        format!("HOST   {}", compact_text(&snapshot.hostname, 32)),
        format!("OS     {}", compact_text(&snapshot.os_name, 32)),
        format!("KERN   {}", compact_text(&snapshot.kernel_version, 32)),
        format!("CHECK  {}", checked_at_utc()),
    ]);
    lines.join("\n")
}

fn checked_at_utc() -> String {
    let now = time::OffsetDateTime::now_utc();
    format!(
        "{:04}-{:02}-{:02} {:02}:{:02} UTC",
        now.year(),
        u8::from(now.month()),
        now.day(),
        now.hour(),
        now.minute()
    )
}

fn placeholder(title: &str, body: &str) -> MenuView {
    MenuView {
        text: format!("{title}\n{body}"),
        keyboard: back_only(),
    }
}

fn short_unit(unit: &str) -> String {
    const MAX_CHARS: usize = 32;
    let mut short: String = unit.chars().take(MAX_CHARS).collect();
    if unit.chars().count() > MAX_CHARS {
        short.push('…');
    }
    short
}

fn refresh_and_back(callback: &str) -> InlineKeyboardMarkup {
    InlineKeyboardMarkup {
        inline_keyboard: vec![vec![
            button("🔄 새로고침", callback),
            button("⬅️ 뒤로가기", "menu:main"),
        ]],
    }
}

fn back_only() -> InlineKeyboardMarkup {
    InlineKeyboardMarkup {
        inline_keyboard: vec![vec![button("⬅️ 뒤로가기", "menu:main")]],
    }
}

fn button(text: &str, data: &str) -> InlineKeyboardButton {
    InlineKeyboardButton::callback(text, data)
}

fn percent(used: u64, total: u64) -> f64 {
    if total == 0 {
        0.0
    } else {
        used as f64 * 100.0 / total as f64
    }
}

fn format_bytes_compact(bytes: u64) -> String {
    const GIB: f64 = 1024.0 * 1024.0 * 1024.0;
    const MIB: f64 = 1024.0 * 1024.0;
    if bytes as f64 >= GIB {
        format!("{:.1}G", bytes as f64 / GIB)
    } else {
        format!("{:.0}M", bytes as f64 / MIB)
    }
}

fn format_uptime(seconds: u64) -> String {
    let days = seconds / 86_400;
    let hours = seconds % 86_400 / 3_600;
    let minutes = seconds % 3_600 / 60;
    format!("{days}d {hours}h {minutes}m")
}

fn compact_text(value: &str, max_chars: usize) -> String {
    let count = value.chars().count();
    if count <= max_chars {
        return value.to_owned();
    }
    let mut compact: String = value.chars().take(max_chars.saturating_sub(1)).collect();
    compact.push('~');
    compact
}

#[cfg(test)]
mod tests {
    use g7tg_core::{
        DiskSnapshot, Menu, ServiceAction, ServiceCategory, ServiceStatus, SystemSnapshot,
        WebCheckResult,
    };

    use super::{
        render, render_action_confirmation, render_alerts, render_service_detail,
        render_services_page, render_settings, render_web_checks,
    };
    use crate::{
        config::{AgentConfig, WebCheckConfig},
        storage::CurrentIncident,
    };

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
            server_reboot_enabled: false,
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

    #[test]
    fn system_menu_has_refresh_and_back() {
        let mut snapshot = SystemSnapshot {
            server_name: "demo".to_owned(),
            hostname: "demo-host".to_owned(),
            os_name: "Ubuntu".to_owned(),
            kernel_version: "6.8".to_owned(),
            uptime_seconds: 90_061,
            cpu_usage_percent: 12.5,
            logical_cpu_count: 2,
            load_one: 0.2,
            memory_total_bytes: 2 * 1024 * 1024 * 1024,
            memory_used_bytes: 1024 * 1024 * 1024,
            swap_total_bytes: 0,
            swap_used_bytes: 0,
            disks: vec![
                DiskSnapshot {
                    mount_point: "/".to_owned(),
                    total_bytes: 10 * 1024 * 1024 * 1024,
                    available_bytes: 5 * 1024 * 1024 * 1024,
                },
                DiskSnapshot {
                    mount_point: "/boot/efi".to_owned(),
                    total_bytes: 100 * 1024 * 1024,
                    available_bytes: 94 * 1024 * 1024,
                },
            ],
        };
        let view = render(Menu::System, Some(&snapshot), &config());
        assert!(view.text.contains("🟢 RAM     1.0G / 2.0G   50.0%"));
        assert!(view.text.contains("DISK"));
        assert!(view.text.contains("USED/TOTAL"));
        assert!(
            view.text
                .lines()
                .any(|line| line.contains("/ ") && line.ends_with("5.0G/10.0G  50.0%"))
        );
        assert!(
            view.text
                .lines()
                .any(|line| line.contains("/boot/efi") && line.ends_with("6M/100M   6.0%"))
        );
        assert!(!view.text.contains('~'));
        assert!(view.text.contains("CHECK"));
        assert!(view.text.contains("UTC"));
        assert!(view.text.lines().all(|line| line.chars().count() <= 30));
        assert_eq!(view.keyboard.inline_keyboard[0].len(), 2);

        let long_mount = "/var/lib/g7telegram-devops/runtime/monitoring-data";
        snapshot.disks = vec![DiskSnapshot {
            mount_point: long_mount.to_owned(),
            total_bytes: 10 * 1024 * 1024 * 1024,
            available_bytes: 5 * 1024 * 1024 * 1024,
        }];
        let long_view = render(Menu::System, Some(&snapshot), &config());
        assert!(
            long_view
                .text
                .lines()
                .any(|line| line.contains(long_mount) && line.ends_with("5.0G/10.0G  50.0%"))
        );
    }

    #[test]
    fn system_menu_marks_warning_and_critical_resources() {
        let snapshot = SystemSnapshot {
            server_name: "demo".to_owned(),
            hostname: "demo-host".to_owned(),
            os_name: "Ubuntu".to_owned(),
            kernel_version: "6.8".to_owned(),
            uptime_seconds: 600,
            cpu_usage_percent: 91.0,
            logical_cpu_count: 2,
            load_one: 0.2,
            memory_total_bytes: 100,
            memory_used_bytes: 50,
            swap_total_bytes: 0,
            swap_used_bytes: 0,
            disks: vec![DiskSnapshot {
                mount_point: "/".to_owned(),
                total_bytes: 100,
                available_bytes: 5,
            }],
        };

        let view = render(Menu::System, Some(&snapshot), &config());
        assert!(view.text.starts_with("🔴 서버 상태"));
        assert!(view.text.contains("🟡 CPU"));
        assert!(view.text.lines().any(|line| line.starts_with("🔴 /")));
    }

    #[test]
    fn service_list_is_paginated_without_omission() {
        let services: Vec<_> = (0..18)
            .map(|index| ServiceStatus {
                unit: format!("demo-{index}.service"),
                description: format!("Demo {index}"),
                category: ServiceCategory::Application,
                load_state: "loaded".to_owned(),
                active_state: "active".to_owned(),
                sub_state: "running".to_owned(),
            })
            .collect();
        let first = render_services_page(&services, 0);
        assert!(first.text.contains("1/3"));
        assert!(first.text.contains("demo-0.service"));
        assert!(!first.text.contains("demo-8.service"));
        assert!(
            first
                .keyboard
                .inline_keyboard
                .iter()
                .flatten()
                .any(|button| { button.callback_data == "menu:services:1" })
        );

        let last = render_services_page(&services, 2);
        assert!(last.text.contains("3/3"));
        assert!(last.text.contains("demo-17.service"));
        assert!(!last.text.contains("생략"));
    }

    #[test]
    fn settings_exposes_only_fixed_digest_intervals() {
        let view = render_settings(Some(43_200), true);
        assert!(view.text.contains("정기 상태 요약: 12시간"));
        let callbacks: Vec<_> = view
            .keyboard
            .inline_keyboard
            .iter()
            .flatten()
            .map(|button| button.callback_data.as_str())
            .collect();
        assert!(callbacks.contains(&"digest:off"));
        assert!(callbacks.contains(&"digest:21600"));
        assert!(callbacks.contains(&"digest:43200"));
        assert!(callbacks.contains(&"digest:86400"));
        assert!(callbacks.contains(&"menu:power"));
    }

    #[test]
    fn reboot_confirmation_requires_typed_phrase_without_execute_button() {
        let phrase = "서버재시작 G7Devops A1B2C3D4";
        let view = super::render_reboot_confirmation(phrase, 60);
        assert!(view.text.contains(phrase));
        assert!(view.text.contains("60초"));
        assert!(
            view.keyboard
                .inline_keyboard
                .iter()
                .flatten()
                .all(|button| button.callback_data != "power:execute")
        );
        assert!(
            view.keyboard
                .inline_keyboard
                .iter()
                .flatten()
                .any(|button| button.callback_data == "power:cancel")
        );
    }

    #[test]
    fn web_checks_distinguish_tls_warning_and_failure() {
        let checks = vec![WebCheckConfig {
            name: "쇼핑몰".to_owned(),
            url: "https://example.com/".to_owned(),
            expected_status_min: 200,
            expected_status_max: 399,
            timeout_seconds: 5,
            tls_warning_days: 14,
        }];
        let results = vec![
            WebCheckResult {
                name: "쇼핑몰".to_owned(),
                url: "https://example.com/".to_owned(),
                status_code: Some(200),
                latency_ms: Some(25),
                tls_days_remaining: Some(7),
                healthy: true,
                error_code: None,
            },
            WebCheckResult {
                name: "관리자".to_owned(),
                url: "https://admin.example.com/".to_owned(),
                status_code: Some(503),
                latency_ms: Some(30),
                tls_days_remaining: Some(30),
                healthy: false,
                error_code: Some("unexpected_status".to_owned()),
            },
        ];

        let view = render_web_checks(&results, &checks);
        assert!(view.text.contains("🟡 쇼핑몰 · 주의"));
        assert!(view.text.contains("🔴 관리자 · 장애"));
    }

    #[test]
    fn alert_screen_uses_severity_icons_and_labels() {
        let incidents = vec![
            CurrentIncident {
                key: "system:cpu".to_owned(),
                severity: "warning".to_owned(),
                summary: "CPU 사용률 95.0%".to_owned(),
                first_seen: 1,
            },
            CurrentIncident {
                key: "service:nginx.service".to_owned(),
                severity: "critical".to_owned(),
                summary: "Nginx 중지".to_owned(),
                first_seen: 2,
            },
        ];

        let view = render_alerts(&incidents, None);
        assert!(view.text.contains("🟡 [주의] system:cpu"));
        assert!(view.text.contains("🔴 [장애] service:nginx.service"));
    }

    #[test]
    fn agent_info_explains_the_app_without_architecture_details() {
        let view = render(Menu::Info, None, &config());
        assert!(view.text.contains("서버 상태와 웹·서비스"));
        assert!(view.text.contains("장애 알림과 승인형 재시작"));
        assert!(view.text.contains(env!("CARGO_PKG_VERSION")));
        assert!(!view.text.contains("구조:"));
        assert!(!view.text.contains("중앙 서버"));
    }

    #[test]
    fn action_callbacks_fit_telegram_limit_and_include_cancel() {
        let service = ServiceStatus {
            unit: "g7-reverb.service".to_owned(),
            description: "G7 Reverb".to_owned(),
            category: ServiceCategory::Application,
            load_state: "loaded".to_owned(),
            active_state: "active".to_owned(),
            sub_state: "running".to_owned(),
        };
        let detail = render_service_detail(&service, true);
        let plan = &detail.keyboard.inline_keyboard[0][0].callback_data;
        assert!(plan.starts_with("action:plan:restart:"));
        assert!(plan.len() <= 64);

        let confirmation = render_action_confirmation(
            &service,
            ServiceAction::Restart,
            "0123456789abcdef0123456789abcdef",
            45,
        );
        let callbacks: Vec<_> = confirmation.keyboard.inline_keyboard[0]
            .iter()
            .map(|button| button.callback_data.as_str())
            .collect();
        assert!(callbacks.iter().all(|callback| callback.len() <= 64));
        assert!(
            callbacks
                .iter()
                .any(|callback| callback.starts_with("action:confirm:"))
        );
        assert!(
            callbacks
                .iter()
                .any(|callback| callback.starts_with("action:cancel:"))
        );
    }
}
