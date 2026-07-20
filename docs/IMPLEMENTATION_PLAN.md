# 단계별 구현계획

각 배치는 독립적으로 테스트·배포할 수 있어야 하며 앞 배치의 gate를 통과한 뒤 다음 배치로 이동합니다.

## 배치 0 — 기본선

- Git 저장소와 원격 연결
- Rust workspace, 품질 lint, 로컬 검증 파이프라인
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
- restart 계획과 단회 승인 token
- 만료, 재전송, 다른 사용자 callback 거부
- 실행 전후 read-back과 감사로그

## 배치 4 — 알림과 웹 검증

- 상태 임계값, 연속 실패, dedupe, recovery
- silence와 bounded incident retention
- collector별 실패 격리와 silence 종료 요약
- 설정된 URL의 HTTP status/latency
- TLS 만료 경고

## 배치 5 — 배포

- Ubuntu 22.04+ systemd unit
- `.deb` package, post-install/pre-remove
- GitHub Release checksum과 설치 스크립트
- GitHub Actions 없는 로컬 빌드·릴리스·VPS 배포
- token rotation, update, rollback, uninstall
- owner 안전 교체·해제, 설치 후 restart/doctor health gate

## 배치 6 — 완료 검증

- `CODE_ONLY`: unit test와 lint
- `AUTO_PASS`: Telegram fixture, systemd fixture, package test
- `VPS_PASS`: Ubuntu 22.04/24.04 2GB VPS
- 일반 웹서버와 G7 관련 서비스 표면 각각 검증
- RSS, idle CPU, SQLite 크기 gate

## 배치 7 — 공개 베타 UX·보안 보강

- 연결코드 16자리 강화와 user/chat별 실패 횟수 제한
- 서비스 8개 단위 이전/다음 페이지 이동
- 서버 상태 마지막 UTC 점검시각
- Telegram 설정의 6·12·24시간 정기 상태 요약
- 로컬에서 켠 경우에만 표시되는 60초 확인문구형 전체 서버 재시작
- BotFather token 발급·폐기·교체 문서화
- prerelease 로컬 릴리스와 `g7devops` 배포 검증

## 배치 8 — 간편 설치 진입점

- 최상위 `install.sh`를 공개 설치 진입점으로 제공
- 설치기 내부에서 현재 Beta 버전을 선택해 사용자 버전 입력 제거
- 기존 `scripts/install.sh` URL은 호환 wrapper로 유지
- README를 5분 설치·연결·정상 확인 중심으로 축소
- installer 기본 버전과 Cargo 버전 불일치 시 로컬 gate 실패
