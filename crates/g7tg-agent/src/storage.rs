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

use g7tg_core::ServiceAction;

const AUDIT_RETENTION_SECONDS: i64 = 30 * 86_400;
const AUDIT_MAX_ROWS: i64 = 10_000;
const APPROVAL_MAX_ROWS: i64 = 256;
const INCIDENT_HISTORY_MAX_ROWS: i64 = 5_000;
const OUTBOX_RETENTION_SECONDS: i64 = 7 * 86_400;
const OUTBOX_MAX_ROWS: i64 = 1_000;
const PAIRING_CODE_LENGTH: usize = 16;
const PAIRING_FAILURE_LIMIT: i64 = 5;
const PAIRING_FAILURE_WINDOW_SECONDS: i64 = 60;
const PAIRING_BLOCK_SECONDS: i64 = 60;
const STATUS_DIGEST_INTERVALS: [u64; 3] = [21_600, 43_200, 86_400];

/// 등록된 Telegram owner입니다.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Owner {
    /// Telegram user ID입니다.
    pub user_id: i64,
    /// 1:1 private chat ID입니다.
    pub chat_id: i64,
}

/// 소비된 단회 서비스 동작 승인입니다.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Approval {
    /// 승인한 Telegram user ID입니다.
    pub actor_user_id: i64,
    /// 승인한 동작입니다.
    pub action: ServiceAction,
    /// 승인한 정확한 systemd unit입니다.
    pub unit: String,
}

/// 한 monitoring cycle에서 관측한 문제입니다.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObservedIncident {
    /// 중복 제거에 사용하는 안정적인 key입니다.
    pub key: String,
    /// warning 또는 critical입니다.
    pub severity: String,
    /// 비밀값을 포함하지 않는 사용자 메시지입니다.
    pub summary: String,
}

/// 확인 횟수를 통과한 현재 장애입니다.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CurrentIncident {
    /// 안정적인 incident key입니다.
    pub key: String,
    /// 장애 등급입니다.
    pub severity: String,
    /// 최근 요약입니다.
    pub summary: String,
    /// 최초 관측시각입니다.
    pub first_seen: i64,
}

