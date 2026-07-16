//! Telegram 메뉴 구성과 읽기 전용 화면 rendering입니다.

use g7tg_core::{Menu, SystemSnapshot};
use serde_json::{Value, json};

use crate::telegram::{InlineKeyboardButton, InlineKeyboardMarkup};

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
