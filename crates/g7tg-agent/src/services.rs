//! systemd service discovery와 안정적인 상태 parsing입니다.

use std::{collections::BTreeMap, time::Duration};

use anyhow::{Context, anyhow};
use g7tg_core::{ServiceCategory, ServiceStatus};
use sha2::{Digest, Sha256};
use tokio::process::Command;

/// 웹서비스 관련 unit을 자동 발견하고 추가 unit과 병합합니다.
pub async fn discover(extra_units: &[String]) -> anyhow::Result<Vec<ServiceStatus>> {
    let output = run_systemctl(&[
        "list-units",
        "--type=service",
        "--all",
        "--no-legend",
        "--no-pager",
        "--plain",
    ])
    .await?;
    let mut services = parse_list_units(&output, extra_units);
    for unit in extra_units {
        if !services.contains_key(unit) {
            let status = show_unit(unit, ServiceCategory::Extra).await?;
            services.insert(unit.clone(), status);
        }
    }
    let mut services: Vec<_> = services.into_values().collect();
    services.sort_by(|left, right| {
        left.category
            .cmp(&right.category)
            .then_with(|| left.unit.cmp(&right.unit))
    });
    Ok(services)
}

/// callback에 노출할 고정 길이 비가역 key입니다.
#[must_use]
pub fn service_key(unit: &str) -> String {
    let digest = Sha256::digest(unit.as_bytes());
    digest
        .iter()
        .take(8)
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// systemd unit 인자로 허용할 안전한 문법입니다.
#[must_use]
pub fn valid_unit_name(unit: &str) -> bool {
    unit.ends_with(".service")
        && (1..=128).contains(&unit.len())
        && unit.chars().all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.' | '@' | ':')
        })
}

fn parse_list_units(output: &str, extra_units: &[String]) -> BTreeMap<String, ServiceStatus> {
    output
        .lines()
        .filter_map(|line| parse_unit_line(line, extra_units))
        .map(|service| (service.unit.clone(), service))
        .collect()
}

fn parse_unit_line(line: &str, extra_units: &[String]) -> Option<ServiceStatus> {
    let mut fields = line.split_whitespace();
    let unit = fields.next()?;
    let load_state = fields.next()?;
    let active_state = fields.next()?;
    let sub_state = fields.next()?;
    if !valid_unit_name(unit) {
        return None;
    }
    let category = classify(unit).or_else(|| {
        extra_units
            .iter()
            .any(|extra| extra == unit)
            .then_some(ServiceCategory::Extra)
    })?;
    Some(ServiceStatus {
        unit: unit.to_owned(),
        description: fields.collect::<Vec<_>>().join(" "),
        category,
        load_state: load_state.to_owned(),
        active_state: active_state.to_owned(),
        sub_state: sub_state.to_owned(),
    })
}

fn classify(unit: &str) -> Option<ServiceCategory> {
    let unit = unit.to_ascii_lowercase();
    if unit == "nginx.service"
        || unit == "apache2.service"
        || unit == "httpd.service"
        || unit == "caddy.service"
    {
        Some(ServiceCategory::Web)
    } else if unit.starts_with("php") && unit.contains("fpm") {
        Some(ServiceCategory::Php)
    } else if unit.starts_with("mariadb")
        || unit.starts_with("mysql")
        || unit.starts_with("postgresql")
    {
        Some(ServiceCategory::Database)
    } else if unit.starts_with("redis") || unit.starts_with("memcached") {
        Some(ServiceCategory::Cache)
    } else if unit.contains("gnuboard")
        || unit.contains("g7-")
        || unit.contains("reverb")
        || unit.contains("horizon")
        || unit.contains("laravel-queue")
        || unit.contains("laravel-scheduler")
    {
        Some(ServiceCategory::Application)
    } else {
        None
    }
}

async fn show_unit(unit: &str, category: ServiceCategory) -> anyhow::Result<ServiceStatus> {
    let output = run_systemctl(&[
        "show",
        unit,
        "--no-pager",
        "--property=Id",
        "--property=Description",
        "--property=LoadState",
        "--property=ActiveState",
        "--property=SubState",
    ])
    .await?;
    let properties: BTreeMap<_, _> = output
        .lines()
        .filter_map(|line| line.split_once('='))
        .collect();
    Ok(ServiceStatus {
        unit: properties.get("Id").copied().unwrap_or(unit).to_owned(),
        description: properties
            .get("Description")
            .copied()
            .unwrap_or_default()
            .to_owned(),
        category,
        load_state: properties
            .get("LoadState")
            .copied()
            .unwrap_or("unknown")
            .to_owned(),
        active_state: properties
            .get("ActiveState")
            .copied()
            .unwrap_or("unknown")
            .to_owned(),
        sub_state: properties
            .get("SubState")
            .copied()
            .unwrap_or("unknown")
            .to_owned(),
    })
}

async fn run_systemctl(arguments: &[&str]) -> anyhow::Result<String> {
    let child = Command::new("systemctl")
        .args(arguments)
        .env("SYSTEMD_COLORS", "0")
        .env("SYSTEMD_PAGER", "cat")
        .output();
    let output = tokio::time::timeout(Duration::from_secs(5), child)
        .await
        .map_err(|_| anyhow!("systemctl timeout"))?
        .context("systemctl 실행 실패")?;
    if !output.status.success() {
        return Err(anyhow!("systemctl 실패: exit={}", output.status));
    }
    String::from_utf8(output.stdout).context("systemctl UTF-8 parse 실패")
}

#[cfg(test)]
mod tests {
    use g7tg_core::ServiceCategory;

    use super::{parse_list_units, service_key, valid_unit_name};

    const FIXTURE: &str = r#"
nginx.service loaded active running A high performance web server
php8.2-fpm.service loaded active running The PHP 8.2 FastCGI Process Manager
mariadb.service loaded failed failed MariaDB database server
g7-reverb.service loaded active running G7 Reverb
ssh.service loaded active running OpenBSD Secure Shell server
"#;

    #[test]
    fn discovery_keeps_only_managed_categories() {
        let services = parse_list_units(FIXTURE, &[]);
        assert_eq!(services.len(), 4);
        assert!(!services.contains_key("ssh.service"));
        assert_eq!(
            services
                .get("g7-reverb.service")
                .map(|value| value.category),
            Some(ServiceCategory::Application)
        );
    }

    #[test]
    fn unit_names_and_keys_are_bounded() {
        assert!(valid_unit_name("php8.2-fpm.service"));
        assert!(!valid_unit_name("../../etc/passwd.service"));
        assert!(!valid_unit_name("nginx.timer"));
        assert_eq!(service_key("nginx.service").len(), 16);
    }
}