/// 전송 성공 전까지 SQLite에 남는 알림입니다.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingNotification {
    /// outbox row ID입니다.
    pub id: i64,
    /// opened 또는 recovered입니다.
    pub kind: String,
    /// 장애 등급입니다.
    pub severity: String,
    /// Telegram 본문입니다.
    pub summary: String,
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
                    expires_at INTEGER NOT NULL,
                    replace_owner INTEGER NOT NULL DEFAULT 0 CHECK (replace_owner IN (0, 1))
                );
                CREATE TABLE IF NOT EXISTS pairing_attempts (
                    actor_user_id INTEGER NOT NULL,
                    chat_id INTEGER NOT NULL,
                    window_started INTEGER NOT NULL,
                    failure_count INTEGER NOT NULL,
                    blocked_until INTEGER NOT NULL DEFAULT 0,
                    PRIMARY KEY(actor_user_id, chat_id)
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
                CREATE TABLE IF NOT EXISTS approvals (
                    token_hash BLOB PRIMARY KEY,
                    actor_user_id INTEGER NOT NULL,
                    action TEXT NOT NULL,
                    unit TEXT NOT NULL,
                    expires_at INTEGER NOT NULL,
                    consumed_at INTEGER
                );
                CREATE INDEX IF NOT EXISTS approvals_expires_at
                    ON approvals(expires_at);
                CREATE TABLE IF NOT EXISTS current_incidents (
                    incident_key TEXT PRIMARY KEY,
                    severity TEXT NOT NULL,
                    summary TEXT NOT NULL,
                    first_seen INTEGER NOT NULL,
                    last_seen INTEGER NOT NULL,
                    consecutive_count INTEGER NOT NULL,
                    opened_emitted INTEGER NOT NULL CHECK (opened_emitted IN (0, 1))
                );
                CREATE TABLE IF NOT EXISTS incident_history (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    incident_key TEXT NOT NULL,
                    severity TEXT NOT NULL,
                    summary TEXT NOT NULL,
                    opened_at INTEGER NOT NULL,
                    resolved_at INTEGER NOT NULL
                );
                CREATE TABLE IF NOT EXISTS notification_outbox (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    notification_kind TEXT NOT NULL,
                    incident_key TEXT NOT NULL,
                    severity TEXT NOT NULL,
                    summary TEXT NOT NULL,
                    created_at INTEGER NOT NULL,
                    sent_at INTEGER
                );
                CREATE INDEX IF NOT EXISTS notification_outbox_pending
                    ON notification_outbox(sent_at, id);
                "#,
            )
            .context("SQLite schema 준비 실패")?;
        ensure_pairing_replacement_column(&connection)?;
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
        self.create_pairing_code_with_mode(ttl_seconds, false)
    }

    /// 기존 owner를 유지한 채 교체용 단회 code를 반환합니다.
    pub fn create_owner_replacement_code(&self, ttl_seconds: u64) -> anyhow::Result<String> {
        anyhow::ensure!(self.owner()?.is_some(), "교체할 기존 owner가 없습니다");
        self.create_pairing_code_with_mode(ttl_seconds, true)
    }

    fn create_pairing_code_with_mode(
        &self,
        ttl_seconds: u64,
        replace_owner: bool,
    ) -> anyhow::Result<String> {
        let code: String = Uuid::new_v4()
            .simple()
            .to_string()
            .chars()
            .take(PAIRING_CODE_LENGTH)
            .map(|character| character.to_ascii_uppercase())
            .collect();
        let code_hash = hash_code(&code);
        let expires_at = now_unix()
            .checked_add(i64::try_from(ttl_seconds).context("pairing TTL 변환 실패")?)
            .ok_or_else(|| anyhow!("pairing 만료시각 overflow"))?;
        let connection = self.lock()?;
        connection
            .execute(
                "INSERT INTO pairing(singleton, code_hash, expires_at, replace_owner) \
                 VALUES(1, ?1, ?2, ?3) \
                 ON CONFLICT(singleton) DO UPDATE SET \
                    code_hash=excluded.code_hash, expires_at=excluded.expires_at, \
                    replace_owner=excluded.replace_owner",
                params![
                    code_hash.as_slice(),
                    expires_at,
                    if replace_owner { 1_i64 } else { 0_i64 }
                ],
            )
            .context("pairing code 저장 실패")?;
        connection
            .execute("DELETE FROM pairing_attempts", [])
            .context("pairing 실패 제한 초기화 실패")?;
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
                "SELECT code_hash, expires_at, replace_owner FROM pairing WHERE singleton=1",
                [],
                |row| {
                    Ok((
                        row.get::<_, Vec<u8>>(0)?,
                        row.get::<_, i64>(1)?,
                        row.get::<_, i64>(2)?,
                    ))
                },
            )
            .optional()
            .context("pairing code 조회 실패")?;
        let Some((stored_hash, expires_at, replace_owner)) = stored else {
            return Ok(false);
        };
        let now = now_unix();
        let blocked_until = transaction
            .query_row(
                "SELECT blocked_until FROM pairing_attempts \
                 WHERE actor_user_id=?1 AND chat_id=?2",
                params![user_id, chat_id],
                |row| row.get::<_, i64>(0),
            )
            .optional()
            .context("pairing 실패 제한 조회 실패")?
            .unwrap_or(0);
        if blocked_until > now {
            return Ok(false);
        }
        let candidate = hash_code(code.trim());
        let hash_matches: bool = stored_hash.as_slice().ct_eq(candidate.as_slice()).into();
        let owner_exists = metadata_i64_on(&transaction, "owner_user_id")?.is_some()
            || metadata_i64_on(&transaction, "owner_chat_id")?.is_some();
        if expires_at < now || !hash_matches || (owner_exists && replace_owner != 1) {
            register_pairing_failure_on(&transaction, user_id, chat_id, now)?;
            transaction
                .commit()
                .context("pairing 실패 제한 commit 실패")?;
            return Ok(false);
        }

        set_metadata_on(&transaction, "owner_user_id", &user_id.to_string())?;
        set_metadata_on(&transaction, "owner_chat_id", &chat_id.to_string())?;
        transaction
            .execute("DELETE FROM pairing WHERE singleton=1", [])
            .context("pairing code 소비 실패")?;
        transaction
            .execute(
                "DELETE FROM pairing_attempts WHERE actor_user_id=?1 AND chat_id=?2",
                params![user_id, chat_id],
            )
            .context("pairing 실패 제한 제거 실패")?;
        insert_audit_on(
            &transaction,
            Some(user_id),
            if owner_exists {
                "owner_replaced"
            } else {
                "owner_paired"
            },
            "success",
            "private_chat",
        )?;
        prune_audit_on(&transaction, now_unix())?;
        transaction
            .commit()
            .context("pairing transaction commit 실패")?;
        Ok(true)
    }

    /// owner와 대기 중인 연결코드·승인을 제거합니다.
    pub fn clear_owner(&self) -> anyhow::Result<bool> {
        let mut connection = self.lock()?;
        let transaction = connection
            .transaction()
            .context("owner 해제 transaction 시작 실패")?;
        let existed = metadata_i64_on(&transaction, "owner_user_id")?.is_some()
            || metadata_i64_on(&transaction, "owner_chat_id")?.is_some();
        transaction
            .execute(
                "DELETE FROM metadata WHERE key IN ('owner_user_id', 'owner_chat_id')",
                [],
            )
            .context("owner metadata 제거 실패")?;
        transaction
            .execute("DELETE FROM pairing", [])
            .context("pairing code 제거 실패")?;
        transaction
            .execute("DELETE FROM approvals", [])
            .context("승인 제거 실패")?;
        transaction
            .execute("DELETE FROM pairing_attempts", [])
            .context("pairing 실패 제한 제거 실패")?;
        if existed {
            insert_audit_on(
                &transaction,
                None,
                "owner_unpaired",
                "success",
                "local_root",
            )?;
            prune_audit_on(&transaction, now_unix())?;
        }
        transaction
            .commit()
            .context("owner 해제 transaction commit 실패")?;
        Ok(existed)
    }

    /// 보안과 운영 이벤트를 기록합니다.
    pub fn audit(
        &self,
        actor_user_id: Option<i64>,
        event_kind: &str,
        outcome: &str,
        detail: &str,
    ) -> anyhow::Result<()> {
        let mut connection = self.lock()?;
        let transaction = connection
            .transaction()
            .context("감사로그 transaction 시작 실패")?;
        let now = now_unix();
        insert_audit_at_on(
            &transaction,
            now,
            actor_user_id,
            event_kind,
            outcome,
            detail,
        )?;
        prune_audit_on(&transaction, now)?;
        transaction
            .commit()
            .context("감사로그 transaction commit 실패")
    }

    /// 특정 사용자·동작·unit에 묶인 단회 승인 token을 발급합니다.
    pub fn create_approval(
        &self,
        actor_user_id: i64,
        action: ServiceAction,
        unit: &str,
        ttl_seconds: u64,
    ) -> anyhow::Result<String> {
        let token = Uuid::new_v4().simple().to_string();
        let token_hash = hash_code(&token);
        let expires_at = now_unix()
            .checked_add(i64::try_from(ttl_seconds).context("approval TTL 변환 실패")?)
            .ok_or_else(|| anyhow!("approval 만료시각 overflow"))?;
        let connection = self.lock()?;
        connection
            .execute(
                "INSERT INTO approvals(token_hash, actor_user_id, action, unit, expires_at) \
                 VALUES(?1, ?2, ?3, ?4, ?5)",
                params![
                    token_hash.as_slice(),
                    actor_user_id,
                    action.id(),
                    unit,
                    expires_at
                ],
            )
            .context("approval 저장 실패")?;
        prune_approvals_on(&connection, now_unix())?;
        Ok(token)
    }

    /// 승인 token을 정확히 한 번 소비합니다.
    pub fn consume_approval(
        &self,
        token: &str,
        actor_user_id: i64,
    ) -> anyhow::Result<Option<Approval>> {
        let token_hash = hash_code(token);
        let mut connection = self.lock()?;
        let transaction = connection
            .transaction()
            .context("approval transaction 시작 실패")?;
        let row = transaction
            .query_row(
                "SELECT actor_user_id, action, unit, expires_at, consumed_at \
                 FROM approvals WHERE token_hash=?1",
                [token_hash.as_slice()],
                |row| {
                    Ok((
                        row.get::<_, i64>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, i64>(3)?,
                        row.get::<_, Option<i64>>(4)?,
                    ))
                },
            )
            .optional()
            .context("approval 조회 실패")?;
        let Some((stored_actor, action, unit, expires_at, consumed_at)) = row else {
            return Ok(None);
        };
        if stored_actor != actor_user_id || expires_at < now_unix() || consumed_at.is_some() {
            return Ok(None);
        }
        let action = match action.as_str() {
            "restart" => ServiceAction::Restart,
            _ => return Err(anyhow!("저장된 approval action이 올바르지 않습니다")),
        };
        transaction
            .execute(
                "UPDATE approvals SET consumed_at=?1 WHERE token_hash=?2 AND consumed_at IS NULL",
                params![now_unix(), token_hash.as_slice()],
            )
            .context("approval 소비 실패")?;
        transaction
            .commit()
            .context("approval transaction commit 실패")?;
        Ok(Some(Approval {
            actor_user_id,
            action,
            unit,
        }))
    }

    /// 취소 callback도 token을 재사용할 수 없도록 소비합니다.
    pub fn cancel_approval(&self, token: &str, actor_user_id: i64) -> anyhow::Result<bool> {
        Ok(self.consume_approval(token, actor_user_id)?.is_some())
    }

    /// 만료·소비된 승인을 제한된 수로 정리합니다.
    pub fn prune_approvals(&self) -> anyhow::Result<usize> {
        let connection = self.lock()?;
        prune_approvals_on(&connection, now_unix())
    }

    /// 관측된 문제와 현재 장애를 원자적으로 대조하고 outbox를 생성합니다.
    #[cfg(test)]
    pub fn reconcile_incidents(
        &self,
        observed: &[ObservedIncident],
        confirmation_count: u32,
    ) -> anyhow::Result<()> {
        self.reconcile_incidents_scoped(observed, confirmation_count, &[])
    }

    /// 지정한 incident key prefix 범위만 원자적으로 대조합니다.
    pub fn reconcile_incidents_scoped(
        &self,
        observed: &[ObservedIncident],
        confirmation_count: u32,
        prefixes: &[&str],
    ) -> anyhow::Result<()> {
        let mut connection = self.lock()?;
        let transaction = connection
            .transaction()
            .context("incident transaction 시작 실패")?;
        let now = now_unix();
        let confirmation_count = i64::from(confirmation_count);
        let mut observed_keys = std::collections::BTreeSet::new();

        for incident in observed {
            validate_incident(incident)?;
            anyhow::ensure!(
                prefixes.is_empty()
                    || prefixes
                        .iter()
                        .any(|prefix| incident.key.starts_with(prefix)),
                "incident가 허용된 대조 범위를 벗어났습니다"
            );
            observed_keys.insert(incident.key.as_str());
            let existing = transaction
                .query_row(
                    "SELECT first_seen, consecutive_count, opened_emitted \
                     FROM current_incidents WHERE incident_key=?1",
                    [&incident.key],
                    |row| {
                        Ok((
                            row.get::<_, i64>(0)?,
                            row.get::<_, i64>(1)?,
                            row.get::<_, i64>(2)?,
                        ))
                    },
                )
                .optional()
                .context("현재 incident 조회 실패")?;
            let (first_seen, consecutive, mut emitted) = existing
                .map_or((now, 1_i64, 0_i64), |(first_seen, count, emitted)| {
                    (first_seen, count.saturating_add(1), emitted)
                });
            if emitted == 0 && consecutive >= confirmation_count {
                insert_notification_on(&transaction, "opened", incident, now)?;
                emitted = 1;
            }
            transaction
                .execute(
                    r#"INSERT INTO current_incidents(
                        incident_key, severity, summary, first_seen, last_seen,
                        consecutive_count, opened_emitted
                    ) VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7)
                    ON CONFLICT(incident_key) DO UPDATE SET
                        severity=excluded.severity,
                        summary=excluded.summary,
                        last_seen=excluded.last_seen,
                        consecutive_count=excluded.consecutive_count,
                        opened_emitted=excluded.opened_emitted"#,
                    params![
                        incident.key,
                        incident.severity,
                        incident.summary,
                        first_seen,
                        now,
                        consecutive,
                        emitted
                    ],
                )
                .context("현재 incident 저장 실패")?;
        }

        let current: Vec<(String, String, String, i64, i64)> = {
            let mut statement = transaction
                .prepare(
                    "SELECT incident_key, severity, summary, first_seen, opened_emitted \
                     FROM current_incidents",
                )
                .context("현재 incident 목록 준비 실패")?;
            let rows = statement
                .query_map([], |row| {
                    Ok((
                        row.get(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                    ))
                })
                .context("현재 incident 목록 조회 실패")?;
            rows.collect::<Result<_, _>>()
                .context("현재 incident 목록 변환 실패")?
        };
        for (key, severity, summary, first_seen, emitted) in current {
            if !prefixes.is_empty() && !prefixes.iter().any(|prefix| key.starts_with(prefix)) {
                continue;
            }
            if observed_keys.contains(key.as_str()) {
                continue;
            }
            if emitted == 1 {
                insert_notification_on(
                    &transaction,
                    "recovered",
                    &ObservedIncident {
                        key: key.clone(),
                        severity: severity.clone(),
                        summary: summary.clone(),
                    },
                    now,
                )?;
                transaction
                    .execute(
                        "INSERT INTO incident_history(incident_key, severity, summary, opened_at, resolved_at) \
                         VALUES(?1, ?2, ?3, ?4, ?5)",
                        params![key, severity, summary, first_seen, now],
                    )
                    .context("incident history 저장 실패")?;
            }
            transaction
                .execute(
                    "DELETE FROM current_incidents WHERE incident_key=?1",
                    [&key],
                )
                .context("복구된 incident 제거 실패")?;
        }

        let history_cutoff = now.saturating_sub(90 * 86_400);
        transaction
            .execute(
                "DELETE FROM incident_history WHERE id IN (\
                    SELECT id FROM incident_history WHERE resolved_at < ?1 LIMIT 256\
                )",
                [history_cutoff],
            )
            .context("incident history 보존정책 실패")?;
        prune_table_to_max_on(&transaction, "incident_history", INCIDENT_HISTORY_MAX_ROWS)?;
        let outbox_cutoff = now.saturating_sub(OUTBOX_RETENTION_SECONDS);
        transaction
            .execute(
                "DELETE FROM notification_outbox WHERE id IN (\
                    SELECT id FROM notification_outbox \
                    WHERE created_at < ?1 LIMIT 256\
                )",
                [outbox_cutoff],
            )
            .context("notification outbox 보존정책 실패")?;
        prune_table_to_max_on(&transaction, "notification_outbox", OUTBOX_MAX_ROWS)?;
        transaction
            .commit()
            .context("incident transaction commit 실패")
    }

    /// 확인 횟수를 통과한 현재 장애를 반환합니다.
    pub fn current_incidents(&self) -> anyhow::Result<Vec<CurrentIncident>> {
        let connection = self.lock()?;
        let mut statement = connection
            .prepare(
                "SELECT incident_key, severity, summary, first_seen \
                 FROM current_incidents WHERE opened_emitted=1 ORDER BY severity, first_seen",
            )
            .context("현재 장애 query 준비 실패")?;
        let rows = statement
            .query_map([], |row| {
                Ok(CurrentIncident {
                    key: row.get(0)?,
                    severity: row.get(1)?,
                    summary: row.get(2)?,
                    first_seen: row.get(3)?,
                })
            })
            .context("현재 장애 query 실패")?;
        rows.collect::<Result<_, _>>()
            .context("현재 장애 변환 실패")
    }

    /// 아직 Telegram 전송이 확인되지 않은 outbox입니다.
    pub fn pending_notifications(&self, limit: usize) -> anyhow::Result<Vec<PendingNotification>> {
        let limit = i64::try_from(limit.min(100)).context("notification limit 변환 실패")?;
        let connection = self.lock()?;
        let mut statement = connection
            .prepare(
                "SELECT id, notification_kind, severity, summary \
                 FROM notification_outbox WHERE sent_at IS NULL ORDER BY id LIMIT ?1",
            )
            .context("notification outbox query 준비 실패")?;
        let rows = statement
            .query_map([limit], |row| {
                Ok(PendingNotification {
                    id: row.get(0)?,
                    kind: row.get(1)?,
                    severity: row.get(2)?,
                    summary: row.get(3)?,
                })
            })
            .context("notification outbox query 실패")?;
        rows.collect::<Result<_, _>>()
            .context("notification outbox 변환 실패")
    }

    /// 성공적으로 전송한 outbox를 완료 처리합니다.
    pub fn mark_notification_sent(&self, id: i64) -> anyhow::Result<()> {
        let connection = self.lock()?;
        connection
            .execute(
                "UPDATE notification_outbox SET sent_at=?1 WHERE id=?2 AND sent_at IS NULL",
                params![now_unix(), id],
            )
            .context("notification outbox 완료 처리 실패")?;
        Ok(())
    }

    /// 현재 유효한 알림 silence 만료시각입니다.
    pub fn silence_until(&self) -> anyhow::Result<Option<i64>> {
        Ok(self
            .metadata_i64("notification_silence_until")?
            .filter(|expires_at| *expires_at > now_unix()))
    }

    /// 허용된 기간 동안 알림 전송만 중지합니다.
    pub fn set_silence(&self, duration_seconds: u64) -> anyhow::Result<i64> {
        let expires_at = now_unix()
            .checked_add(i64::try_from(duration_seconds).context("silence 기간 변환 실패")?)
            .ok_or_else(|| anyhow!("silence 만료시각 overflow"))?;
        let mut connection = self.lock()?;
        let transaction = connection
            .transaction()
            .context("silence transaction 시작 실패")?;
        set_metadata_on(
            &transaction,
            "notification_silence_until",
            &expires_at.to_string(),
        )?;
        discard_pending_notifications_on(&transaction)?;
        transaction
            .commit()
            .context("silence transaction commit 실패")?;
        Ok(expires_at)
    }

    /// 만료됐지만 아직 종료 안내를 하지 않은 silence 시각입니다.
    pub fn expired_silence_until(&self) -> anyhow::Result<Option<i64>> {
        Ok(self
            .metadata_i64("notification_silence_until")?
            .filter(|expires_at| *expires_at <= now_unix()))
    }

    /// 아직 보내지 않은 알림을 폐기해 silence 뒤 지연 전송을 막습니다.
    pub fn discard_pending_notifications(&self) -> anyhow::Result<usize> {
        let connection = self.lock()?;
        discard_pending_notifications_on(&connection)
    }

    /// 알림 silence를 즉시 해제합니다.
    pub fn clear_silence(&self) -> anyhow::Result<()> {
        let mut connection = self.lock()?;
        let transaction = connection
            .transaction()
            .context("silence 해제 transaction 시작 실패")?;
        discard_pending_notifications_on(&transaction)?;
        transaction
            .execute(
                "DELETE FROM metadata WHERE key='notification_silence_until'",
                [],
            )
            .context("silence 해제 실패")?;
        transaction
            .commit()
            .context("silence 해제 transaction commit 실패")
    }

    /// Telegram 정기 상태 요약 간격입니다. `None`이면 꺼짐입니다.
    pub fn status_digest_interval_seconds(&self) -> anyhow::Result<Option<u64>> {
        let Some(value) = self.metadata_i64("status_digest_interval_seconds")? else {
            return Ok(None);
        };
        let interval = u64::try_from(value).context("정기 상태 요약 간격 변환 실패")?;
        anyhow::ensure!(
            STATUS_DIGEST_INTERVALS.contains(&interval),
            "저장된 정기 상태 요약 간격이 허용 범위를 벗어났습니다"
        );
        Ok(Some(interval))
    }

    /// 정기 상태 요약을 꺼짐 또는 6·12·24시간 중 하나로 제한해 저장합니다.
    pub fn set_status_digest_interval_seconds(&self, interval: Option<u64>) -> anyhow::Result<()> {
        if let Some(interval) = interval {
            anyhow::ensure!(
                STATUS_DIGEST_INTERVALS.contains(&interval),
                "허용되지 않은 정기 상태 요약 간격입니다"
            );
        }
        let mut connection = self.lock()?;
        let transaction = connection
            .transaction()
            .context("정기 상태 요약 설정 transaction 시작 실패")?;
        match interval {
            Some(interval) => {
                set_metadata_on(
                    &transaction,
                    "status_digest_interval_seconds",
                    &interval.to_string(),
                )?;
                set_metadata_on(
                    &transaction,
                    "status_digest_last_sent_at",
                    &now_unix().to_string(),
                )?;
            }
            None => {
                transaction
                    .execute(
                        "DELETE FROM metadata WHERE key IN \
                         ('status_digest_interval_seconds', 'status_digest_last_sent_at')",
                        [],
                    )
                    .context("정기 상태 요약 설정 제거 실패")?;
            }
        }
        transaction
            .commit()
            .context("정기 상태 요약 설정 commit 실패")
    }

    /// 지정 시각에 정기 상태 요약을 보낼 차례인지 확인합니다.
    pub fn status_digest_due(&self, now: i64) -> anyhow::Result<bool> {
        let Some(interval) = self.status_digest_interval_seconds()? else {
            return Ok(false);
        };
        let interval = i64::try_from(interval).context("정기 상태 요약 간격 변환 실패")?;
        let last_sent = self
            .metadata_i64("status_digest_last_sent_at")?
            .unwrap_or(now);
        Ok(now.saturating_sub(last_sent) >= interval)
    }

    /// 성공적으로 전송한 정기 상태 요약의 시각을 기록합니다.
    pub fn mark_status_digest_sent(&self, sent_at: i64) -> anyhow::Result<()> {
        self.set_metadata("status_digest_last_sent_at", &sent_at.to_string())
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
            "INSERT INTO metadata(key, value) VALUES(?1, ?2) \
             ON CONFLICT(key) DO UPDATE SET value=excluded.value",
            params![key, value],
        )
        .context("metadata 저장 실패")?;
    Ok(())
}

