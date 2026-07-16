//! G7Telegram DevOps Agent 실행 진입점입니다.

use std::path::PathBuf;

use anyhow::Context;
use clap::{Parser, Subcommand};
use tracing_subscriber::EnvFilter;

mod actions;
mod config;
mod menu;
mod monitor;
mod runtime;
mod services;
mod setup;
mod storage;
mod system;
mod telegram;
mod tls;
mod web;

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
    /// 최초 Telegram owner 연결에 사용할 일회용 코드를 발급합니다.
    Pair {
        /// 연결 코드 유효시간입니다.
        #[arg(long, default_value_t = 300)]
        ttl_seconds: u64,
    },
    /// Bot token·서버 이름·서비스 allowlist·systemd를 대화형 설정합니다.
    Setup {
        /// Telegram 화면에 표시할 서버 이름입니다.
        #[arg(long)]
        server_name: Option<String>,
        /// systemd LoadCredential이 읽을 root 전용 원본입니다.
        #[arg(long, default_value = "/etc/g7telegram-devops/secrets/bot-token")]
        secret_file: PathBuf,
        /// 설정만 하고 systemd 서비스를 시작하지 않습니다.
        #[arg(long)]
        no_start: bool,
        /// 최초 연결코드 유효시간입니다.
        #[arg(long, default_value_t = 300)]
        pairing_ttl_seconds: u64,
    },
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
    let mut config = config::AgentConfig::load(&cli.config)
        .with_context(|| format!("설정 파일을 읽지 못했습니다: {}", cli.config.display()))?;

    match cli.command {
        Command::Run => {
            config.validate()?;
            runtime::run(config).await
        }
        Command::Doctor => {
            config.validate()?;
            let store = storage::Store::open(&config.state_database)?;
            let owner = store.owner()?;
            let owner_state = if owner.is_some() {
                "paired"
            } else {
                "not-paired"
            };
            println!(
                "PASS: configuration for {} ({owner_state})",
                config.server_name
            );
            Ok(())
        }
        Command::Pair { ttl_seconds } => {
            config.validate()?;
            anyhow::ensure!(
                (60..=900).contains(&ttl_seconds),
                "ttl_seconds는 60~900이어야 합니다"
            );
            let store = storage::Store::open(&config.state_database)?;
            let code = store.create_pairing_code(ttl_seconds)?;
            println!("Telegram에서 다음 연결 코드를 보내십시오: {code}");
            println!("유효시간: {ttl_seconds}초");
            Ok(())
        }
        Command::Setup {
            server_name,
            secret_file,
            no_start,
            pairing_ttl_seconds,
        } => {
            setup::run(
                &cli.config,
                &mut config,
                server_name.as_deref(),
                &secret_file,
                no_start,
                pairing_ttl_seconds,
            )
            .await
        }
    }
}
