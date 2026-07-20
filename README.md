# G7Telegram DevOps

> 공개 베타 `v0.6.1-beta.1`: 실제 서버에 설치할 수 있지만 아직 `1.0` 정식판은 아닙니다. 중요한 서버에서는 설치 후 재시작·장애 알림 검증을 먼저 수행하십시오.

Ubuntu VPS 한 대와 Telegram Bot 한 개를 직접 연결하는 메뉴형 서버 관리 Agent입니다. 중앙 관제 서버나 중앙 데이터베이스 없이 VPS 내부의 Rust Agent가 Telegram Bot API로 outbound HTTPS long polling을 수행합니다.

## 제품 구조와 지원 환경

- 연결 구조: `VPS 1대 = Telegram Bot 1개 = Agent 1개`
- 운영체제: Ubuntu 22.04 이상
- CPU 아키텍처: 현재 `amd64`만 지원
- 권장 메모리: 2GB 이상, Agent 자체 사용량은 운영 VPS에서 약 6~8MB
- 서비스 관리자: systemd
- 웹 스택: Nginx, Apache, Caddy, PHP-FPM, MariaDB/MySQL/PostgreSQL, Redis/Memcached
- GnuBoard 7: queue, Reverb, Horizon 등 발견된 장기 실행 서비스를 일반 systemd 서비스로 표시
- 네트워크: VPS에서 GitHub와 `api.telegram.org`의 HTTPS 443 outbound 연결 필요

GnuBoard 5/7 코어를 수정하거나 PHP 플러그인·별도 공개 health endpoint를 설치하지 않습니다.

## 지원 기능

| 구분 | 지원 내용 |
|---|---|
| 서버 상태 | CPU, 1분 Load, 논리 CPU 수, RAM, Swap, 디스크, uptime, hostname, OS, kernel |
| 자원 경고 | CPU·Load·메모리·Swap 압박·디스크 임계값을 연속 확인한 뒤 장애/복구 알림 |
| 서비스 상태 | 웹서버, PHP-FPM, DB, 캐시, G7/Laravel 관련 systemd 서비스 자동 탐지·분류, 8개 단위 페이지 이동 |
| 서비스 상세 | unit 설명, LoadState, ActiveState, SubState, 정상/중지/장애 판정 |
| 안전한 조치 | root 소유 exact allowlist에 포함된 `.service`의 재시작만 허용, 실행 전 45초 단회 재승인 |
| 웹 상태 | 설정한 공개 URL의 HTTP 상태, 응답시간, TLS 연결과 인증서 만료일 확인 |
| 장애 관리 | 연속 확인, 중복 제거, 복구 알림, 1시간/6시간 알림중지와 종료 요약 |
| 정기 상태 요약 | 기본 꺼짐, Telegram 설정에서 6시간/12시간/24시간 선택, 자원·서비스·웹·현재 장애 요약 |
| Telegram 인증 | 개인채팅, 숫자 user/chat ID, 5분·16자리 단회 연결코드, 사용자별 실패 제한, 단일 owner |
| 상태 저장 | 로컬 SQLite WAL, update cursor, incident, bounded 감사로그와 notification outbox |
| 배포 관리 | `.deb` checksum 설치, 설정 보존 업데이트, 버전 지정 롤백, remove/purge |

다음 기능은 지원하지 않습니다.

- 중앙 관제, 다중 서버 통합 화면, 멀티테넌시
- 임의 shell·SQL·파일 편집·업로드·다운로드
- DB 복원·삭제, 방화벽·SSH·사용자 계정·OS 전체 업데이트
- Telegram을 통한 Agent 자체 업데이트
- Agent 또는 VPS 자체가 완전히 중단됐을 때의 외부 감지
- `arm64/aarch64`

## 설치 전 준비

1. Ubuntu 22.04 이상 `amd64` VPS와 `sudo` 가능한 SSH 계정을 준비합니다.
2. Telegram 공식 `@BotFather`에서 이 VPS 전용 Bot을 만들고 token을 복사합니다.
3. 선택 사항으로 검사할 대표 웹 주소를 준비합니다. 공개 HTTP(S) URL만 사용하며 query token이나 ID/PW가 포함된 URL은 허용하지 않습니다.
4. VPS에 `curl`과 CA 인증서가 없으면 먼저 설치합니다.

```bash
sudo apt-get update
sudo apt-get install -y curl ca-certificates
```

Bot token은 설치 터미널에만 입력합니다. Telegram 대화, GitHub Issue, 설정 TOML 또는 명령행 인자로 전달하지 마십시오.

