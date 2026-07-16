//! Telegram 메뉴 구성과 읽기 전용 화면 rendering입니다.

use g7tg_core::{Menu, ServiceAction, ServiceStatus, SystemSnapshot};
use serde_json::{Value, json};

use crate::{
    storage::CurrentIncident,
    telegram::{InlineKeyboardButton, InlineKeyboardMarkup},
};

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
pub fn render(menu: Menu, snapshot: Option<&SystemSnapshot>) -> MenuView {
    match menu {
        Menu::Main => MenuView {
            text: "서버 관리 메뉴\n원하는 항목을 선택하세요.".to_owned(),
            keyboard: InlineKeyboardMarkup {
                inline_keyboard: vec![
                    vec![
                        button("서버 상태", "menu:system"),
                        button("서비스", "menu:services"),
                    ],
                    vec![
                        button("웹 상태", "menu:web"),
                        button("장애/알림", "menu:alerts"),
                    ],
                    vec![button("Agent 정보", "menu:info")],
                ],
            },
        },
        Menu::System => MenuView {
            text: snapshot.map_or_else(
                || "서버 상태를 수집하지 못했습니다.".to_owned(),
                format_system_snapshot,
            ),
            keyboard: refresh_and_back("menu:system"),
        },
        Menu::Services => placeholder("서비스", "다음 배치에서 자동 탐지를 연결합니다."),
        Menu::Web => placeholder("웹 상태", "HTTP/TLS 검증 배치에서 연결합니다."),
        Menu::Alerts => placeholder("장애/알림", "현재 등록된 장애가 없습니다."),
        Menu::Info => MenuView {
            text: format!(
                "G7Telegram DevOps\n버전: {}\n구조: VPS 1대 ↔ Bot 1개\n중앙 서버: 사용하지 않음",
                env!("CARGO_PKG_VERSION")
            ),
            keyboard: back_only(),
        },
    }
}

/// 탐지한 서비스 목록을 분류해 render합니다.
#[must_use]
pub fn render_services(services: &[ServiceStatus]) -> MenuView {
    if services.is_empty() {
        return MenuView {
            text: "서비스\n관리 대상 웹서비스를 발견하지 못했습니다.".to_owned(),
            keyboard: refresh_and_back("menu:services"),
        };
    }
    let healthy = services
        .iter()
        .filter(|service| service.is_healthy())
        .count();
    let mut lines = vec![format!(
        "서비스 상태\n정상 {healthy}개 · 확인필요 {}개",
        services.len().saturating_sub(healthy)
    )];
    let mut rows = Vec::new();
    let mut previous_category = None;
    for service in services.iter().take(24) {
        if previous_category != Some(service.category) {
            lines.push(format!("\n[{}]", service.category.label()));
            previous_category = Some(service.category);
        }
        lines.push(format!("{} · {}", service.unit, service.state_label()));
        rows.push(vec![button(
            &format!("{} · {}", service.state_label(), short_unit(&service.unit)),
            &format!("service:{}", crate::services::service_key(&service.unit)),
        )]);
    }
    if services.len() > 24 {
        lines.push(format!(
            "\n외 {}개는 화면 한도로 생략했습니다.",
            services.len() - 24
        ));
    }
    rows.push(vec![
        button("새로고침", "menu:services"),
        button("뒤로가기", "menu:main"),
    ]);
    MenuView {
        text: lines.join("\n"),
        keyboard: InlineKeyboardMarkup {
            inline_keyboard: rows,
        },
    }
}

