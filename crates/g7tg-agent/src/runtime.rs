//! Telegram update loop와 메뉴 상태 처리입니다.

use std::time::Duration;

use anyhow::{Context, anyhow};
use g7tg_core::Menu;
use tokio::task;

use crate::{
    actions,
    config::AgentConfig,
    menu, monitor, services,
    storage::{Owner, Store},
    system,
    telegram::{CallbackQuery, Message, TelegramClient, Update},
};

const UPDATE_MAX_ATTEMPTS: u32 = 3;

/// 설정을 검증하고 종료 신호까지 Agent를 실행합니다.
pub async fn run(config: AgentConfig) -> anyhow::Result<()> {
    let store = Store::open(&config.state_database)?;
    let telegram = TelegramClient::from_token_file(&config.bot_token_file)?;
    let me = telegram.get_me().await?;
    telegram.delete_webhook().await?;
    tracing::info!(
        server = %config.server_name,
        bot_id = me.id,
        bot_name = %me.first_name,
        paired = store.owner()?.is_some(),
        "Telegram Agent를 시작합니다"
    );

    let mut offset = store.update_offset()?;
    let mut retry_seconds = config.retry_seconds;
    let mut monitor_interval =
        tokio::time::interval(Duration::from_secs(config.monitor_interval_seconds));
    monitor_interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        let updates = tokio::select! {
            signal = tokio::signal::ctrl_c() => {
                signal.context("종료 신호 처리 실패")?;
                tracing::info!("종료 신호를 받았습니다");
                return Ok(());
            }
            _ = monitor_interval.tick() => {
                if let Err(error) = monitor::cycle(&config, &store, &telegram).await {
                    tracing::warn!(error = %error, "monitoring cycle 실패");
                }
                continue;
            }
            result = telegram.get_updates(offset, config.poll_timeout_seconds) => result,
        };

        match updates {
            Ok(updates) => {
                retry_seconds = config.retry_seconds;
                for update in updates {
                    let next_offset = update
                        .update_id
                        .checked_add(1)
                        .ok_or_else(|| anyhow!("Telegram update_id overflow"))?;
                    if let Err(error) =
                        handle_update_with_retry(&config, &store, &telegram, update).await
                    {
                        tracing::error!(error = %error, "Telegram update 실패기록 저장 실패");
                        tokio::time::sleep(Duration::from_secs(config.retry_seconds)).await;
                        break;
                    }
                    store.set_update_offset(next_offset)?;
                    offset = next_offset;
                }
            }
            Err(error) => {
                tracing::warn!(error = %error, retry_seconds, "Telegram polling 실패");
                tokio::time::sleep(Duration::from_secs(retry_seconds)).await;
                retry_seconds = retry_seconds.saturating_mul(2).min(30);
            }
        }
    }
}

async fn handle_update_with_retry(
    config: &AgentConfig,
    store: &Store,
    telegram: &TelegramClient,
    update: Update,
) -> anyhow::Result<()> {
    for attempt in 1..=UPDATE_MAX_ATTEMPTS {
        match handle_update(config, store, telegram, update.clone()).await {
            Ok(()) => return Ok(()),
            Err(error) if attempt < UPDATE_MAX_ATTEMPTS => {
                let delay_seconds = update_retry_delay_seconds(config.retry_seconds, attempt);
                tracing::warn!(
                    update_id = update.update_id,
                    attempt,
                    delay_seconds,
                    error = %error,
                    "Telegram update 재시도"
                );
                tokio::time::sleep(Duration::from_secs(delay_seconds)).await;
            }
            Err(error) => {
                let detail = bounded_detail(&format!(
                    "update_id={};attempts={UPDATE_MAX_ATTEMPTS};error={error}",
                    update.update_id
                ));
                store
                    .audit(None, "telegram_update_dead_letter", "failed", &detail)
                    .context("Telegram update 실패기록 저장 실패")?;
                tracing::error!(
                    update_id = update.update_id,
                    attempts = UPDATE_MAX_ATTEMPTS,
                    error = %error,
                    "Telegram update를 실패기록으로 이동했습니다"
                );
                return Ok(());
            }
        }
    }
    Err(anyhow!("Telegram update 재시도 상태가 올바르지 않습니다"))
}