### Telegram Bot token 받는 법

1. Telegram에서 공식 인증 계정 `@BotFather`를 검색해 개인채팅을 엽니다.
2. `/newbot`을 보내고 Bot 표시 이름을 입력합니다. 예: `회사 VPS 관리봇`
3. 영문 username을 입력합니다. 반드시 `bot`으로 끝나야 합니다. 예: `company_vps_devops_bot`
4. BotFather가 보내는 `숫자:문자열` 형태의 **HTTP API token**을 복사합니다.
5. VPS에서 `sudo g7tg setup`을 실행할 때 숨김 입력란에만 붙여넣습니다.
6. 설정이 끝나면 생성한 Bot의 개인채팅을 열고 VPS 터미널에 나온 연결코드를 보냅니다.

VPS 한 대마다 Bot 하나를 따로 만드는 것을 권장합니다. 이 Agent는 개인채팅만 받으므로 BotFather의 group privacy 설정을 해제할 필요가 없습니다. token이 노출되면 BotFather에서 `/revoke`로 기존 token을 즉시 폐기하고 새 token을 받은 뒤 `sudo g7tg setup`을 다시 실행하십시오.

## 권장 설치 방법

공개 베타에서는 설치 스크립트를 먼저 내려받아 확인하고 Beta 버전을 고정해 실행하는 방식을 권장합니다.

```bash
curl -fsSLo /tmp/g7tg-install.sh \
  https://raw.githubusercontent.com/jiwonpapa/g7Telegram-devops/main/scripts/install.sh
sed -n '1,220p' /tmp/g7tg-install.sh
sudo G7TG_VERSION=0.6.1-beta.1 sh /tmp/g7tg-install.sh
rm -f /tmp/g7tg-install.sh
```

간편 설치는 다음 한 줄로 가능합니다.

```bash
curl -fsSL https://raw.githubusercontent.com/jiwonpapa/g7Telegram-devops/main/scripts/install.sh \
  | sudo G7TG_VERSION=0.6.1-beta.1 sh
```

### 서버 콘솔에서 설치 따라하기

아래 예시는 새 VPS에 SSH로 접속해 설치하고 Telegram owner 연결까지 마치는 실제 CLI 흐름입니다. `$` 뒤의 명령만 입력하십시오. 패키지 설치 로그와 Bot 이름·ID·연결코드는 서버마다 달라질 수 있습니다.

```console
$ ssh ubuntu@서버주소

ubuntu@my-vps:~$ sudo apt-get update
ubuntu@my-vps:~$ sudo apt-get install -y curl ca-certificates

ubuntu@my-vps:~$ curl -fsSL https://raw.githubusercontent.com/jiwonpapa/g7Telegram-devops/main/scripts/install.sh \
>   | sudo G7TG_VERSION=0.6.1-beta.1 sh

g7telegram-devops_0.6.1-beta.1_amd64.deb: OK
[apt 패키지 설치 로그]
지금 Telegram 초기설정을 시작하시겠습니까? [Y/n] y
Server name [my-vps]: my-vps
Telegram Bot token: [입력해도 화면에 표시되지 않음]
Telegram Bot 확인: 회사 VPS 관리봇 (ID 1234567890)
Web status URL (optional, Enter=skip): https://example.com
설정 완료: my-vps
웹 상태 검사: https://example.com/
Telegram Bot에 다음 연결코드를 보내십시오: ABCD1234EF567890
연결코드 유효시간: 300초
Telegram owner 연결을 기다립니다...
```

여기서 Telegram을 열고 생성한 Bot의 **개인채팅에 화면의 연결코드만** 보냅니다. 연결되면 서버 콘솔이 다음처럼 완료됩니다.

```console
Telegram owner 연결 완료: user ID 123456789, chat ID 123456789
PASS: configuration for my-vps (paired)
Monitoring: 60s interval, 2 consecutive confirmations
Thresholds: CPU 90.0%, Load 1.50/CPU, Memory 90.0%, Swap 80.0% with memory pressure, Disk 85.0%
Agent health: PASS
Installed g7telegram-devops_0.6.1-beta.1_amd64.deb
```

대표 웹 주소를 검사하지 않으려면 `Web status URL` 질문에서 Enter를 누릅니다. 초기설정 질문에서 `n`을 선택했거나 연결 대기시간이 끝났다면 다음 명령으로 다시 진행합니다.

```console
ubuntu@my-vps:~$ sudo g7tg setup
```

