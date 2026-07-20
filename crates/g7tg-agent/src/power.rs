//! 명시적으로 허용된 서버 재시작 helper 호출과 boot ID 확인입니다.

use std::{fs, path::Path, time::Duration};

use anyhow::{Context, anyhow, ensure};
use tokio::process::Command;

const BOOT_ID_PATH: &str = "/proc/sys/kernel/random/boot_id";

/// root helper가 서버 재시작을 로컬에서 허용하는지 확인합니다.
pub async fn can_reboot(executor: &str) -> anyhow::Result<bool> {
    if !Path::new(executor).is_file() {
        return Ok(false);
    }
    let output = tokio::time::timeout(
        Duration::from_secs(3),
        Command::new(executor).arg("check-reboot").output(),
    )
    .await
    .map_err(|_| anyhow!("server reboot executor check timeout"))?
    .context("server reboot executor check 실패")?;
    Ok(output.status.success())
}

/// sudo를 통해 서버 전체 재시작만 요청합니다.
pub async fn execute_reboot(executor: &str) -> anyhow::Result<()> {
    ensure!(
        can_reboot(executor).await?,
        "서버 재시작이 로컬 setup에서 허용되지 않았습니다"
    );
    let output = tokio::time::timeout(
        Duration::from_secs(15),
        Command::new("sudo")
            .arg("-n")
            .arg(executor)
            .arg("reboot")
            .env("SYSTEMD_COLORS", "0")
            .env("SYSTEMD_PAGER", "cat")
            .output(),
    )
    .await
    .map_err(|_| anyhow!("서버 재시작 요청 timeout"))?
    .context("서버 재시작 helper 실행 실패")?;
    ensure!(
        output.status.success(),
        "서버 재시작 helper 실패: exit={}",
        output.status
    );
    Ok(())
}

/// 현재 Linux boot ID를 반환합니다.
pub fn current_boot_id() -> anyhow::Result<String> {
    read_boot_id(Path::new(BOOT_ID_PATH))
}

fn read_boot_id(path: &Path) -> anyhow::Result<String> {
    let boot_id = fs::read_to_string(path)
        .with_context(|| format!("boot ID read 실패: {}", path.display()))?;
    let boot_id = boot_id.trim();
    ensure!(
        !boot_id.is_empty()
            && boot_id.len() <= 64
            && boot_id
                .chars()
                .all(|character| character.is_ascii_hexdigit() || character == '-'),
        "boot ID 형식이 올바르지 않습니다"
    );
    Ok(boot_id.to_owned())
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::tempdir;

    use super::read_boot_id;

    #[test]
    fn boot_id_is_trimmed_and_fail_closed() -> anyhow::Result<()> {
        let directory = tempdir()?;
        let path = directory.path().join("boot-id");
        fs::write(&path, "01234567-89ab-cdef-0123-456789abcdef\n")?;
        assert_eq!(read_boot_id(&path)?, "01234567-89ab-cdef-0123-456789abcdef");
        fs::write(&path, "../../unsafe\n")?;
        assert!(read_boot_id(&path).is_err());
        Ok(())
    }
}