fn metadata_i64_on(connection: &Connection, key: &str) -> anyhow::Result<Option<i64>> {
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

fn register_pairing_failure_on(
    connection: &Connection,
    user_id: i64,
    chat_id: i64,
    now: i64,
) -> anyhow::Result<()> {
    let previous = connection
        .query_row(
            "SELECT window_started, failure_count FROM pairing_attempts \
             WHERE actor_user_id=?1 AND chat_id=?2",
            params![user_id, chat_id],
            |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?)),
        )
        .optional()
        .context("pairing 실패 횟수 조회 실패")?;
    let (window_started, failure_count) = match previous {
        Some((window_started, failure_count))
            if now.saturating_sub(window_started) < PAIRING_FAILURE_WINDOW_SECONDS =>
        {
            (window_started, failure_count.saturating_add(1))
        }
        _ => (now, 1),
    };
    let blocked_until = if failure_count >= PAIRING_FAILURE_LIMIT {
        now.saturating_add(PAIRING_BLOCK_SECONDS)
    } else {
        0
    };
    connection
        .execute(
            "INSERT INTO pairing_attempts( \
                actor_user_id, chat_id, window_started, failure_count, blocked_until \
             ) VALUES(?1, ?2, ?3, ?4, ?5) \
             ON CONFLICT(actor_user_id, chat_id) DO UPDATE SET \
                window_started=excluded.window_started, \
                failure_count=excluded.failure_count, \
                blocked_until=excluded.blocked_until",
            params![
                user_id,
                chat_id,
                window_started,
                failure_count,
                blocked_until
            ],
        )
        .context("pairing 실패 횟수 저장 실패")?;
    connection
        .execute(
            "DELETE FROM pairing_attempts WHERE window_started < ?1",
            [now.saturating_sub(86_400)],
        )
        .context("pairing 실패 제한 정리 실패")?;
    Ok(())
}

