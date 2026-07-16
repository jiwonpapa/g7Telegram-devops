//! root-owned allowlist executor를 통한 서비스 동작입니다.

use std::{path::Path, time::Duration};

use anyhow::{Context, anyhow, ensure};
use g7tg_core::ServiceAction;
use tokio::process::Command;

/// executor의 root allowlist에 unit이 있는지 읽기 전용 확인합니다.
pub async fn can_manage(executor: &str, unit: &str) -> anyhow::Result<bool> {
    ensure!(
        crate::services::valid_unit_name(unit),
        "잘못된 systemd unit입니다"
    );
    if !Path::new(executor).is_file() {
        return Ok(false);
    }
    let output = tokio::time::timeout(
        Duration::from_secs(3),
        Command::new(executor).arg("check").arg(unit).output(),
    )
    .await
    .map_err(|_| anyhow!("action executor check timeout"))?
    .context("action executor check 실패")?;
    Ok(output.status.success())
}

/// sudo를 통해 이미 승인된 정확한 동작만 실행합니다.
pub async fn execute(executor: &str, action: ServiceAction, unit: &str) -> anyhow::Result<()> {
    ensure!(
        crate::services::valid_unit_name(unit),
        "잘못된 systemd unit입니다"
    );
    ensure!(
        can_manage(executor, unit).await?,
        "root allowlist에 없는 서비스입니다"
    );
    let output = tokio::time::timeout(
        Duration::from_secs(45),
        Command::new("sudo")
            .arg("-n")
            .arg(executor)
            .arg(action.id())
            .arg(unit)
            .env("SYSTEMD_COLORS", "0")
            .env("SYSTEMD_PAGER", "cat")
            .output(),
    )
    .await
    .map_err(|_| anyhow!("서비스 {} timeout", action.label()))?
    .context("서비스 action executor 실행 실패")?;
    if !output.status.success() {
        return Err(anyhow!(
            "서비스 {} 실패: exit={}",
            action.label(),
            output.status
        ));
    }
    Ok(())
}
