//! G7Telegram DevOps Agent 실행 진입점입니다.

use std::path::PathBuf;

use anyhow::Context;
use clap::{Parser, Subcommand};
use tracing_subscriber::EnvFilter;

mod config;

/// G7Telegram DevOps Agent CLI입니다.
#[derive(Debug, Parser)]
#[command(name = "g7tg", version, about = "Telegram 기반 1:1 VPS 관리 Agent")]
struct Cli {
    /// 설정 파일 경로입니다.
    #[arg(long, default_value = "/etc/g7telegram-devops/agent.toml")]
    config: PathBuf,

    /// 실행할 명령입니다.
    #[command(subcommand)]
    command: Command,
}

/// Agent 관리 명령입니다.
#[derive(Debug, Subcommand)]
enum Command {
    /// 설정을 검증하고 Agent를 실행합니다.
    Run,
    /// 설정과 로컬 실행환경을 읽기 전용으로 검사합니다.
    Doctor,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .json()
        .init();

    let cli = Cli::parse();
    let config = config::AgentConfig::load(&cli.config)
        .with_context(|| format!("설정 파일을 읽지 못했습니다: {}", cli.config.display()))?;

    match cli.command {
        Command::Run => {
            tracing::info!(server = %config.server_name, "Agent 기본선이 준비되었습니다");
            anyhow::bail!("배치 1 Telegram runtime은 아직 활성화되지 않았습니다")
        }
        Command::Doctor => {
            config.validate()?;
            println!("PASS: configuration for {}", config.server_name);
            Ok(())
        }
    }
}