fn ensure_pairing_replacement_column(connection: &Connection) -> anyhow::Result<()> {
    let mut statement = connection
        .prepare("PRAGMA table_info(pairing)")
        .context("pairing schema 조회 준비 실패")?;
    let columns = statement
        .query_map([], |row| row.get::<_, String>(1))
        .context("pairing schema 조회 실패")?
        .collect::<Result<Vec<_>, _>>()
        .context("pairing schema 변환 실패")?;
    drop(statement);
    if !columns.iter().any(|column| column == "replace_owner") {
        connection
            .execute(
                "ALTER TABLE pairing ADD COLUMN replace_owner INTEGER NOT NULL DEFAULT 0 \
                 CHECK (replace_owner IN (0, 1))",
                [],
            )
            .context("pairing schema migration 실패")?;
    }
    Ok(())
}

fn insert_audit_on(
    connection: &Connection,
    actor_user_id: Option<i64>,
    event_kind: &str,
    outcome: &str,
    detail: &str,
) -> anyhow::Result<()> {
    insert_audit_at_on(
        connection,
        now_unix(),
        actor_user_id,
        event_kind,
        outcome,
        detail,
    )
}

fn insert_audit_at_on(
    connection: &Connection,
    occurred_at: i64,
    actor_user_id: Option<i64>,
    event_kind: &str,
    outcome: &str,
    detail: &str,
) -> anyhow::Result<()> {
    connection
        .execute(
            "INSERT INTO audit_events(occurred_at, actor_user_id, event_kind, outcome, detail) \
             VALUES(?1, ?2, ?3, ?4, ?5)",
            params![occurred_at, actor_user_id, event_kind, outcome, detail],
        )
        .context("감사로그 저장 실패")?;
    Ok(())
}