설치가 끝난 뒤 다음 명령으로 버전·설정·서비스·로그를 확인합니다.

```console
ubuntu@my-vps:~$ g7tg --version
g7tg 0.6.1-beta.1

ubuntu@my-vps:~$ sudo g7tg doctor
PASS: configuration for my-vps (paired)
Monitoring: 60s interval, 2 consecutive confirmations
Thresholds: CPU 90.0%, Load 1.50/CPU, Memory 90.0%, Swap 80.0% with memory pressure, Disk 85.0%

ubuntu@my-vps:~$ systemctl is-active g7tg-agent.service
active

ubuntu@my-vps:~$ sudo journalctl -u g7tg-agent.service --since today --no-pager
```

정상 기준은 `doctor`의 `PASS`, systemd의 `active`, Telegram 개인채팅의 `메뉴` 버튼입니다. token 오류가 발생하면 BotFather에서 token을 다시 복사해 `sudo g7tg setup`을 실행하십시오. token을 명령행에 직접 넣지는 마십시오.

설치 스크립트는 다음 순서로 동작합니다.

1. root 권한, Ubuntu 22.04 이상, `amd64` 여부를 확인합니다.
2. 지정한 GitHub Release에서 `.deb`와 `SHA256SUMS`를 내려받습니다.
3. 설치할 `.deb`의 SHA-256이 정확히 일치할 때만 `apt`로 설치합니다.
4. 기존 설치라면 설정, Bot token, Telegram owner와 SQLite 상태를 유지합니다.
5. 처음 설치하고 터미널이 연결되어 있으면 초기설정 시작 여부를 묻습니다.
6. 설정된 서버는 설치 후 Agent 재시작, `doctor`, systemd active 검증까지 통과해야 성공합니다.

태그만 있고 Release 산출물이 없는 버전은 설치되지 않습니다. `G7TG_VERSION`을 생략하면 GitHub의 최신 정식 Release를 설치하며 prerelease는 자동 선택되지 않습니다.

## 최초 설정과 Telegram 연결

설치를 마친 뒤 초기설정을 시작하지 않았다면 다음 명령을 실행합니다.

```bash
sudo g7tg setup
```

초기설정 순서는 다음과 같습니다.

1. Telegram에 표시할 서버 이름을 입력합니다. 기본값은 hostname입니다.
2. BotFather가 발급한 Bot token을 입력합니다. 입력 문자는 화면에 표시되지 않습니다.
3. Agent가 Telegram `getMe`로 token과 Bot 계정을 검증합니다.
4. 대표 웹 URL을 선택적으로 입력합니다. `example.com`처럼 입력하면 HTTPS를 기본 적용합니다.
5. Agent가 관리 대상 systemd 서비스를 탐지하고 root 소유 allowlist를 생성합니다.
6. Agent 서비스를 enable/restart하고 active 상태를 확인합니다.
7. 터미널에 5분 유효 일회용 연결코드가 표시됩니다.
8. Telegram에서 만든 Bot의 **개인채팅**을 열고 연결코드만 전송합니다.
9. 실제 메시지 발신자의 숫자 user/chat ID가 owner로 저장되면 `메뉴` 버튼이 표시됩니다.

Telegram username이나 사용자가 직접 입력한 ID는 신뢰하지 않습니다. 한 번 연결되면 등록 owner 이외의 사용자, 그룹, 슈퍼그룹, 채널 요청은 거부합니다.

### 자동 설치에서 설정 건너뛰기

```bash
curl -fsSL https://raw.githubusercontent.com/jiwonpapa/g7Telegram-devops/main/scripts/install.sh \
  | sudo G7TG_VERSION=0.6.1-beta.1 G7TG_SKIP_SETUP=1 sh
sudo g7tg setup
```

### 특정 버전 설치 또는 롤백

현재 공개 Beta를 설치하거나 같은 버전으로 롤백하는 명령입니다.

```bash
curl -fsSL https://raw.githubusercontent.com/jiwonpapa/g7Telegram-devops/main/scripts/install.sh \
  | sudo G7TG_VERSION=0.6.1-beta.1 sh
```

## 설치 확인

```bash
g7tg --version
sudo g7tg doctor
sudo systemctl status g7tg-agent.service --no-pager
sudo journalctl -u g7tg-agent.service --since today --no-pager
```

정상 기준은 다음과 같습니다.

- `doctor`: `PASS`와 실제 감시 주기·임계값 출력
- systemd: `active (running)`
- Telegram: `메뉴` 입력 또는 하단 메뉴 버튼으로 주 메뉴 표시
- 웹 상태: URL을 설정했다면 HTTP 상태·응답시간·TLS 잔여일 표시
- 서비스: 자동 발견된 unit과 실제 systemd 상태가 일치

