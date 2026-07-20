//! Ubuntu package의 대화형 초기 설정입니다.

use std::{
    fs::{self, OpenOptions},
    io::{self, Write},
    os::unix::fs::{OpenOptionsExt, PermissionsExt},
    path::{Path, PathBuf},
    time::Duration,
};

use anyhow::{Context, anyhow, ensure};
use tokio::process::Command;
use url::Url;
use uuid::Uuid;

use crate::{
    config::{AgentConfig, WebCheckConfig},
    services,
    storage::{Owner, Store},
    telegram::TelegramClient,
};

const ALLOWLIST_PATH: &str = "/etc/g7telegram-devops/allowed-units";
const REBOOT_SENTINEL_PATH: &str = "/etc/g7telegram-devops/allow-server-reboot";
const SECRET_PATH: &str = "/etc/g7telegram-devops/secrets/bot-token";
const PAIRING_RESPONSE_GUIDANCE: [&str; 2] = [
    "Telegram 응답은 Agent 시작과 네트워크 상태에 따라 수초 걸릴 수 있습니다.",
    "10초 뒤에도 답장이 없으면 같은 연결코드를 한 번 다시 보내십시오.",
];

/// 연결코드 전송 뒤 정상적인 대기와 안전한 재시도 기준을 안내합니다.
pub(crate) fn print_pairing_response_guidance() {
    for line in PAIRING_RESPONSE_GUIDANCE {
        println!("{line}");
    }
}

/// 비밀값과 안전한 자동 탐지 결과를 원자 저장하고 service를 시작합니다.
pub async fn run(
    config_path: &Path,
    config: &mut AgentConfig,
    server_name: Option<&str>,
    web_url: Option<&str>,
    no_start: bool,
    pairing_ttl_seconds: u64,
    no_wait_for_pairing: bool,
) -> anyhow::Result<()> {
    ensure!(
        (60..=900).contains(&pairing_ttl_seconds),
        "pairing_ttl_seconds는 60~900이어야 합니다"
    );
    config.server_name = match server_name {
        Some(server_name) => validated_server_name(server_name)?,
        None => prompt_server_name(&default_server_name(config))?,
    };

    let token = rpassword::prompt_password("Telegram Bot token (숨김 입력): ")
        .context("Bot token 입력 실패")?;
    let telegram = TelegramClient::from_token(&token)?;
    let bot = telegram
        .get_me()
        .await
        .context("Bot token Telegram 검증 실패")?;
    ensure!(bot.is_bot, "입력한 token이 Telegram Bot 계정이 아닙니다");
    println!("Telegram Bot 확인: {} (ID {})", bot.first_name, bot.id);
    configure_primary_web_check(config, web_url)?;
    config.server_reboot_enabled = prompt_server_reboot(config.server_reboot_enabled)?;

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
    configure_reboot_sentinel(config.server_reboot_enabled)?;

    let store = Store::open(&config.state_database)?;
    let existing_owner = store.owner()?;
    let pairing_code = if existing_owner.is_none() {
        Some(store.create_pairing_code(pairing_ttl_seconds)?)
    } else {
        None
    };
    set_database_owner(Path::new(&config.state_database))?;
    write_secret(Path::new(SECRET_PATH), token.trim())?;

    if !no_start {
        run_checked("systemctl", &["daemon-reload"]).await?;
        run_checked("systemctl", &["enable", "g7tg-agent.service"]).await?;
        run_checked("systemctl", &["restart", "g7tg-agent.service"]).await?;
        tokio::time::sleep(Duration::from_secs(2)).await;
        run_checked("systemctl", &["is-active", "--quiet", "g7tg-agent.service"]).await?;
    }

    println!("설정 완료: {}", config.server_name);
    if let Some(check) = config.web_checks.first() {
        println!("웹 상태 검사: {}", check.url);
    } else {
        println!("웹 상태 검사: 미설정");
    }
    println!(
        "Telegram 서버 재시작: {}",
        if config.server_reboot_enabled {
            "사용"
        } else {
            "사용 안 함"
        }
    );
    if let Some(pairing_code) = pairing_code {
        println!("Telegram Bot에 다음 연결코드를 보내십시오: {pairing_code}");
        println!("연결코드 유효시간: {pairing_ttl_seconds}초");
        print_pairing_response_guidance();
        if !no_start && !no_wait_for_pairing {
            println!("Telegram owner 연결을 기다립니다...");
            match wait_for_owner(&store, pairing_ttl_seconds).await? {
                Some(owner) => println!(
                    "Telegram owner 연결 완료: user ID {}, chat ID {}",
                    owner.user_id, owner.chat_id
                ),
                None => {
                    println!("연결 대기시간이 끝났습니다. Agent는 계속 실행 중입니다.");
                    println!("필요하면 sudo -u g7tg-agent g7tg pair 로 새 코드를 발급하십시오.");
                }
            }
        } else if no_start {
            println!("서비스 시작 후 유효시간 안에 연결코드를 전송하십시오.");
        }
    } else {
        let owner = existing_owner.ok_or_else(|| anyhow!("owner 상태가 사라졌습니다"))?;
        println!(
            "기존 Telegram owner를 유지했습니다: user ID {}",
            owner.user_id
        );
    }
    if no_start {
        println!("서비스 시작: sudo systemctl enable --now g7tg-agent.service");
    }
    Ok(())
}