fn update_retry_delay_seconds(base_seconds: u64, attempt: u32) -> u64 {
    base_seconds
        .saturating_mul(1_u64 << attempt.saturating_sub(1).min(4))
        .min(30)
}

fn bounded_detail(detail: &str) -> String {
    detail.chars().take(500).collect()
}

async fn handle_update(
    config: &AgentConfig,
    store: &Store,
    telegram: &TelegramClient,
    update: Update,
) -> anyhow::Result<()> {
    if let Some(message) = update.message {
        return handle_message(config, store, telegram, message).await;
    }
    if let Some(callback) = update.callback_query {
        return handle_callback(config, store, telegram, callback).await;
    }
    Ok(())
}

async fn handle_message(
    config: &AgentConfig,
    store: &Store,
    telegram: &TelegramClient,
    message: Message,
) -> anyhow::Result<()> {
    let Some(user) = message.from.as_ref() else {
        return Ok(());
    };
    if user.is_bot || message.chat.kind != "private" {
        store.audit(
            Some(user.id),
            "message_rejected",
            "denied",
            "non_private_or_bot",
        )?;
        return Ok(());
    }
    let text = message.text.as_deref().unwrap_or_default().trim();
    if store.consume_pairing_code(text, user.id, message.chat.id)? {
        telegram
            .send_message(
                message.chat.id,
                "연결되었습니다. 아래 메뉴 버튼으로 서버를 관리하세요.",
                Some(menu::persistent_menu_keyboard()),
            )
            .await?;
        send_new_menu(config, store, telegram, message.chat.id, Menu::Main).await?;
        return Ok(());
    }
    let owner = store.owner()?;
    if owner.is_none() {
        return Ok(());
    }
    let owner = owner.ok_or_else(|| anyhow!("owner 상태가 사라졌습니다"))?;
    if !is_owner(owner, user.id, message.chat.id) {
        store.audit(Some(user.id), "message_rejected", "denied", "not_owner")?;
        return Ok(());
    }

    if matches!(text, "메뉴" | "시작" | "/start") {
        if matches!(text, "시작" | "/start") {
            telegram
                .send_message(
                    owner.chat_id,
                    "아래 메뉴 버튼을 사용하세요.",
                    Some(menu::persistent_menu_keyboard()),
                )
                .await?;
        }
        send_new_menu(config, store, telegram, owner.chat_id, Menu::Main).await?;
    } else {
        telegram
            .send_message(
                owner.chat_id,
                "지원하지 않는 입력입니다. 아래의 메뉴 버튼을 선택하세요.",
                Some(menu::persistent_menu_keyboard()),
            )
            .await?;
    }
    Ok(())
}