운영에 사용하기 전에는 비핵심 allowlist 서비스 하나로 `재시작 → 승인 → 성공`을 확인하고, 테스트용 웹 URL로 장애·복구 알림이 각각 한 번 도착하는지 확인하십시오.

## Telegram 메뉴 사용법과 UX 평가

```text
메뉴
├─ 서버 상태 ─ 새로고침 / 뒤로가기
├─ 서비스 ─ 이전/다음 ─ 서비스 상세 ─ 재시작 ─ 승인하고 실행 / 취소
├─ 웹 상태 ─ 새로고침 / 뒤로가기
├─ 장애/알림 ─ 1시간·6시간 중지 / 해제 / 뒤로가기
├─ 설정 ─ 정기 상태 요약 꺼짐·6시간·12시간·24시간
└─ Agent 정보 ─ 버전 / 뒤로가기
```

- 슬래시 명령을 외울 필요 없이 하단 `메뉴` 버튼과 inline 버튼만 사용합니다.
- 조회 화면은 기존 메시지를 갱신하므로 채팅이 불필요하게 길어지지 않습니다.
- 조회 화면에는 `새로고침`과 `뒤로가기`를 배치했습니다.
- 서비스는 8개씩 표시하고 `이전/다음`으로 모두 확인할 수 있습니다.
- 서버 상태와 정기 요약에 마지막 UTC 점검시각을 표시합니다.
- 서비스 재시작은 대상·현재 상태·영향·승인 만료시간을 보여준 뒤 다시 승인받습니다.
- Telegram Bot API는 채팅 폭과 글꼴 크기를 제어할 수 없습니다. 서버 상태는 기본 글꼴과 짧은 한 줄 표현을 사용하므로 기기에 따라 열 정렬이 조금 다를 수 있습니다.

현재 UX는 슬래시 명령 없이 조회·이동·재승인·제한된 알림 설정을 수행하므로 공개 베타 범위에 적합합니다.

## 설정 옵션

정적 설정은 root가 `/etc/g7telegram-devops/agent.toml`에서 관리합니다. Telegram에서는 임의 설정값이나 경로를 입력받지 않습니다.

| 설정 | 기본값 | 허용 범위·설명 |
|---|---:|---|
| `monitor_interval_seconds` | `60` | 30~300초, 상태·서비스·웹 검사 주기 |
| `incident_confirmation_count` | `2` | 1~5회, 같은 문제를 장애로 확정할 연속 횟수 |
| `cpu_warning_percent` | `90` | 50~99% |
| `load_warning_per_cpu` | `1.5` | 논리 CPU당 0.5~10.0 |
| `memory_warning_percent` | `90` | 50~99% |
| `swap_warning_percent` | `80` | 메모리 경고와 함께 발생할 때만 적용, 50~99% |
| `disk_warning_percent` | `85` | 50~99%, 95% 이상은 critical |
| `approval_ttl_seconds` | `45` | 20~120초, 재시작 재승인 유효시간 |
| `web_checks` | 없음 | 최대 8개, HTTP(S) status·latency·TLS 검사 |
| `extra_service_units` | 없음 | 자동 탐지 외에 표시할 `.service`, 최대 32개 |
| `service_actions_enabled` | 설치 시 결정 | 탐지 unit이 있고 exact allowlist가 생성된 경우 활성화 |

설정을 수정한 뒤에는 검증을 통과한 경우에만 Agent를 재시작합니다.

```bash
sudoedit /etc/g7telegram-devops/agent.toml
sudo g7tg doctor
sudo systemctl restart g7tg-agent.service
sudo systemctl is-active g7tg-agent.service
```

### 웹 상태 검사 추가 예시

```toml
[[web_checks]]
name = "대표 사이트"
url = "https://example.com/"
expected_status_min = 200
expected_status_max = 399
timeout_seconds = 5
tls_warning_days = 14
```

URL에는 credential, query, fragment를 넣을 수 없습니다. 설정은 root만 수정할 수 있지만 공개 Beta에서는 외부 공개 URL만 등록하십시오.

### 정기 상태 알림 옵션

Agent는 기본 60초마다 헬스체크하고 문제 발생·복구는 즉시 알립니다. 정상 상태를 정기적으로 받고 싶으면 Telegram `메뉴 → 설정`에서 선택합니다.