fn configure_primary_web_check(
    config: &mut AgentConfig,
    requested_url: Option<&str>,
) -> anyhow::Result<()> {
    let existing_url = config.web_checks.first().map(|check| check.url.as_str());
    let selected = match requested_url {
        Some(url) => Some(normalized_web_url(url)?),
        None => prompt_web_url(existing_url)?,
    };
    let Some(url) = selected else {
        return Ok(());
    };
    let name = config
        .web_checks
        .first()
        .map_or_else(|| "대표 사이트".to_owned(), |check| check.name.clone());
    let check = WebCheckConfig {
        name,
        url,
        expected_status_min: 200,
        expected_status_max: 399,
        timeout_seconds: 5,
        tls_warning_days: 14,
    };
    if let Some(existing) = config.web_checks.first_mut() {
        *existing = check;
    } else {
        config.web_checks.push(check);
    }
    Ok(())
}

fn prompt_web_url(existing_url: Option<&str>) -> anyhow::Result<Option<String>> {
    match existing_url {
        Some(url) => print!("웹 상태 확인 주소 (Enter=기존값 유지) [{url}]: "),
        None => print!("웹 상태 확인 주소 (선택, Enter=건너뜀): "),
    }
    io::stdout().flush().context("web URL prompt 출력 실패")?;
    let mut input = String::new();
    io::stdin()
        .read_line(&mut input)
        .context("web URL 입력 실패")?;
    if input.trim().is_empty() {
        return existing_url.map(normalized_web_url).transpose();
    }
    normalized_web_url(&input).map(Some)
}

fn prompt_server_reboot(existing_enabled: bool) -> anyhow::Result<bool> {
    println!("서버 재시작은 웹서비스와 Telegram Agent를 함께 중단합니다.");
    if existing_enabled {
        print!("Telegram 원격 서버 재시작 기능을 계속 사용하시겠습니까? [Y/n] ");
    } else {
        print!("Telegram 원격 서버 재시작 기능을 사용하시겠습니까? [y/N] ");
    }
    io::stdout()
        .flush()
        .context("server reboot prompt 출력 실패")?;
    let mut input = String::new();
    io::stdin()
        .read_line(&mut input)
        .context("server reboot 입력 실패")?;
    parse_yes_no(&input, existing_enabled)
}

fn parse_yes_no(input: &str, default: bool) -> anyhow::Result<bool> {
    match input.trim().to_ascii_lowercase().as_str() {
        "" => Ok(default),
        "y" | "yes" => Ok(true),
        "n" | "no" => Ok(false),
        _ => Err(anyhow!("y 또는 n을 입력하십시오")),
    }
}

fn configure_reboot_sentinel(enabled: bool) -> anyhow::Result<()> {
    let path = Path::new(REBOOT_SENTINEL_PATH);
    if enabled {
        write_atomic(path, "enabled\n", 0o640)?;
        set_owner_and_mode(path, "root:g7tg-agent", "0640")?;
    } else if path.exists() {
        fs::remove_file(path).context("server reboot 허용 파일 제거 실패")?;
    }
    Ok(())
}

fn normalized_web_url(input: &str) -> anyhow::Result<String> {
    let input = input.trim();
    ensure!(!input.is_empty(), "web URL이 비어 있습니다");
    let candidate = if input.contains("://") {
        input.to_owned()
    } else {
        format!("https://{input}")
    };
    let mut url = Url::parse(&candidate).context("web URL parse 실패")?;
    ensure!(
        matches!(url.scheme(), "http" | "https"),
        "web URL은 HTTP(S)만 허용합니다"
    );
    ensure!(url.host_str().is_some(), "web URL host가 없습니다");
    ensure!(
        url.username().is_empty() && url.password().is_none(),
        "web URL credential을 허용하지 않습니다"
    );
    ensure!(
        url.query().is_none() && url.fragment().is_none(),
        "web URL query와 fragment를 허용하지 않습니다"
    );
    if url.path().is_empty() {
        url.set_path("/");
    }
    Ok(url.to_string())
}

