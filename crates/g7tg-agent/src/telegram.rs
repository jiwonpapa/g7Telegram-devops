//! Telegram Bot API의 필요한 최소 transport입니다.

use std::{fs, path::Path, time::Duration};

use anyhow::{Context, anyhow, ensure};
use reqwest::Client;
use secrecy::{ExposeSecret, SecretString};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::Value;

/// Telegram Bot API client입니다.
#[derive(Clone)]
pub struct TelegramClient {
    client: Client,
    token: SecretString,
}

impl TelegramClient {
    /// root 전용 파일에서 token을 읽습니다.
    pub fn from_token_file(path: impl AsRef<Path>) -> anyhow::Result<Self> {
        let token = fs::read_to_string(path.as_ref()).context("Bot token 파일 read 실패")?;
        let token = token.trim().to_owned();
        validate_token_shape(&token)?;
        let client = Client::builder()
            .user_agent(concat!("g7telegram-devops/", env!("CARGO_PKG_VERSION")))
            .build()
            .context("Telegram HTTP client 생성 실패")?;
        Ok(Self {
            client,
            token: SecretString::from(token),
        })
    }

    /// Bot token과 계정을 확인합니다.
    pub async fn get_me(&self) -> anyhow::Result<TelegramUser> {
        self.post::<_, TelegramUser>("getMe", &EmptyRequest, 10)
            .await
    }

    /// webhook을 제거해 전용 long polling을 보장합니다.
    pub async fn delete_webhook(&self) -> anyhow::Result<()> {
        let _: bool = self
            .post(
                "deleteWebhook",
                &DeleteWebhookRequest {
                    drop_pending_updates: false,
                },
                10,
            )
            .await?;
        Ok(())
    }

    /// long polling으로 다음 update 묶음을 받습니다.
    pub async fn get_updates(&self, offset: i64, timeout: u64) -> anyhow::Result<Vec<Update>> {
        self.post(
            "getUpdates",
            &GetUpdatesRequest {
                offset,
                timeout,
                allowed_updates: ["message", "callback_query"],
            },
            timeout.saturating_add(10),
        )
        .await
    }

    /// 새 메시지를 보냅니다.
    pub async fn send_message(
        &self,
        chat_id: i64,
        text: &str,
        reply_markup: Option<Value>,
    ) -> anyhow::Result<Message> {
        self.post(
            "sendMessage",
            &SendMessageRequest {
                chat_id,
                text,
                reply_markup,
            },
            10,
        )
        .await
    }

    /// 기존 inline 메뉴 메시지를 교체합니다.
    pub async fn edit_message(
        &self,
        chat_id: i64,
        message_id: i64,
        text: &str,
        reply_markup: InlineKeyboardMarkup,
    ) -> anyhow::Result<Message> {
        self.post(
            "editMessageText",
            &EditMessageRequest {
                chat_id,
                message_id,
                text,
                reply_markup,
            },
            10,
        )
        .await
    }

    /// callback 진행 표시를 끝내고 짧은 결과를 표시합니다.
    pub async fn answer_callback(&self, callback_id: &str, text: &str) -> anyhow::Result<()> {
        let _: bool = self
            .post(
                "answerCallbackQuery",
                &AnswerCallbackRequest {
                    callback_query_id: callback_id,
                    text,
                },
                10,
            )
            .await?;
        Ok(())
    }

    async fn post<Request, Response>(
        &self,
        method: &str,
        request: &Request,
        timeout_seconds: u64,
    ) -> anyhow::Result<Response>
    where
        Request: Serialize + ?Sized,
        Response: DeserializeOwned,
    {
        let url = format!(
            "https://api.telegram.org/bot{}/{method}",
            self.token.expose_secret()
        );
        let response = self
            .client
            .post(url)
            .timeout(Duration::from_secs(timeout_seconds))
            .json(request)
            .send()
            .await
            .map_err(|_| anyhow!("Telegram {method} 전송 실패"))?;
        let status = response.status();
        let envelope = response
            .json::<ApiResponse<Response>>()
            .await
            .map_err(|_| anyhow!("Telegram {method} 응답 parse 실패"))?;
        if !status.is_success() || !envelope.ok {
            return Err(anyhow!(
                "Telegram {method} 거부: status={}, description={}",
                status.as_u16(),
                envelope.description.unwrap_or_else(|| "unknown".to_owned())
            ));
        }
        envelope
            .result
            .ok_or_else(|| anyhow!("Telegram {method} result 누락"))
    }
}