async fn handle_callback(
    config: &AgentConfig,
    store: &Store,
    telegram: &TelegramClient,
    callback: CallbackQuery,
) -> anyhow::Result<()> {
    let Some(message) = callback.message.as_ref() else {
        telegram
            .answer_callback(&callback.id, "처리할 수 없는 메뉴입니다.")
            .await?;
        return Ok(());
    };
    let Some(owner) = store.owner()? else {
        telegram
            .answer_callback(&callback.id, "먼저 서버와 연결하세요.")
            .await?;
        return Ok(());
    };
    if !is_owner(owner, callback.from.id, message.chat.id) || message.chat.kind != "private" {
        store.audit(
            Some(callback.from.id),
            "callback_rejected",
            "denied",
            "not_owner",
        )?;
        telegram
            .answer_callback(&callback.id, "권한이 없습니다.")
            .await?;
        return Ok(());
    }
    let data = callback.data.as_deref().unwrap_or_default();
    if let Some(value) = data.strip_prefix("silence:") {
        match value {
            "3600" => {
                let _ = store.set_silence(3600)?;
            }
            "21600" => {
                let _ = store.set_silence(21_600)?;
            }
            "clear" => store.clear_silence()?,
            _ => {
                telegram
                    .answer_callback(&callback.id, "잘못된 알림중지 요청입니다.")
                    .await?;
                return Ok(());
            }
        }
        let view = menu::render_alerts(&store.current_incidents()?, store.silence_until()?);
        telegram
            .edit_message(
                message.chat.id,
                message.message_id,
                &view.text,
                view.keyboard,
            )
            .await?;
        telegram.answer_callback(&callback.id, "적용됨").await?;
        return Ok(());
    }
    if let Some(rest) = data.strip_prefix("action:plan:") {
        return handle_action_plan(config, store, telegram, &callback, message, rest).await;
    }
    if let Some(token) = data.strip_prefix("action:confirm:") {
        return handle_action_confirm(config, store, telegram, &callback, message, token).await;
    }
    if let Some(token) = data.strip_prefix("action:cancel:") {
        let cancelled = store.cancel_approval(token, callback.from.id)?;
        let text = if cancelled {
            "서비스 재시작을 취소했습니다."
        } else {
            "이미 사용했거나 만료된 승인입니다."
        };
        let view = menu::render_action_cancelled(text);
        telegram
            .edit_message(
                message.chat.id,
                message.message_id,
                &view.text,
                view.keyboard,
            )
            .await?;
        telegram.answer_callback(&callback.id, "취소됨").await?;
        return Ok(());
    }
    if let Some(service_key) = data.strip_prefix("service:") {
        let inventory = services::discover(&config.extra_service_units).await?;
        let matches: Vec<_> = inventory
            .iter()
            .filter(|service| services::service_key(&service.unit) == service_key)
            .collect();
        let [service] = matches.as_slice() else {
            telegram
                .answer_callback(&callback.id, "서비스가 사라졌거나 메뉴가 만료되었습니다.")
                .await?;
            return Ok(());
        };
        let action_allowed = config.service_actions_enabled
            && actions::can_manage(&config.action_executor, &service.unit).await?;
        let view = menu::render_service_detail(service, action_allowed);
        telegram
            .edit_message(
                message.chat.id,
                message.message_id,
                &view.text,
                view.keyboard,
            )
            .await?;
        telegram.answer_callback(&callback.id, "완료").await?;
        return Ok(());
    }
    let Some(target_menu) = Menu::from_callback(data) else {
        telegram
            .answer_callback(&callback.id, "만료되었거나 잘못된 메뉴입니다.")
            .await?;
        return Ok(());
    };
    let snapshot = if target_menu == Menu::System {
        let server_name = config.server_name.clone();
        Some(
            task::spawn_blocking(move || system::collect(&server_name))
                .await
                .context("시스템 collector task 실패")?,
        )
    } else {
        None
    };
    let view = match target_menu {
        Menu::Services => {
            let inventory = services::discover(&config.extra_service_units).await?;
            menu::render_services(&inventory)
        }
        Menu::Web => {
            let results = crate::web::check_all(&config.web_checks).await?;
            menu::render_web_checks(&results)
        }
        Menu::Alerts => menu::render_alerts(&store.current_incidents()?, store.silence_until()?),
        _ => menu::render(target_menu, snapshot.as_ref()),
    };
    telegram
        .edit_message(
            message.chat.id,
            message.message_id,
            &view.text,
            view.keyboard,
        )
        .await?;
    telegram.answer_callback(&callback.id, "완료").await?;
    Ok(())
}

async fn handle_action_plan(
    config: &AgentConfig,
    store: &Store,
    telegram: &TelegramClient,
    callback: &CallbackQuery,
    message: &Message,
    payload: &str,
) -> anyhow::Result<()> {
    if !config.service_actions_enabled {
        telegram
            .answer_callback(&callback.id, "서비스 작업이 비활성화되어 있습니다.")
            .await?;
        return Ok(());
    }
    let Some((action_id, service_key)) = payload.split_once(':') else {
        telegram
            .answer_callback(&callback.id, "잘못된 재시작 요청입니다.")
            .await?;
        return Ok(());
    };
    let action = match action_id {
        "restart" => g7tg_core::ServiceAction::Restart,
        _ => {
            telegram
                .answer_callback(&callback.id, "허용되지 않은 동작입니다.")
                .await?;
            return Ok(());
        }
    };
    let inventory = services::discover(&config.extra_service_units).await?;
    let matches: Vec<_> = inventory
        .iter()
        .filter(|service| services::service_key(&service.unit) == service_key)
        .collect();
    let [service] = matches.as_slice() else {
        telegram
            .answer_callback(&callback.id, "서비스가 사라졌습니다.")
            .await?;
        return Ok(());
    };
    if !actions::can_manage(&config.action_executor, &service.unit).await? {
        telegram
            .answer_callback(&callback.id, "root allowlist에 없는 서비스입니다.")
            .await?;
        return Ok(());
    }
    let token = store.create_approval(
        callback.from.id,
        action,
        &service.unit,
        config.approval_ttl_seconds,
    )?;
    let view =
        menu::render_action_confirmation(service, action, &token, config.approval_ttl_seconds);
    telegram
        .edit_message(
            message.chat.id,
            message.message_id,
            &view.text,
            view.keyboard,
        )
        .await?;
    telegram
        .answer_callback(&callback.id, "재승인이 필요합니다.")
        .await?;
    Ok(())
}