fn prune_audit_on(connection: &Connection, now: i64) -> anyhow::Result<usize> {
    let cutoff = now.saturating_sub(AUDIT_RETENTION_SECONDS);
    let expired = connection
        .execute(
            "DELETE FROM audit_events WHERE id IN (\
                SELECT id FROM audit_events WHERE occurred_at < ?1 ORDER BY id LIMIT 256\
            )",
            [cutoff],
        )
        .context("감사로그 기간 보존정책 실패")?;
    let overflow = connection
        .execute(
            "DELETE FROM audit_events WHERE id <= COALESCE((\
                SELECT id FROM audit_events ORDER BY id DESC LIMIT 1 OFFSET ?1\
            ), -1)",
            [AUDIT_MAX_ROWS],
        )
        .context("감사로그 개수 보존정책 실패")?;
    Ok(expired.saturating_add(overflow))
}

fn prune_approvals_on(connection: &Connection, now: i64) -> anyhow::Result<usize> {
    let stale = connection
        .execute(
            "DELETE FROM approvals WHERE rowid IN ( \
                SELECT rowid FROM approvals \
                WHERE expires_at < ?1 OR consumed_at IS NOT NULL \
                ORDER BY expires_at ASC LIMIT 256 \
            )",
            [now],
        )
        .context("approval 정리 실패")?;
    let overflow = connection
        .execute(
            "DELETE FROM approvals WHERE rowid <= COALESCE((\
                SELECT rowid FROM approvals ORDER BY rowid DESC LIMIT 1 OFFSET ?1\
            ), -1)",
            [APPROVAL_MAX_ROWS],
        )
        .context("approval 개수 보존정책 실패")?;
    Ok(stale.saturating_add(overflow))
}

