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

```bash
curl -fsSL https://raw.githubusercontent.com/jiwonpapa/g7Telegram-devops/main/scripts/install.sh | sudo sh
```

최초 설치에서는 바로 초기설정 시작 여부를 묻습니다. 서버 이름과 Bot token, 선택형 대표 웹 URL을 입력하면 Telegram API로 token을 검증하고, 발견한 관리 대상 서비스를 exact allowlist로 저장합니다. 웹 URL을 입력하면 HTTP 상태·응답시간·TLS 만료를 검사합니다. 출력된 일회용 연결코드를 Bot 개인채팅으로 보내면 실제 발신자의 변하지 않는 숫자 user/chat ID가 자동 저장됩니다. Telegram ID를 문자열이나 수동 입력값으로 신뢰하지 않습니다.

초기설정을 건너뛰었거나 터미널이 없는 자동 설치에서는 나중에 실행합니다.

```bash
sudo g7tg setup
```

## 현재 상태

초기 `v0.2.x` 범위는 메뉴형 조회, 서비스 분류, 승인형 재시작, 장애 알림, 대화형 초기설정과 `.deb` 배포입니다. 자동 검증과 실제 VPS 검증은 증명 수준을 분리합니다.

- [제품 범위](docs/PRODUCT_SCOPE.md)
- [아키텍처](docs/ARCHITECTURE.md)
- [단계별 구현계획](docs/IMPLEMENTATION_PLAN.md)
- [보안 경계](docs/SECURITY.md)
- [설치와 운영](docs/OPERATIONS.md)
- [검증 기준](docs/VERIFICATION.md)

## 로컬 개발

```bash
scripts/check.sh
```
