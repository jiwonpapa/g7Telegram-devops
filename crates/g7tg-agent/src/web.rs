//! 공개 URL의 본문을 해석하지 않는 최소 HTTP/TLS 검사입니다.

use std::time::{Duration, Instant};

use anyhow::Context;
use g7tg_core::WebCheckResult;
use reqwest::{Client, redirect::Policy};
use tokio::task::JoinSet;
use url::Url;

use crate::{config::WebCheckConfig, tls};

/// 설정된 endpoint를 병렬 검사합니다.
pub async fn check_all(checks: &[WebCheckConfig]) -> anyhow::Result<Vec<WebCheckResult>> {
    let client = Client::builder()
        .redirect(Policy::limited(3))
        .user_agent(concat!("g7telegram-devops/", env!("CARGO_PKG_VERSION")))
        .build()
        .context("web health HTTP client 생성 실패")?;
    let mut tasks = JoinSet::new();
    for check in checks.iter().cloned() {
        let client = client.clone();
        tasks.spawn(async move { check_one(&client, check).await });
    }
    let mut results = Vec::with_capacity(checks.len());
    while let Some(result) = tasks.join_next().await {
        results.push(result.context("web health task 실패")?);
    }
    results.sort_by(|left, right| left.name.cmp(&right.name));
    Ok(results)
}

async fn check_one(client: &Client, check: WebCheckConfig) -> WebCheckResult {
    let parsed_url = match Url::parse(&check.url) {
        Ok(url) => url,
        Err(_) => return error_result(check, "invalid_url"),
    };
    let started = Instant::now();
    let response = client
        .get(parsed_url.clone())
        .timeout(Duration::from_secs(check.timeout_seconds))
        .send()
        .await;
    let latency_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
    let response = match response {
        Ok(response) => response,
        Err(error) if error.is_timeout() => return error_result(check, "http_timeout"),
        Err(_) => return error_result(check, "http_failed"),
    };
    let status_code = response.status().as_u16();
    let status_healthy =
        (check.expected_status_min..=check.expected_status_max).contains(&status_code);
    let tls_days_remaining = if parsed_url.scheme() == "https" {
        match tls::days_remaining(&parsed_url, check.timeout_seconds).await {
            Ok(days) => Some(days),
            Err(_) => {
                return WebCheckResult {
                    name: check.name,
                    url: check.url,
                    status_code: Some(status_code),
                    latency_ms: Some(latency_ms),
                    tls_days_remaining: None,
                    healthy: false,
                    error_code: Some("tls_failed".to_owned()),
                };
            }
        }
    } else {
        None
    };
    let tls_healthy = tls_days_remaining.is_none_or(|days| days > 0);
    WebCheckResult {
        name: check.name,
        url: check.url,
        status_code: Some(status_code),
        latency_ms: Some(latency_ms),
        tls_days_remaining,
        healthy: status_healthy && tls_healthy,
        error_code: (!status_healthy).then(|| "unexpected_status".to_owned()),
    }
}

fn error_result(check: WebCheckConfig, error_code: &str) -> WebCheckResult {
    WebCheckResult {
        name: check.name,
        url: check.url,
        status_code: None,
        latency_ms: None,
        tls_days_remaining: None,
        healthy: false,
        error_code: Some(error_code.to_owned()),
    }
}