fn prune_table_to_max_on(
    connection: &Connection,
    table: &str,
    max_rows: i64,
) -> anyhow::Result<usize> {
    let statement = match table {
        "incident_history" => {
            "DELETE FROM incident_history WHERE id <= COALESCE((\
                SELECT id FROM incident_history ORDER BY id DESC LIMIT 1 OFFSET ?1\
            ), -1)"
        }
        "notification_outbox" => {
            "DELETE FROM notification_outbox WHERE id <= COALESCE((\
                SELECT id FROM notification_outbox ORDER BY id DESC LIMIT 1 OFFSET ?1\
            ), -1)"
        }
        _ => return Err(anyhow!("지원하지 않는 보존정책 table입니다")),
    };
    connection
        .execute(statement, [max_rows])
        .with_context(|| format!("{table} 개수 보존정책 실패"))
}

fn discard_pending_notifications_on(connection: &Connection) -> anyhow::Result<usize> {
    connection
        .execute("DELETE FROM notification_outbox WHERE sent_at IS NULL", [])
        .context("대기 알림 폐기 실패")
}

fn insert_notification_on(
    connection: &Connection,
    kind: &str,
    incident: &ObservedIncident,
    now: i64,
) -> anyhow::Result<()> {
    connection
        .execute(
            "INSERT INTO notification_outbox(\
                notification_kind, incident_key, severity, summary, created_at\
             ) VALUES(?1, ?2, ?3, ?4, ?5)",
            params![kind, incident.key, incident.severity, incident.summary, now],
        )
        .context("notification outbox 저장 실패")?;
    Ok(())
}

