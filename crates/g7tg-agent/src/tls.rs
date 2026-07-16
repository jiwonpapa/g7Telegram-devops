//! HTTPS endpoint의 인증서 만료를 독립적으로 확인합니다.

use std::{sync::Arc, time::Duration};

use anyhow::{Context, anyhow};
use rustls::{ClientConfig, RootCertStore, pki_types::ServerName};
use time::OffsetDateTime;
use tokio::net::TcpStream;
use tokio_rustls::TlsConnector;
use url::Url;
use x509_parser::parse_x509_certificate;

/// 공개 HTTPS URL의 leaf 인증서 만료까지 남은 일수입니다.
pub async fn days_remaining(url: &Url, timeout_seconds: u64) -> anyhow::Result<i64> {
    let host = url.host_str().ok_or_else(|| anyhow!("TLS host 누락"))?;
    let port = url
        .port_or_known_default()
        .ok_or_else(|| anyhow!("TLS port 누락"))?;
    let tcp = tokio::time::timeout(
        Duration::from_secs(timeout_seconds),
        TcpStream::connect((host, port)),
    )
    .await
    .map_err(|_| anyhow!("TLS TCP timeout"))?
    .context("TLS TCP 연결 실패")?;

    let mut roots = RootCertStore::empty();
    roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
    let config = ClientConfig::builder()
        .with_root_certificates(roots)
        .with_no_client_auth();
    let connector = TlsConnector::from(Arc::new(config));
    let server_name = ServerName::try_from(host.to_owned()).context("TLS server name 실패")?;
    let stream = tokio::time::timeout(
        Duration::from_secs(timeout_seconds),
        connector.connect(server_name, tcp),
    )
    .await
    .map_err(|_| anyhow!("TLS handshake timeout"))?
    .context("TLS handshake 실패")?;
    let certificates = stream
        .get_ref()
        .1
        .peer_certificates()
        .ok_or_else(|| anyhow!("TLS peer certificate 누락"))?;
    let leaf = certificates
        .first()
        .ok_or_else(|| anyhow!("TLS leaf certificate 누락"))?;
    let (_, certificate) = parse_x509_certificate(leaf.as_ref()).context("TLS X.509 parse 실패")?;
    let expires_at = certificate.validity().not_after.timestamp();
    Ok((expires_at - OffsetDateTime::now_utc().unix_timestamp()) / 86_400)
}