async fn handle_action_confirm(
    config: &AgentConfig,
    store: &Store,
    telegram: &TelegramClient,
    callback: &CallbackQuery,
    message: &Message,
    token: &str,
) -> anyhow::Result<()> {
    let Some(approval) = store.consume_approval(token, callback.from.id)? else {
        let view = menu::render_action_cancelled("이미 사용했거나 만료된 승인입니다.");
        telegram
            .edit_message(
                message.chat.id,
                message.message_id,
                &view.text,
                view.keyboard,
            )
            .await?;
        telegram.answer_callback(&callback.id, "승인 만료").await?;
        return Ok(());
    };
    telegram.answer_callback(&callback.id, "실행 중").await?;
    let inventory = services::discover(&config.extra_service_units).await?;
    let Some(before) = inventory
        .iter()
        .find(|service| service.unit == approval.unit)
    else {
        store.audit(
            Some(approval.actor_user_id),
            "service_action",
            "denied",
            "unit_not_discovered",
        )?;
        return Err(anyhow!("승인한 서비스가 더 이상 발견되지 않습니다"));
    };
    let category = before.category;
    let execution =
        actions::execute(&config.action_executor, approval.action, &approval.unit).await;
    let latest = services::refresh_status(&approval.unit, category).await?;
    let success = execution.is_ok() && latest.is_healthy();
    store.audit(
        Some(approval.actor_user_id),
        "service_action",
        if success { "success" } else { "failed" },
        &format!("{}:{}", approval.action.id(), approval.unit),
    )?;
    let _ = store.prune_approvals()?;
    let view = menu::render_action_result(&latest, approval.action, success);
    telegram
        .edit_message(
            message.chat.id,
            message.message_id,
            &view.text,
            view.keyboard,
        )
        .await?;
    if let Err(error) = execution {
        tracing::warn!(unit = %approval.unit, error = %error, "서비스 action 실패");
    }
    Ok(())
}

async fn send_new_menu(
    config: &AgentConfig,
    store: &Store,
    telegram: &TelegramClient,
    chat_id: i64,
    target_menu: Menu,
) -> anyhow::Result<()> {
    let snapshot = if target_menu == Menu::System {
        let server_name = config.server_name.clone();
        Some(
            task::spawn_blocking(move || system::collect(&server_name))
                .await
                .context("시스템 collector task 실패")?,
        )
    } else {
        None
    };
    let view = match target_menu {
        Menu::Services => {
            let inventory = services::discover(&config.extra_service_units).await?;
            menu::render_services(&inventory)
        }
        Menu::Web => {
            let results = crate::web::check_all(&config.web_checks).await?;
            menu::render_web_checks(&results)
        }
        Menu::Alerts => menu::render_alerts(&store.current_incidents()?, store.silence_until()?),
        _ => menu::render(target_menu, snapshot.as_ref()),
    };
    let keyboard = Some(serde_json::to_value(view.keyboard).context("메뉴 JSON 생성 실패")?);
    telegram.send_message(chat_id, &view.text, keyboard).await?;
    Ok(())
}

fn is_owner(owner: Owner, user_id: i64, chat_id: i64) -> bool {
    owner.user_id == user_id && owner.chat_id == chat_id
}

#[cfg(test)]
mod tests {
    use super::{bounded_detail, update_retry_delay_seconds};

    #[test]
    fn update_retry_delay_is_bounded() {
        assert_eq!(update_retry_delay_seconds(2, 1), 2);
        assert_eq!(update_retry_delay_seconds(2, 2), 4);
        assert_eq!(update_retry_delay_seconds(20, 3), 30);
    }

    #[test]
    fn dead_letter_detail_is_bounded_by_characters() {
        let detail = bounded_detail(&"가".repeat(700));
        assert_eq!(detail.chars().count(), 500);
    }
}