fn validated_server_name(server_name: &str) -> anyhow::Result<String> {
    let server_name = server_name.trim();
    ensure!(!server_name.is_empty(), "server name이 비어 있습니다");
    ensure!(
        server_name.chars().count() <= 64,
        "server name은 최대 64자입니다"
    );
    ensure!(
        server_name.chars().all(|character| !character.is_control()),
        "server name에 제어문자를 사용할 수 없습니다"
    );
    Ok(server_name.to_owned())
}

fn default_server_name(config: &AgentConfig) -> String {
    std::process::Command::new("hostname")
        .output()
        .ok()
        .filter(|output| output.status.success())
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .and_then(|hostname| validated_server_name(&hostname).ok())
        .unwrap_or_else(|| config.server_name.clone())
}

fn prompt_server_name(default: &str) -> anyhow::Result<String> {
    print!("서버 이름 [{default}]: ");
    io::stdout()
        .flush()
        .context("server name prompt 출력 실패")?;
    let mut input = String::new();
    io::stdin()
        .read_line(&mut input)
        .context("server name 입력 실패")?;
    if input.trim().is_empty() {
        validated_server_name(default)
    } else {
        validated_server_name(&input)
    }
}

async fn wait_for_owner(store: &Store, timeout_seconds: u64) -> anyhow::Result<Option<Owner>> {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(timeout_seconds);
    loop {
        if let Some(owner) = store.owner()? {
            return Ok(Some(owner));
        }
        if tokio::time::Instant::now() >= deadline {
            return Ok(None);
        }
        tokio::time::sleep(Duration::from_secs(2)).await;
    }
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

    use super::{
        normalized_web_url, parse_yes_no, validated_server_name, wait_for_owner, write_atomic,
    };
    use crate::storage::Store;

    #[test]
    fn atomic_writer_sets_requested_mode() -> anyhow::Result<()> {
        let directory = tempdir()?;
        let path = directory.path().join("secret");
        write_atomic(&path, "value\n", 0o600)?;
        assert_eq!(std::fs::read_to_string(&path)?, "value\n");
        assert_eq!(std::fs::metadata(path)?.permissions().mode() & 0o777, 0o600);
        Ok(())
    }

    #[test]
    fn server_name_is_trimmed_and_fail_closed() -> anyhow::Result<()> {
        assert_eq!(validated_server_name(" web-01 ")?, "web-01");
        assert!(validated_server_name("\n").is_err());
        assert!(validated_server_name(&"x".repeat(65)).is_err());
        assert!(validated_server_name("web\u{0000}01").is_err());
        Ok(())
    }

    #[test]
    fn web_url_defaults_to_https_and_rejects_secrets() -> anyhow::Result<()> {
        assert_eq!(normalized_web_url("example.com")?, "https://example.com/");
        assert_eq!(
            normalized_web_url("http://example.com/health")?,
            "http://example.com/health"
        );
        assert!(normalized_web_url("ftp://example.com/").is_err());
        assert!(normalized_web_url("https://user:pass@example.com/").is_err());
        assert!(normalized_web_url("https://example.com/?token=secret").is_err());
        Ok(())
    }

    #[test]
    fn reboot_prompt_defaults_are_explicit_and_fail_closed() -> anyhow::Result<()> {
        assert!(!parse_yes_no("", false)?);
        assert!(parse_yes_no("", true)?);
        assert!(parse_yes_no("Y", false)?);
        assert!(!parse_yes_no("no", true)?);
        assert!(parse_yes_no("maybe", false).is_err());
        Ok(())
    }

    #[tokio::test]
    async fn pairing_wait_reads_telegram_numeric_identity() -> anyhow::Result<()> {
        let directory = tempdir()?;
        let store = Store::open(directory.path().join("state.sqlite3"))?;
        let code = store.create_pairing_code(60)?;
        let writer = store.clone();
        let task = tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
            writer.consume_pairing_code(&code, 987_654_321, 987_654_321)
        });

        let owner = wait_for_owner(&store, 3)
            .await?
            .ok_or_else(|| anyhow::anyhow!("owner 연결 누락"))?;
        assert!(task.await??);
        assert_eq!(owner.user_id, 987_654_321);
        assert_eq!(owner.chat_id, 987_654_321);
        Ok(())
    }
}
