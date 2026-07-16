//! SQLite 기반 최소 상태와 감사 저장소입니다.

use std::{
    fs,
    path::Path,
    sync::{Arc, Mutex, MutexGuard},
    time::Duration,
};

use anyhow::{Context, anyhow};
use rusqlite::{Connection, OptionalExtension, params};
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;
use time::OffsetDateTime;
use uuid::Uuid;

/// 등록된 Telegram owner입니다.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Owner {
    /// Telegram user ID입니다.
    pub user_id: i64,
    /// 1:1 private chat ID입니다.
    pub chat_id: i64,
}

/// 여러 async 작업에서 짧게 공유하는 SQLite 저장소입니다.
#[derive(Clone)]
pub struct Store {
    connection: Arc<Mutex<Connection>>,
}

impl Store {
    /// DB를 열고 schema와 WAL을 준비합니다.
    pub fn open(path: impl AsRef<Path>) -> anyhow::Result<Self> {
        let path = path.as_ref();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).context("상태 DB 디렉터리 생성 실패")?;
        }
        let connection = Connection::open(path).context("상태 DB open 실패")?;
        connection
            .busy_timeout(Duration::from_secs(5))
            .context("SQLite busy timeout 설정 실패")?;
        connection
            .pragma_update(None, "journal_mode", "WAL")
            .context("SQLite WAL 설정 실패")?;
        connection
            .execute_batch(
                r#"
                PRAGMA foreign_keys = ON;
                CREATE TABLE IF NOT EXISTS metadata (
                    key TEXT PRIMARY KEY,
                    value TEXT NOT NULL
                );
                CREATE TABLE IF NOT EXISTS pairing (
                    singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
                    code_hash BLOB NOT NULL,
                    expires_at INTEGER NOT NULL
                );
                CREATE TABLE IF NOT EXISTS audit_events (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    occurred_at INTEGER NOT NULL,
                    actor_user_id INTEGER,
                    event_kind TEXT NOT NULL,
                    outcome TEXT NOT NULL,
                    detail TEXT NOT NULL
                );
                CREATE INDEX IF NOT EXISTS audit_events_occurred_at
                    ON audit_events(occurred_at);
                "#,
            )
            .context("SQLite schema 준비 실패")?;
        Ok(Self {
            connection: Arc::new(Mutex::new(connection)),
        })
    }

    /// 마지막으로 처리 완료한 다음 update offset입니다.
    pub fn update_offset(&self) -> anyhow::Result<i64> {
        self.metadata_i64("telegram_update_offset")
            .map(|value| value.unwrap_or(0))
    }

    /// 처리 완료한 다음 update offset을 원자 저장합니다.
    pub fn set_update_offset(&self, offset: i64) -> anyhow::Result<()> {
        self.set_metadata("telegram_update_offset", &offset.to_string())
    }

    /// 현재 owner를 반환합니다.
    pub fn owner(&self) -> anyhow::Result<Option<Owner>> {
        let user_id = self.metadata_i64("owner_user_id")?;
        let chat_id = self.metadata_i64("owner_chat_id")?;
        match (user_id, chat_id) {
            (Some(user_id), Some(chat_id)) => Ok(Some(Owner { user_id, chat_id })),
            (None, None) => Ok(None),
            _ => Err(anyhow!("owner metadata가 불완전합니다")),
        }
    }

    /// 기존 연결 코드를 폐기하고 새 단회 code를 반환합니다.
    pub fn create_pairing_code(&self, ttl_seconds: u64) -> anyhow::Result<String> {
        let code: String = Uuid::new_v4()
            .simple()
            .to_string()
            .chars()
            .take(8)
            .map(|character| character.to_ascii_uppercase())
            .collect();
        let code_hash = hash_code(&code);
        let expires_at = now_unix()
            .checked_add(i64::try_from(ttl_seconds).context("pairing TTL 변환 실패")?)
            .ok_or_else(|| anyhow!("pairing 만료시각 overflow"))?;
        let connection = self.lock()?;
        connection
            .execute(
                "INSERT INTO pairing(singleton, code_hash, expires_at) VALUES(1, ?1, ?2)\
                 ON CONFLICT(singleton) DO UPDATE SET code_hash=excluded.code_hash, expires_at=excluded.expires_at",
                params![code_hash.as_slice(), expires_at],
            )
            .context("pairing code 저장 실패")?;
        Ok(code)
    }

    /// 올바른 단회 code면 owner를 등록하고 code를 소비합니다.
    pub fn consume_pairing_code(
        &self,
        code: &str,
        user_id: i64,
        chat_id: i64,
    ) -> anyhow::Result<bool> {
        let mut connection = self.lock()?;
        let transaction = connection
            .transaction()
            .context("pairing transaction 시작 실패")?;
        let stored = transaction
            .query_row(
                "SELECT code_hash, expires_at FROM pairing WHERE singleton=1",
                [],
                |row| Ok((row.get::<_, Vec<u8>>(0)?, row.get::<_, i64>(1)?)),
            )
            .optional()
            .context("pairing code 조회 실패")?;
        let Some((stored_hash, expires_at)) = stored else {
            return Ok(false);
        };
        let candidate = hash_code(code.trim());
        let hash_matches: bool = stored_hash.as_slice().ct_eq(candidate.as_slice()).into();
        if expires_at < now_unix() || !hash_matches {
            return Ok(false);
        }

        set_metadata_on(&transaction, "owner_user_id", &user_id.to_string())?;
        set_metadata_on(&transaction, "owner_chat_id", &chat_id.to_string())?;
        transaction
            .execute("DELETE FROM pairing WHERE singleton=1", [])
            .context("pairing code 소비 실패")?;
        insert_audit_on(
            &transaction,
            Some(user_id),
            "owner_paired",
            "success",
            "private_chat",
        )?;
        transaction
            .commit()
            .context("pairing transaction commit 실패")?;
        Ok(true)
    }

    /// 보안과 운영 이벤트를 기록합니다.
    pub fn audit(
        &self,
        actor_user_id: Option<i64>,
        event_kind: &str,
        outcome: &str,
        detail: &str,
    ) -> anyhow::Result<()> {
        let connection = self.lock()?;
        insert_audit_on(&connection, actor_user_id, event_kind, outcome, detail)
    }

    fn metadata_i64(&self, key: &str) -> anyhow::Result<Option<i64>> {
        let connection = self.lock()?;
        let value = connection
            .query_row("SELECT value FROM metadata WHERE key=?1", [key], |row| {
                row.get::<_, String>(0)
            })
            .optional()
            .context("metadata 조회 실패")?;
        value
            .map(|value| value.parse::<i64>().context("metadata 정수 변환 실패"))
            .transpose()
    }

    fn set_metadata(&self, key: &str, value: &str) -> anyhow::Result<()> {
        let connection = self.lock()?;
        set_metadata_on(&connection, key, value)
    }

    fn lock(&self) -> anyhow::Result<MutexGuard<'_, Connection>> {
        self.connection
            .lock()
            .map_err(|_| anyhow!("SQLite mutex가 오염되었습니다"))
    }
}

