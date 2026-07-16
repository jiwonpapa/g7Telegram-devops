# G7Telegram DevOps

Ubuntu VPS 한 대와 Telegram Bot 한 개를 직접 연결하는 로컬 우선 서버 관리 Agent입니다.

## 제품 경계

- 중앙 관제 서버와 중앙 데이터베이스를 사용하지 않습니다.
- VPS에서 Telegram Bot API로 outbound HTTPS long polling만 수행합니다.
- 일반 웹서비스와 systemd 서비스 상태를 우선 지원합니다.
- GnuBoard 7이 설치된 경우 관련 queue, scheduler, Reverb 등 발견된 서비스를 함께 분류합니다.
- GnuBoard 5/7 코어를 수정하거나 별도 웹 endpoint를 설치하지 않습니다.
- 임의 shell, SQL, 복원, 삭제, 방화벽 및 SSH 설정 변경은 Telegram에서 제공하지 않습니다.

## 목표 환경

- Ubuntu 22.04 이상
- x86_64 우선, aarch64 후속
- 2GB RAM VPS
- Nginx 또는 Apache, PHP-FPM, MariaDB/MySQL, Redis, systemd 기반 서비스

## 배포 원칙

정식 산출물은 GitHub Release의 `.deb`입니다. 설치 스크립트는 Release에서 `.deb`와 checksum을 내려받아 검증한 뒤 `apt`로 설치합니다. 서버에서는 Rust를 빌드하지 않습니다.

## 개발 상태

현재는 단계별 구축 중입니다. 상세 범위와 완료 조건은 다음 문서를 기준으로 합니다.

- [제품 범위](docs/PRODUCT_SCOPE.md)
- [아키텍처](docs/ARCHITECTURE.md)
- [단계별 구현계획](docs/IMPLEMENTATION_PLAN.md)
- [보안 경계](docs/SECURITY.md)

## 로컬 개발

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