매시간 정상 알림은 알림 피로가 크므로 제공하지 않습니다.

- 정기 상태 요약: `꺼짐`(기본), `6시간`, `12시간`, `24시간`
- 임계값은 조회만 제공하고 변경은 SSH의 root 설정에서 수행
- 정기 요약에는 자원·서비스·웹 상태와 마지막 점검시각만 포함
- 알림중지 중에는 정기 요약도 보내지 않음

Agent와 VPS가 함께 중단되면 Telegram을 보낼 수 없으므로 정기 메시지만으로 완전한 서버 다운 감지를 보장할 수 없습니다. 완전한 다운 감지는 외부 dead-man monitor가 필요합니다.

## 업데이트, owner 교체와 제거

같은 버전 고정 설치 명령을 다시 실행하면 기존 설정과 상태를 유지하며 패키지를 검증합니다. 새 Beta가 나오면 `G7TG_VERSION`만 변경합니다.

```bash
curl -fsSL https://raw.githubusercontent.com/jiwonpapa/g7Telegram-devops/main/scripts/install.sh \
  | sudo G7TG_VERSION=0.6.1-beta.1 sh
```

owner 교체와 해제는 VPS의 root만 시작할 수 있습니다.

```bash
sudo g7tg pair --replace
sudo g7tg unpair --confirm
```

설정과 상태를 남기는 제거와 완전 삭제는 구분됩니다.

```bash
sudo apt remove g7telegram-devops
sudo apt purge g7telegram-devops
```

## 보안 설계와 공개 베타 주의사항

- Bot token: `/etc/g7telegram-devops/secrets/bot-token`, `root:root 0600`
- 설정: `/etc/g7telegram-devops/agent.toml`, `root:g7tg-agent 0640`
- 재시작 allowlist: root 소유 파일과 executor가 unit 문법을 이중 검증
- Agent: 비로그인 전용 사용자, systemd filesystem/kernel/device/namespace 제한
- 네트워크: inbound port를 열지 않고 Telegram Bot API로 outbound HTTPS만 사용
- 승인: owner·동작·unit·무작위 nonce·만료시간에 묶고 한 번만 소비
- 연결: 16자리 무작위 code와 user/chat별 1분 5회 실패 제한
- 금지 기능: 임의 shell, 파일, SQL, 사용자·SSH·방화벽 조작

현재 `.deb`와 체크섬은 같은 GitHub Release에서 배포되며 별도 서명은 아직 없습니다. 공개 베타에서는 버전을 고정하고 설치 스크립트를 검토한 뒤 사용하십시오. 상세 검토 결과는 [보안 검토 보고서](docs/SECURITY_REVIEW.md)를 확인하십시오.

현재 저장소에는 오픈소스 `LICENSE`가 아직 지정되지 않았습니다. 소스 공개와 오픈소스 라이선스 부여는 다르므로 재배포·수정·상업 이용 조건은 라이선스 결정 후 확정됩니다.

## 검증 수준과 추가 테스트

- `CODE_ONLY`: Rust format, Clippy, 40개 unit/fixture test, dependency audit, ShellCheck
- `AUTO_PASS`: Ubuntu 22.04 amd64 빌드, `.deb` 구조·권한, Ubuntu 22.04/24.04 2GB 설치·업그레이드 smoke
- `VPS_PASS`: 실제 Bot pairing, 서비스 재시작, 장애·복구 알림, 24시간 자원·안정성 확인

현재 설치·실행·조회 기능은 운영 VPS에서 동작 중입니다. 정식판 전에는 최신 버전의 서비스 재시작 성공 기록, 실제 장애·복구 알림 왕복, Ubuntu 22.04 실제 VPS, 24시간 이상 연속 운영 검증이 추가로 필요합니다.

## 개발자 로컬 검증과 릴리스

GitHub Actions는 사용하지 않습니다. 로컬에서 검사·패키지·Ubuntu smoke를 모두 통과한 산출물만 GitHub Release에 업로드합니다.

```bash
scripts/verify-local.sh
scripts/build-package-local.sh
scripts/release-local.sh
```

## 문서

- [제품 범위](docs/PRODUCT_SCOPE.md)
- [아키텍처](docs/ARCHITECTURE.md)
- [단계별 구현계획](docs/IMPLEMENTATION_PLAN.md)
- [보안 경계](docs/SECURITY.md)
- [보안 검토 보고서](docs/SECURITY_REVIEW.md)
- [설치와 운영](docs/OPERATIONS.md)
- [검증 기준](docs/VERIFICATION.md)