/// Telegram update입니다.
#[derive(Debug, Clone, Deserialize)]
pub struct Update {
    /// 순서와 중복 제거에 사용하는 ID입니다.
    pub update_id: i64,
    /// 일반 메시지 update입니다.
    pub message: Option<Message>,
    /// inline keyboard callback입니다.
    pub callback_query: Option<CallbackQuery>,
}

/// Telegram 메시지의 필요한 최소 필드입니다.
#[derive(Debug, Clone, Deserialize)]
pub struct Message {
    /// chat 내부 message ID입니다.
    pub message_id: i64,
    /// 발신자입니다.
    pub from: Option<TelegramUser>,
    /// 메시지가 속한 chat입니다.
    pub chat: Chat,
    /// text 메시지일 때의 본문입니다.
    pub text: Option<String>,
}

/// Telegram 사용자입니다.
#[derive(Debug, Clone, Deserialize)]
pub struct TelegramUser {
    /// 변하지 않는 숫자 ID입니다.
    pub id: i64,
    /// bot 계정 여부입니다.
    pub is_bot: bool,
    /// 표시 이름입니다.
    pub first_name: String,
}

/// Telegram chat입니다.
#[derive(Debug, Clone, Deserialize)]
pub struct Chat {
    /// chat ID입니다.
    pub id: i64,
    /// private, group 등 Telegram chat type입니다.
    #[serde(rename = "type")]
    pub kind: String,
}

/// inline keyboard callback입니다.
#[derive(Debug, Clone, Deserialize)]
pub struct CallbackQuery {
    /// callback 응답용 ID입니다.
    pub id: String,
    /// 버튼을 누른 사용자입니다.
    pub from: TelegramUser,
    /// 원본 bot 메시지입니다.
    pub message: Option<Message>,
    /// Agent가 발급한 callback payload입니다.
    pub data: Option<String>,
}

/// Telegram inline keyboard입니다.
#[derive(Debug, Clone, Serialize)]
pub struct InlineKeyboardMarkup {
    /// 행 단위 버튼입니다.
    pub inline_keyboard: Vec<Vec<InlineKeyboardButton>>,
}

/// Telegram inline keyboard 버튼입니다.
#[derive(Debug, Clone, Serialize)]
pub struct InlineKeyboardButton {
    /// 사용자에게 보이는 label입니다.
    pub text: String,
    /// Agent가 해석하는 callback payload입니다.
    pub callback_data: String,
}

impl InlineKeyboardButton {
    /// callback 버튼을 만듭니다.
    #[must_use]
    pub fn callback(text: impl Into<String>, data: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            callback_data: data.into(),
        }
    }
}

#[derive(Debug, Deserialize)]
struct ApiResponse<T> {
    ok: bool,
    result: Option<T>,
    description: Option<String>,
}

#[derive(Debug, Serialize)]
struct EmptyRequest;

#[derive(Debug, Serialize)]
struct DeleteWebhookRequest {
    drop_pending_updates: bool,
}

#[derive(Debug, Serialize)]
struct GetUpdatesRequest<'a> {
    offset: i64,
    timeout: u64,
    allowed_updates: [&'a str; 2],
}

#[derive(Debug, Serialize)]
struct SendMessageRequest<'a> {
    chat_id: i64,
    text: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    reply_markup: Option<Value>,
}

#[derive(Debug, Serialize)]
struct EditMessageRequest<'a> {
    chat_id: i64,
    message_id: i64,
    text: &'a str,
    reply_markup: InlineKeyboardMarkup,
}

#[derive(Debug, Serialize)]
struct AnswerCallbackRequest<'a> {
    callback_query_id: &'a str,
    text: &'a str,
}

fn validate_token_shape(token: &str) -> anyhow::Result<()> {
    ensure!(
        (20..=256).contains(&token.len()),
        "Bot token 형식이 올바르지 않습니다"
    );
    let (bot_id, secret) = token
        .split_once(':')
        .ok_or_else(|| anyhow!("Bot token 형식이 올바르지 않습니다"))?;
    ensure!(
        !bot_id.is_empty() && bot_id.chars().all(|character| character.is_ascii_digit()),
        "Bot token 형식이 올바르지 않습니다"
    );
    ensure!(
        !secret.is_empty()
            && secret.chars().all(|character| {
                character.is_ascii_alphanumeric() || matches!(character, '_' | '-')
            }),
        "Bot token 형식이 올바르지 않습니다"
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::validate_token_shape;

    #[test]
    fn bot_token_shape_is_fail_closed() {
        assert!(validate_token_shape("123456789:ABCdef_123456789-xyz").is_ok());
        assert!(validate_token_shape("not-a-token").is_err());
        assert!(validate_token_shape("123:../../secret").is_err());
    }
}