fn set_metadata_on(connection: &Connection, key: &str, value: &str) -> anyhow::Result<()> {
    connection
        .execute(
            "INSERT INTO metadata(key, value) VALUES(?1, ?2)\
             ON CONFLICT(key) DO UPDATE SET value=excluded.value",
            params![key, value],
        )
        .context("metadata 저장 실패")?;
    Ok(())
}

fn insert_audit_on(
    connection: &Connection,
    actor_user_id: Option<i64>,
    event_kind: &str,
    outcome: &str,
    detail: &str,
) -> anyhow::Result<()> {
    connection
        .execute(
            "INSERT INTO audit_events(occurred_at, actor_user_id, event_kind, outcome, detail)\
             VALUES(?1, ?2, ?3, ?4, ?5)",
            params![now_unix(), actor_user_id, event_kind, outcome, detail],
        )
        .context("감사로그 저장 실패")?;
    Ok(())
}

fn hash_code(code: &str) -> [u8; 32] {
    Sha256::digest(code.as_bytes()).into()
}

fn now_unix() -> i64 {
    OffsetDateTime::now_utc().unix_timestamp()
}

#[cfg(test)]
mod tests {
    use tempfile::tempdir;

    use super::Store;

    #[test]
    fn pairing_is_single_use_and_persists_owner() -> anyhow::Result<()> {
        let directory = tempdir()?;
        let store = Store::open(directory.path().join("state.sqlite3"))?;
        let code = store.create_pairing_code(300)?;
        assert!(store.consume_pairing_code(&code, 123, 456)?);
        assert!(!store.consume_pairing_code(&code, 999, 999)?);
        let owner = store
            .owner()?
            .ok_or_else(|| anyhow::anyhow!("owner 없음"))?;
        assert_eq!(owner.user_id, 123);
        assert_eq!(owner.chat_id, 456);
        Ok(())
    }

    #[test]
    fn update_offset_round_trips() -> anyhow::Result<()> {
        let directory = tempdir()?;
        let store = Store::open(directory.path().join("state.sqlite3"))?;
        assert_eq!(store.update_offset()?, 0);
        store.set_update_offset(42)?;
        assert_eq!(store.update_offset()?, 42);
        Ok(())
    }
}
