//! Ubuntu package의 대화형 초기 설정입니다.

use std::{
    fs::{self, OpenOptions},
    io::Write,
    os::unix::fs::{OpenOptionsExt, PermissionsExt},
    path::{Path, PathBuf},
};

use anyhow::{Context, anyhow, ensure};
use tokio::process::Command;
use uuid::Uuid;

use crate::{config::AgentConfig, services, storage::Store, telegram::validate_token_shape};

const ALLOWLIST_PATH: &str = "/etc/g7telegram-devops/allowed-units";
const SECRET_PATH: &str = "/etc/g7telegram-devops/secrets/bot-token";

/// 비밀값과 안전한 자동 탐지 결과를 원자 저장하고 service를 시작합니다.
pub async fn run(
    config_path: &Path,
    config: &mut AgentConfig,
    server_name: Option<&str>,
    no_start: bool,
    pairing_ttl_seconds: u64,
) -> anyhow::Result<()> {
    ensure!(
        (60..=900).contains(&pairing_ttl_seconds),
        "pairing_ttl_seconds는 60~900이어야 합니다"
    );
    if let Some(server_name) = server_name {
        ensure!(
            !server_name.trim().is_empty(),
            "server name이 비어 있습니다"
        );
        ensure!(
            server_name.chars().count() <= 64,
            "server name은 최대 64자입니다"
        );
        config.server_name = server_name.trim().to_owned();
    }

    let token =
        rpassword::prompt_password("Telegram Bot token: ").context("Bot token 입력 실패")?;
    validate_token_shape(token.trim())?;
    write_secret(Path::new(SECRET_PATH), token.trim())?;

    let discovered = services::discover(&config.extra_service_units).await?;
    let mut units: Vec<_> = discovered
        .iter()
        .map(|service| service.unit.as_str())
        .collect();
    units.sort_unstable();
    units.dedup();
    write_atomic(
        Path::new(ALLOWLIST_PATH),
        &(units.join("\n") + if units.is_empty() { "" } else { "\n" }),
        0o644,
    )?;
    config.service_actions_enabled = !units.is_empty();
    config.validate()?;
    write_atomic(config_path, &config.to_toml()?, 0o640)?;
    set_owner_and_mode(config_path, "root:g7tg-agent", "0640")?;

    let store = Store::open(&config.state_database)?;
    let pairing_code = if store.owner()?.is_none() {
        Some(store.create_pairing_code(pairing_ttl_seconds)?)
    } else {
        None
    };
    drop(store);
    set_database_owner(Path::new(&config.state_database))?;

    if !no_start {
        run_checked("systemctl", &["daemon-reload"]).await?;
        run_checked("systemctl", &["enable", "--now", "g7tg-agent.service"]).await?;
    }

    println!("설정 완료: {}", config.server_name);
    if let Some(pairing_code) = pairing_code {
        println!("Telegram Bot에 다음 연결코드를 보내십시오: {pairing_code}");
        println!("연결코드 유효시간: {pairing_ttl_seconds}초");
    } else {
        println!("기존 Telegram owner 연결을 유지했습니다.");
    }
    if no_start {
        println!("서비스 시작: sudo systemctl enable --now g7tg-agent.service");
    }
    Ok(())
}

fn write_secret(path: &Path, token: &str) -> anyhow::Result<()> {
    let parent = path.parent().ok_or_else(|| anyhow!("secret parent 누락"))?;
    fs::create_dir_all(parent).context("secret 디렉터리 생성 실패")?;
    fs::set_permissions(parent, fs::Permissions::from_mode(0o700))
        .context("secret 디렉터리 권한 설정 실패")?;
    write_atomic(path, &(token.to_owned() + "\n"), 0o600)
}

fn write_atomic(path: &Path, body: &str, mode: u32) -> anyhow::Result<()> {
    let parent = path.parent().ok_or_else(|| anyhow!("파일 parent 누락"))?;
    fs::create_dir_all(parent).context("파일 디렉터리 생성 실패")?;
    let temporary = temporary_path(path);
    let mut file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .mode(mode)
        .open(&temporary)
        .context("임시 파일 생성 실패")?;
    file.write_all(body.as_bytes())
        .context("임시 파일 write 실패")?;
    file.sync_all().context("임시 파일 sync 실패")?;
    fs::set_permissions(&temporary, fs::Permissions::from_mode(mode))
        .context("임시 파일 권한 설정 실패")?;
    fs::rename(&temporary, path).context("파일 원자 교체 실패")?;
    Ok(())
}

fn temporary_path(path: &Path) -> PathBuf {
    let mut name = path.as_os_str().to_os_string();
    name.push(format!(".tmp.{}", Uuid::new_v4().simple()));
    PathBuf::from(name)
}

fn set_owner_and_mode(path: &Path, owner: &str, mode: &str) -> anyhow::Result<()> {
    let status = std::process::Command::new("chown")
        .arg(owner)
        .arg(path)
        .status()
        .context("chown 실행 실패")?;
    ensure!(status.success(), "chown 실패: {path:?}");
    let status = std::process::Command::new("chmod")
        .arg(mode)
        .arg(path)
        .status()
        .context("chmod 실행 실패")?;
    ensure!(status.success(), "chmod 실패: {path:?}");
    Ok(())
}

fn set_database_owner(path: &Path) -> anyhow::Result<()> {
    set_owner_and_mode(path, "g7tg-agent:g7tg-agent", "0640")?;
    for suffix in ["-wal", "-shm"] {
        let mut sidecar = path.as_os_str().to_os_string();
        sidecar.push(suffix);
        let sidecar = PathBuf::from(sidecar);
        if sidecar.exists() {
            set_owner_and_mode(&sidecar, "g7tg-agent:g7tg-agent", "0640")?;
        }
    }
    Ok(())
}

async fn run_checked(program: &str, arguments: &[&str]) -> anyhow::Result<()> {
    let status = Command::new(program)
        .args(arguments)
        .status()
        .await
        .with_context(|| format!("{program} 실행 실패"))?;
    ensure!(status.success(), "{program} 실패: {status}");
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::os::unix::fs::PermissionsExt;

    use tempfile::tempdir;

    use super::write_atomic;

    #[test]
    fn atomic_writer_sets_requested_mode() -> anyhow::Result<()> {
        let directory = tempdir()?;
        let path = directory.path().join("secret");
        write_atomic(&path, "value\n", 0o600)?;
        assert_eq!(std::fs::read_to_string(&path)?, "value\n");
        assert_eq!(std::fs::metadata(path)?.permissions().mode() & 0o777, 0o600);
        Ok(())
    }
}
