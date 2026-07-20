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
mod ui;
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
    /// Telegram owner 연결·교체에 사용할 일회용 코드를 발급합니다.
    Pair {
        /// 연결 코드 유효시간입니다.
        #[arg(long, default_value_t = 300)]
        ttl_seconds: u64,
        /// 기존 owner를 유지하면서 새 owner가 코드를 사용할 때 교체합니다.
        #[arg(long)]
        replace: bool,
    },
    /// 등록된 Telegram owner와 대기 중인 승인을 제거합니다.
    Unpair {
        /// owner 제거 의도를 명시적으로 확인합니다.
        #[arg(long)]
        confirm: bool,
    },
    /// Bot token·서버 이름·서비스 allowlist·systemd를 대화형 설정합니다.
    Setup {
        /// Telegram 화면에 표시할 서버 이름입니다.
        #[arg(long)]
        server_name: Option<String>,
        /// HTTP 상태·응답시간·TLS를 검사할 대표 웹 URL입니다.
        #[arg(long)]
        web_url: Option<String>,
        /// 설정만 하고 systemd 서비스를 시작하지 않습니다.
        #[arg(long)]
        no_start: bool,
        /// 최초 연결코드 유효시간입니다.
        #[arg(long, default_value_t = 300)]
        pairing_ttl_seconds: u64,
        /// 연결코드 출력 후 Telegram owner 등록을 기다리지 않습니다.
        #[arg(long)]
        no_wait_for_pairing: bool,
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
            println!(
                "Monitoring: {}s interval, {} consecutive confirmations",
                config.monitor_interval_seconds, config.incident_confirmation_count
            );
            println!(
                "Thresholds: CPU {:.1}%, Load {:.2}/CPU, Memory {:.1}%, Swap {:.1}% with memory pressure, Disk {:.1}%",
                config.cpu_warning_percent,
                config.load_warning_per_cpu,
                config.memory_warning_percent,
                config.swap_warning_percent,
                config.disk_warning_percent
            );
            Ok(())
        }
        Command::Pair {
            ttl_seconds,
            replace,
        } => {
            config.validate()?;
            anyhow::ensure!(
                (60..=900).contains(&ttl_seconds),
                "ttl_seconds는 60~900이어야 합니다"
            );
            let store = storage::Store::open(&config.state_database)?;
            let owner_exists = store.owner()?.is_some();
            anyhow::ensure!(
                !owner_exists || replace,
                "owner가 이미 등록되어 있습니다. 교체하려면 root로 pair --replace를 실행하십시오"
            );
            if replace {
                ensure_root("owner 교체")?;
            }
            let code = if replace {
                store.create_owner_replacement_code(ttl_seconds)?
            } else {
                store.create_pairing_code(ttl_seconds)?
            };
            println!("Telegram에서 다음 연결 코드를 보내십시오: {code}");
            println!("유효시간: {ttl_seconds}초");
            setup::print_pairing_response_guidance();
            if replace {
                println!("새 owner가 코드를 사용할 때까지 기존 owner는 유지됩니다.");
            }
            Ok(())
        }
        Command::Unpair { confirm } => {
            config.validate()?;
            ensure_root("owner 해제")?;
            anyhow::ensure!(confirm, "owner 해제에는 --confirm이 필요합니다");
            let store = storage::Store::open(&config.state_database)?;
            if store.clear_owner()? {
                println!("Telegram owner와 대기 중인 승인을 제거했습니다.");
            } else {
                println!("등록된 Telegram owner가 없습니다.");
            }
            Ok(())
        }
        Command::Setup {
            server_name,
            web_url,
            no_start,
            pairing_ttl_seconds,
            no_wait_for_pairing,
        } => {
            setup::run(
                &cli.config,
                &mut config,
                server_name.as_deref(),
                web_url.as_deref(),
                no_start,
                pairing_ttl_seconds,
                no_wait_for_pairing,
            )
            .await
        }
    }
}

fn ensure_root(operation: &str) -> anyhow::Result<()> {
    let output = std::process::Command::new("id")
        .arg("-u")
        .output()
        .with_context(|| format!("{operation} 권한 확인 실패"))?;
    anyhow::ensure!(output.status.success(), "{operation} 권한 확인 실패");
    anyhow::ensure!(
        String::from_utf8_lossy(&output.stdout).trim() == "0",
        "{operation}은 root 권한이 필요합니다"
    );
    Ok(())
}
