//! Telegram update loop와 메뉴 상태 처리입니다.

use std::time::Duration;

use anyhow::{Context, anyhow};
use g7tg_core::Menu;
use tokio::task;

use crate::{
    config::AgentConfig,
    menu, services,
    storage::{Owner, Store},
    system,
    telegram::{CallbackQuery, Message, TelegramClient, Update},
};

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
    loop {
        let updates = tokio::select! {
            signal = tokio::signal::ctrl_c() => {
                signal.context("종료 신호 처리 실패")?;
                tracing::info!("종료 신호를 받았습니다");
                return Ok(());
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
                    if let Err(error) = handle_update(&config, &store, &telegram, update).await {
                        tracing::warn!(error = %error, "Telegram update 처리 실패");
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
    let owner = store.owner()?;
    if owner.is_none() {
        if store.consume_pairing_code(text, user.id, message.chat.id)? {
            telegram
                .send_message(
                    message.chat.id,
                    "연결되었습니다. 아래 메뉴 버튼으로 서버를 관리하세요.",
                    Some(menu::persistent_menu_keyboard()),
                )
                .await?;
            send_new_menu(config, telegram, message.chat.id, Menu::Main).await?;
        }
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
        send_new_menu(config, telegram, owner.chat_id, Menu::Main).await?;
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
        let view = menu::render_service_detail(service);
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
    let view = if target_menu == Menu::Services {
        let inventory = services::discover(&config.extra_service_units).await?;
        menu::render_services(&inventory)
    } else {
        menu::render(target_menu, snapshot.as_ref())
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

async fn send_new_menu(
    config: &AgentConfig,
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
    let view = if target_menu == Menu::Services {
        let inventory = services::discover(&config.extra_service_units).await?;
        menu::render_services(&inventory)
    } else {
        menu::render(target_menu, snapshot.as_ref())
    };
    telegram
        .send_message(
            chat_id,
            &view.text,
            Some(serde_json::to_value(view.keyboard).context("메뉴 JSON 생성 실패")?),
        )
        .await?;
    Ok(())
}

fn is_owner(owner: Owner, user_id: i64, chat_id: i64) -> bool {
    owner.user_id == user_id && owner.chat_id == chat_id
}
