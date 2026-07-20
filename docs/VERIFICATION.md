# 검증 기준

검증 수준을 섞어 표현하지 않습니다.

| 수준 | 확인 범위 | 로컬 gate |
|---|---|---|
| `CODE_ONLY` | Rust format, Clippy, unit/fixture test, dependency audit, ShellCheck | `scripts/verify-local.sh` |
| `AUTO_PASS` | Ubuntu 22.04 amd64 빌드, `.deb` 구조·권한, Ubuntu 22.04/24.04 설치·실행 | `scripts/build-package-local.sh`의 2GB 제한 Docker smoke |
| `VPS_PASS` | 실제 Bot pairing, 장시간 idle 자원, 실제 systemd restart와 알림 왕복 | 운영 VPS에서 별도 수행 |

## 자동 자원 gate

- 컨테이너 메모리 상한: 2GB
- `g7tg doctor` 최대 RSS: 64MiB 이하
- 초기 SQLite: 1MiB 이하
- Ubuntu 22.04에서 만든 amd64 패키지를 22.04와 24.04에 각각 설치
- 이전 SQLite schema 자동 migration, owner 안전 교체, 16자리 pairing과 실패 제한
- 서비스 pagination, 정기 상태 요약 허용값·주기 계산, scoped collector 대조, bounded 감사로그
- 최상위 installer 기본 버전과 Cargo 버전 일치, 기존 installer URL 호환 wrapper ShellCheck
- Apache-2.0 메타데이터와 LICENSE·NOTICE의 `.deb` 포함 여부
- 설치 전 책임 제한 `Y/N` 확인, 비대화형 기본 거부와 관리자 자동화의 명시적 동의값
- 패키지 재시작 실패 전파, downgrade 허용, 설치 후 health gate 정적 검사

이 gate는 저사양 설치 적합성을 확인하지만, 실제 Bot token과 운영 서비스가 필요한 `VPS_PASS`를 대신하지 않습니다.

GitHub Actions는 사용하지 않습니다. `scripts/release-local.sh`가 로컬 gate를 모두 다시 통과한 경우에만 태그와 GitHub Release를 생성합니다.

## VPS 검증 절차

1. Ubuntu 버전과 가용 메모리를 기록합니다.
2. Release 설치 스크립트로 `.deb` checksum 설치를 수행합니다.
3. `setup` 연결코드로 개인 Telegram owner를 등록합니다.
4. 메뉴, 서버 상태, 일반 웹서비스와 G7 관련 서비스 분류를 확인합니다.
5. 서비스가 8개를 넘으면 이전/다음 페이지와 마지막 점검시각을 확인합니다.
6. 정기 상태 요약 설정을 켰다가 끄고 저장값과 메뉴 표시를 확인합니다.
7. 테스트용 allowlist 서비스만 선택해 취소, 만료, 승인 재시작을 각각 확인합니다.
8. HTTP/TLS 장애와 복구 알림이 한 번씩 오는지 확인합니다.
9. 24시간 idle CPU, 최대 RSS, SQLite 크기와 서비스 재시작 횟수를 기록합니다.
