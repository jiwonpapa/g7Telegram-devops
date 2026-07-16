# 단계별 구현계획

각 배치는 독립적으로 테스트·배포할 수 있어야 하며 앞 배치의 gate를 통과한 뒤 다음 배치로 이동합니다.

## 배치 0 — 기본선

- Git 저장소와 원격 연결
- Rust workspace, 품질 lint, CI
- 제품 범위, 아키텍처, 보안 경계
- 설정 계약과 SQLite schema 기본선

## 배치 1 — 읽기 전용 Telegram MVP

- long polling과 update cursor
- 일회용 pairing과 숫자 Telegram user ID 고정
- `메뉴 → 서버 상태 → 뒤로가기`
- CPU, 메모리, swap, load, 디스크 상태
- 영속 reply keyboard와 inline menu

## 배치 2 — 서비스 관제

- systemd unit discovery
- Nginx/Apache, PHP-FPM, MariaDB/MySQL, Redis 분류
- G7 queue, scheduler, Reverb 후보 분류
- 서비스별 active/sub 상태와 최근 실패 표시

## 배치 3 — 안전한 원격조치

- root-owned unit allowlist
- restart/reload 계획과 단회 승인 token
- 만료, 재전송, 다른 사용자 callback 거부
- 실행 전후 read-back과 감사로그

## 배치 4 — 알림과 웹 검증

- 상태 임계값, 연속 실패, dedupe, recovery
- silence와 bounded incident retention
- 설정된 URL의 HTTP status/latency
- TLS 만료 경고

## 배치 5 — 배포

- Ubuntu 22.04+ systemd unit
- `.deb` package, post-install/pre-remove
- GitHub Release checksum과 설치 스크립트
- token rotation, update, rollback, uninstall

## 배치 6 — 완료 검증

- `CODE_ONLY`: unit test와 lint
- `AUTO_PASS`: Telegram fixture, systemd fixture, package test
- `VPS_PASS`: Ubuntu 22.04/24.04 2GB VPS
- 일반 웹서버와 G7 관련 서비스 표면 각각 검증
- RSS, idle CPU, SQLite 크기 gate