fn validate_incident(incident: &ObservedIncident) -> anyhow::Result<()> {
    anyhow::ensure!(
        !incident.key.is_empty() && incident.key.len() <= 160,
        "incident key 길이가 올바르지 않습니다"
    );
    anyhow::ensure!(
        matches!(incident.severity.as_str(), "warning" | "critical"),
        "incident severity가 올바르지 않습니다"
    );
    anyhow::ensure!(
        !incident.summary.is_empty() && incident.summary.chars().count() <= 500,
        "incident summary 길이가 올바르지 않습니다"
    );
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
    use rusqlite::Connection;
    use tempfile::tempdir;

    use super::{AUDIT_MAX_ROWS, ObservedIncident, Store, now_unix};

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
    fn owner_replacement_keeps_old_owner_until_code_is_used() -> anyhow::Result<()> {
        let directory = tempdir()?;
        let store = Store::open(directory.path().join("state.sqlite3"))?;
        let initial = store.create_pairing_code(300)?;
        assert!(store.consume_pairing_code(&initial, 123, 456)?);

        let ordinary = store.create_pairing_code(300)?;
        assert!(!store.consume_pairing_code(&ordinary, 999, 999)?);
        assert_eq!(store.owner()?.map(|owner| owner.user_id), Some(123));

        let replacement = store.create_owner_replacement_code(300)?;
        assert_eq!(store.owner()?.map(|owner| owner.user_id), Some(123));
        assert!(store.consume_pairing_code(&replacement, 999, 999)?);
        assert_eq!(store.owner()?.map(|owner| owner.user_id), Some(999));
        Ok(())
    }

    #[test]
    fn clear_owner_removes_identity_and_pending_approvals() -> anyhow::Result<()> {
        let directory = tempdir()?;
        let store = Store::open(directory.path().join("state.sqlite3"))?;
        let code = store.create_pairing_code(300)?;
        assert!(store.consume_pairing_code(&code, 123, 456)?);
        let approval =
            store.create_approval(123, g7tg_core::ServiceAction::Restart, "nginx.service", 45)?;
        assert!(store.clear_owner()?);
        assert!(store.owner()?.is_none());
        assert!(store.consume_approval(&approval, 123)?.is_none());
        assert!(!store.clear_owner()?);
        Ok(())
    }

    #[test]
    fn old_pairing_schema_is_migrated() -> anyhow::Result<()> {
        let directory = tempdir()?;
        let path = directory.path().join("state.sqlite3");
        let connection = Connection::open(&path)?;
        connection.execute_batch(
            "CREATE TABLE pairing (\
                singleton INTEGER PRIMARY KEY CHECK (singleton = 1),\
                code_hash BLOB NOT NULL,\
                expires_at INTEGER NOT NULL\
            );",
        )?;
        drop(connection);

        let store = Store::open(&path)?;
        assert_eq!(store.create_pairing_code(300)?.len(), 16);
        Ok(())
    }

    #[test]
    fn pairing_code_is_64_bit_and_failed_attempts_are_throttled() -> anyhow::Result<()> {
        let directory = tempdir()?;
        let store = Store::open(directory.path().join("state.sqlite3"))?;
        let code = store.create_pairing_code(300)?;
        assert_eq!(code.len(), 16);
        for _ in 0..5 {
            assert!(!store.consume_pairing_code("WRONG-CODE", 123, 456)?);
        }
        assert!(!store.consume_pairing_code(&code, 123, 456)?);

        {
            let connection = store.lock()?;
            connection.execute(
                "UPDATE pairing_attempts SET blocked_until=0 WHERE actor_user_id=123 AND chat_id=456",
                [],
            )?;
        }
        assert!(store.consume_pairing_code(&code, 123, 456)?);
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

    #[test]
    fn approval_is_actor_bound_and_single_use() -> anyhow::Result<()> {
        let directory = tempdir()?;
        let store = Store::open(directory.path().join("state.sqlite3"))?;
        let token =
            store.create_approval(123, g7tg_core::ServiceAction::Restart, "nginx.service", 45)?;
        assert!(store.consume_approval(&token, 999)?.is_none());
        let approval = store
            .consume_approval(&token, 123)?
            .ok_or_else(|| anyhow::anyhow!("approval 없음"))?;
        assert_eq!(approval.unit, "nginx.service");
        assert!(store.consume_approval(&token, 123)?.is_none());
        Ok(())
    }

    #[test]
    fn incidents_require_confirmation_and_emit_recovery() -> anyhow::Result<()> {
        let directory = tempdir()?;
        let store = Store::open(directory.path().join("state.sqlite3"))?;
        let issue = super::ObservedIncident {
            key: "service:nginx".to_owned(),
            severity: "critical".to_owned(),
            summary: "Nginx 중지".to_owned(),
        };
        store.reconcile_incidents(std::slice::from_ref(&issue), 2)?;
        assert!(store.current_incidents()?.is_empty());
        assert!(store.pending_notifications(10)?.is_empty());
        store.reconcile_incidents(std::slice::from_ref(&issue), 2)?;
        assert_eq!(store.current_incidents()?.len(), 1);
        assert_eq!(store.pending_notifications(10)?.len(), 1);
        store.reconcile_incidents(&[], 2)?;
        assert!(store.current_incidents()?.is_empty());
        assert_eq!(store.pending_notifications(10)?.len(), 2);
        Ok(())
    }

    #[test]
    fn scoped_reconciliation_preserves_failed_collector_state() -> anyhow::Result<()> {
        let directory = tempdir()?;
        let store = Store::open(directory.path().join("state.sqlite3"))?;
        let service = ObservedIncident {
            key: "service:nginx.service".to_owned(),
            severity: "critical".to_owned(),
            summary: "nginx stopped".to_owned(),
        };
        let web = ObservedIncident {
            key: "web:site".to_owned(),
            severity: "critical".to_owned(),
            summary: "site failed".to_owned(),
        };
        store.reconcile_incidents(&[service, web], 1)?;
        store.reconcile_incidents_scoped(&[], 1, &["service:"])?;
        let incidents = store.current_incidents()?;
        assert_eq!(incidents.len(), 1);
        assert_eq!(incidents[0].key, "web:site");
        Ok(())
    }

    #[test]
    fn silence_can_be_set_and_cleared() -> anyhow::Result<()> {
        let directory = tempdir()?;
        let store = Store::open(directory.path().join("state.sqlite3"))?;
        let issue = ObservedIncident {
            key: "service:nginx".to_owned(),
            severity: "critical".to_owned(),
            summary: "Nginx 중지".to_owned(),
        };
        store.reconcile_incidents(&[issue], 1)?;
        assert_eq!(store.pending_notifications(10)?.len(), 1);
        assert!(store.silence_until()?.is_none());
        assert!(store.set_silence(3600)? > 0);
        assert!(store.silence_until()?.is_some());
        assert!(store.pending_notifications(10)?.is_empty());
        store.clear_silence()?;
        assert!(store.silence_until()?.is_none());
        Ok(())
    }

    #[test]
    fn expired_silence_is_detected_until_cleared() -> anyhow::Result<()> {
        let directory = tempdir()?;
        let store = Store::open(directory.path().join("state.sqlite3"))?;
        store.set_metadata(
            "notification_silence_until",
            &now_unix().saturating_sub(1).to_string(),
        )?;
        assert!(store.silence_until()?.is_none());
        assert!(store.expired_silence_until()?.is_some());
        store.clear_silence()?;
        assert!(store.expired_silence_until()?.is_none());
        Ok(())
    }

    #[test]
    fn status_digest_accepts_only_fixed_intervals_and_tracks_due_time() -> anyhow::Result<()> {
        let directory = tempdir()?;
        let store = Store::open(directory.path().join("state.sqlite3"))?;
        assert_eq!(store.status_digest_interval_seconds()?, None);
        assert!(
            store
                .set_status_digest_interval_seconds(Some(3600))
                .is_err()
        );

        store.set_status_digest_interval_seconds(Some(21_600))?;
        assert_eq!(store.status_digest_interval_seconds()?, Some(21_600));
        assert!(!store.status_digest_due(now_unix())?);
        store.set_metadata(
            "status_digest_last_sent_at",
            &now_unix().saturating_sub(21_601).to_string(),
        )?;
        assert!(store.status_digest_due(now_unix())?);
        store.mark_status_digest_sent(now_unix())?;
        assert!(!store.status_digest_due(now_unix())?);

        store.set_status_digest_interval_seconds(None)?;
        assert_eq!(store.status_digest_interval_seconds()?, None);
        Ok(())
    }

    #[test]
    fn audit_log_is_capped() -> anyhow::Result<()> {
        let directory = tempdir()?;
        let store = Store::open(directory.path().join("state.sqlite3"))?;
        {
            let connection = store.lock()?;
            connection.execute_batch(&format!(
                "WITH RECURSIVE counter(value) AS (\
                    VALUES(1) UNION ALL SELECT value + 1 FROM counter WHERE value < {}\
                 )\
                 INSERT INTO audit_events(occurred_at, event_kind, outcome, detail)\
                 SELECT {}, 'fixture', 'success', 'bounded' FROM counter;",
                AUDIT_MAX_ROWS + 20,
                now_unix()
            ))?;
        }
        store.audit(None, "retention", "success", "trigger")?;
        let connection = store.lock()?;
        let count: i64 =
            connection.query_row("SELECT COUNT(*) FROM audit_events", [], |row| row.get(0))?;
        assert_eq!(count, AUDIT_MAX_ROWS);
        Ok(())
    }
}