/// 설정된 웹 endpoint의 최소 가용성 결과를 render합니다.
#[must_use]
pub fn render_web_checks(results: &[g7tg_core::WebCheckResult]) -> MenuView {
    if results.is_empty() {
        return MenuView {
            text: "웹 상태\n검사 대상이 설정되지 않았습니다.\n서비스 상태만으로 운영합니다."
                .to_owned(),
            keyboard: back_only(),
        };
    }
    let mut lines = vec!["웹 상태".to_owned()];
    for result in results {
        let state = if result.healthy {
            "정상"
        } else {
            "확인필요"
        };
        let status = result
            .status_code
            .map_or_else(|| "-".to_owned(), |status| status.to_string());
        let latency = result
            .latency_ms
            .map_or_else(|| "-".to_owned(), |latency| format!("{latency}ms"));
        lines.push(format!("\n{} · {state}", result.name));
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
    let mut lines = vec!["장애/알림".to_owned()];
    if let Some(expires_at) = silence_until {
        let remaining_minutes = expires_at
            .saturating_sub(time::OffsetDateTime::now_utc().unix_timestamp())
            .saturating_add(59)
            / 60;
        lines.push(format!("알림 일시중지 중 · 약 {remaining_minutes}분 남음"));
    }
    if incidents.is_empty() {
        lines.push("현재 확인된 장애가 없습니다.".to_owned());
    } else {
        for incident in incidents {
            lines.push(format!(
                "\n[{}] {}\n{}",
                incident.severity, incident.key, incident.summary
            ));
        }
    }
    let mut rows = if silence_until.is_some() {
        vec![vec![button("알림중지 해제", "silence:clear")]]
    } else {
        vec![vec![
            button("1시간 중지", "silence:3600"),
            button("6시간 중지", "silence:21600"),
        ]]
    };
    rows.push(vec![
        button("새로고침", "menu:alerts"),
        button("뒤로가기", "menu:main"),
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
            "서비스 재시작",
            &format!(
                "action:plan:{}:{}",
                ServiceAction::Restart.id(),
                crate::services::service_key(&service.unit)
            ),
        )]);
    }
    rows.push(vec![
        button(
            "새로고침",
            &format!("service:{}", crate::services::service_key(&service.unit)),
        ),
        button("뒤로가기", "menu:services"),
    ]);
    MenuView {
        text: format!(
            "서비스 상세\n이름: {}\n설명: {}\n분류: {}\n상태: {}\nActiveState: {}\nSubState: {}\nLoadState: {}",
            service.unit,
            service.description,
            service.category.label(),
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
            "서비스 {} 승인\n대상: {}\n현재 상태: {} / {}\n영향: 진행 중인 연결 또는 작업이 잠시 중단될 수 있습니다.\n유효시간: {ttl_seconds}초\n\n실행하시겠습니까?",
            action.label(),
            service.unit,
            service.active_state,
            service.sub_state
        ),
        keyboard: InlineKeyboardMarkup {
            inline_keyboard: vec![vec![
                button("승인하고 실행", &format!("action:confirm:{token}")),
                button("취소", &format!("action:cancel:{token}")),
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
    let outcome = if success { "성공" } else { "실패" };
    MenuView {
        text: format!(
            "서비스 {} {outcome}\n대상: {}\n현재 상태: {}\nActiveState: {}\nSubState: {}",
            action.label(),
            service.unit,
            service.state_label(),
            service.active_state,
            service.sub_state
        ),
        keyboard: InlineKeyboardMarkup {
            inline_keyboard: vec![vec![
                button(
                    "서비스 상세",
                    &format!("service:{}", crate::services::service_key(&service.unit)),
                ),
                button("목록", "menu:services"),
            ]],
        },
    }
}

/// 취소 또는 만료 결과입니다.
#[must_use]
pub fn render_action_cancelled(message: &str) -> MenuView {
    MenuView {
        text: message.to_owned(),
        keyboard: InlineKeyboardMarkup {
            inline_keyboard: vec![vec![button("서비스 목록", "menu:services")]],
        },
    }
}

fn format_system_snapshot(snapshot: &SystemSnapshot) -> String {
    let memory_percent = percent(snapshot.memory_used_bytes, snapshot.memory_total_bytes);
    let swap_percent = percent(snapshot.swap_used_bytes, snapshot.swap_total_bytes);
    let mut lines = vec![
        format!("서버 상태 — {}", snapshot.server_name),
        format!("호스트: {}", snapshot.hostname),
        format!("OS: {}", snapshot.os_name),
        format!("Kernel: {}", snapshot.kernel_version),
        format!("Uptime: {}", format_uptime(snapshot.uptime_seconds)),
        format!(
            "CPU: {:.1}% · Load: {:.2}",
            snapshot.cpu_usage_percent, snapshot.load_one
        ),
        format!(
            "메모리: {} / {} ({memory_percent:.1}%)",
            format_bytes(snapshot.memory_used_bytes),
            format_bytes(snapshot.memory_total_bytes)
        ),
        format!(
            "Swap: {} / {} ({swap_percent:.1}%)",
            format_bytes(snapshot.swap_used_bytes),
            format_bytes(snapshot.swap_total_bytes)
        ),
    ];
    for disk in &snapshot.disks {
        let used = disk.total_bytes.saturating_sub(disk.available_bytes);
        lines.push(format!(
            "디스크 {}: {} / {} ({:.1}%)",
            disk.mount_point,
            format_bytes(used),
            format_bytes(disk.total_bytes),
            percent(used, disk.total_bytes)
        ));
    }
    lines.join("\n")
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
            button("새로고침", callback),
            button("뒤로가기", "menu:main"),
        ]],
    }
}

fn back_only() -> InlineKeyboardMarkup {
    InlineKeyboardMarkup {
        inline_keyboard: vec![vec![button("뒤로가기", "menu:main")]],
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

fn format_bytes(bytes: u64) -> String {
    const GIB: f64 = 1024.0 * 1024.0 * 1024.0;
    const MIB: f64 = 1024.0 * 1024.0;
    if bytes as f64 >= GIB {
        format!("{:.1} GiB", bytes as f64 / GIB)
    } else {
        format!("{:.1} MiB", bytes as f64 / MIB)
    }
}

fn format_uptime(seconds: u64) -> String {
    let days = seconds / 86_400;
    let hours = seconds % 86_400 / 3_600;
    let minutes = seconds % 3_600 / 60;
    format!("{days}일 {hours}시간 {minutes}분")
}

#[cfg(test)]
mod tests {
    use g7tg_core::{DiskSnapshot, Menu, SystemSnapshot};

    use super::render;

    #[test]
    fn system_menu_has_refresh_and_back() {
        let snapshot = SystemSnapshot {
            server_name: "demo".to_owned(),
            hostname: "demo-host".to_owned(),
            os_name: "Ubuntu".to_owned(),
            kernel_version: "6.8".to_owned(),
            uptime_seconds: 90_061,
            cpu_usage_percent: 12.5,
            load_one: 0.2,
            memory_total_bytes: 2 * 1024 * 1024 * 1024,
            memory_used_bytes: 1024 * 1024 * 1024,
            swap_total_bytes: 0,
            swap_used_bytes: 0,
            disks: vec![DiskSnapshot {
                mount_point: "/".to_owned(),
                total_bytes: 10 * 1024 * 1024 * 1024,
                available_bytes: 5 * 1024 * 1024 * 1024,
            }],
        };
        let view = render(Menu::System, Some(&snapshot));
        assert!(view.text.contains("메모리: 1.0 GiB / 2.0 GiB (50.0%)"));
        assert_eq!(view.keyboard.inline_keyboard[0].len(), 2);
    }
}
