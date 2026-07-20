# 공개 베타 보안 검토 보고서

검토일: 2026-07-20
대상: `main` v0.6.1-beta.4 source, `.deb` 패키지와 기존 `g7devops` Ubuntu 24.04 운영 검증

## 요약

치명적 취약점은 발견하지 못했습니다. Telegram owner 인증, 비밀값 파일 권한, systemd 격리, exact service allowlist, 단회 재승인 구조는 공개 베타에 적절합니다.

다만 root로 설치되는 공개 제품의 배포 산출물이 별도 서명되지 않았으므로 공급망 보안은 정식 출시 기준에 미달합니다. 공개 URL 제한과 보안 제보 경로도 정식판 전에 보완하는 것이 좋습니다.

Rust 전용 보안 검토 지침은 사용한 보안 점검 도구에 포함되어 있지 않아, 일반 보안 원칙과 Rust 코드·systemd·sudo·Telegram 공식 계약을 기준으로 직접 검토했습니다.

## 확인된 보호 장치

- Telegram private chat과 숫자 user/chat ID가 모두 일치하는 단일 owner만 처리합니다. (`crates/g7tg-agent/src/runtime.rs:149`)
- Bot token은 root 전용 `0600` 파일에 원자 저장하고 systemd `LoadCredential=`로 전달합니다. (`crates/g7tg-agent/src/setup.rs:257`, `packaging/systemd/g7tg-agent.service:13`)
- Telegram 전송 오류는 token이 포함된 URL이나 reqwest 원문을 로그에 남기지 않습니다. (`crates/g7tg-agent/src/telegram.rs:130`)
- callback 승인은 owner, action, unit, UUID nonce, expiry에 묶이고 한 번만 소비됩니다. (`crates/g7tg-agent/src/storage.rs:363`)
- pairing code는 16자리 hexadecimal(64-bit)이며 5분 단회 소비, constant-time 비교, user/chat별 1분 5회 실패 후 1분 차단을 적용합니다.
- 재시작은 root 소유 allowlist와 안전한 `.service` 이름을 executor가 다시 검사합니다. (`packaging/libexec/g7tg-exec:11`)
- 임의 shell·SQL·파일·SSH·방화벽 작업은 Telegram 기능에 존재하지 않습니다.
- Agent는 비로그인 전용 사용자로 실행되며 filesystem, device, kernel, namespace 접근을 제한합니다. (`packaging/systemd/g7tg-agent.service:8`)
- 운영 VPS의 `systemd-analyze security` 노출 점수는 `4.0 OK`, token은 `root:root 0600`, sudoers는 `root:root 0440`으로 확인했습니다.
- 현재 저장소와 Git 전체 이력에서 Telegram Bot token 형태의 비밀값은 검출되지 않았습니다.
- `cargo audit`은 258개 dependency에서 알려진 취약점을 발견하지 않았습니다.
- Telegram 설정은 정기 상태 요약의 `꺼짐·6시간·12시간·24시간`만 허용하며 임의 URL·경로·임계값 입력을 받지 않습니다.

## High

### SEC-001 — 공개 산출물에 독립 서명이 없음

영향: GitHub 계정, 저장소 또는 Release가 탈취되면 공격자가 악성 `.deb`와 일치하는 체크섬을 함께 배포해 사용자 서버에서 root 코드를 실행시킬 수 있습니다.

최상위 `install.sh`는 내부의 현재 Beta 버전 또는 관리자가 지정한 `G7TG_VERSION`의 `.deb`와 `SHA256SUMS`를 같은 GitHub Release에서 받아 비교합니다. 이 검사는 전송 오류와 우발적 손상은 차단하지만 배포 주체 자체가 침해된 경우를 방어하지 못합니다. 짧은 설치 명령의 bootstrap도 mutable `main` URL을 사용합니다.

권고:

1. 로컬 오프라인 키로 release tag와 `SHA256SUMS`를 서명합니다.
2. 설치 스크립트에 검증용 공개키를 고정하고 서명 검증 실패 시 설치를 중단합니다.
3. 공개 베타 문서에서는 버전 고정 설치와 스크립트 사전 검토를 기본 절차로 안내합니다.
4. GitHub 계정 2FA와 서명된 commit/tag를 사용합니다.

## Medium

### SEC-002 — 해결: pairing code 강도와 실패 제한

v0.6.1-beta.1에서 code를 8자리에서 16자리로 늘리고 user/chat별 실패 제한을 추가했습니다. 실패 입력과 실제 code는 로그·감사 detail에 남기지 않습니다. 이전 SQLite DB는 새 제한 테이블을 자동 생성합니다.

### SEC-003 — privileged restart 성공 경로의 최신 실서버 증거 부족

sudo 명령은 wildcard argument를 사용하지만 executor가 unit 문법과 exact allowlist를 다시 확인하므로 임의 명령으로 확장되지는 않습니다. (`packaging/sudoers/g7telegram-devops:2`, `packaging/libexec/g7tg-exec:10`)

다만 운영 감사 DB에는 과거 capability 수정 전 재시작 실패 6건만 남아 있고 최신 버전의 성공 기록은 없습니다. 현재 `sudo -l` dry check는 통과하지만 정식판 전 비핵심 서비스에서 실제 성공·취소·만료를 다시 증명해야 합니다.

## Low / 향후 설정 기능 제약

### SEC-004 — 공개 URL 검사가 redirect와 private address를 제한하지 않음

웹 검사는 HTTP(S), credential·query·fragment 금지와 짧은 timeout을 적용하지만 최대 3회 redirect를 허용하고 private/loopback/link-local 목적지를 차단하지 않습니다. (`crates/g7tg-agent/src/web.rs:15`, `crates/g7tg-agent/src/config.rs:191`)

현재 URL은 VPS root만 설정하므로 외부 사용자가 SSRF를 발생시키는 입력 경로가 없어 위험은 낮습니다. 향후 Telegram `설정` 메뉴에서 URL 입력을 허용하면 다음 보호 없이 출시하면 안 됩니다.

- loopback, RFC1918, link-local, multicast, cloud metadata 주소 차단
- DNS resolve 결과와 redirect의 매 단계 재검증
- 가능하면 외부 공개 host allowlist 사용

### SEC-005 — 로컬 감사로그는 tamper-evident가 아님

감사로그는 bounded SQLite에 저장되며 Agent 사용자에게 DB 쓰기 권한이 있습니다. Agent 계정이나 프로세스가 완전히 침해되면 기록을 수정할 수 있습니다. 중앙 서버 없는 1:1 제품 경계에서는 수용 가능한 한계지만 법적·보안 감사용 원장으로 표현하면 안 됩니다.

## 수용된 설계상 위험

- `NoNewPrivileges=yes`는 setuid sudo executor를 막으므로 적용하지 않았습니다. 대신 capability를 `SETUID`, `SETGID`, `AUDIT_WRITE`로 제한하고 root helper가 exact allowlist를 재검증합니다.
- Telegram Bot API 특성상 token은 HTTPS URL 경로에 포함됩니다. Agent는 해당 URL과 transport 원문 오류를 로그에 남기지 않습니다.
- Agent와 VPS가 함께 중단되면 자체적으로 Telegram 알림을 보낼 수 없습니다. 외부 dead-man monitor가 없는 현재 제품 경계의 알려진 한계입니다.

## 공개 베타 판정

- runtime 인증·권한 경계: **Beta 적합**
- token·설정 파일 권한: **Beta 적합**
- dependency·비밀값 검사: **PASS**
- privileged action: **구조 적합, 최신 실서버 성공 증거 필요**
- release 공급망: **Beta 주의, 정식판 전 서명 필요**
- 종합: **제한 공개 베타 가능, 정식 GA 보류**

## 참고한 공식 자료

- [Telegram Bot API](https://core.telegram.org/bots/api)
- [sudoers manual](https://www.sudo.ws/docs/man/1.9.14/sudoers.man.pdf)
- [GitHub commit/tag signature verification](https://docs.github.com/en/authentication/managing-commit-signature-verification/about-commit-signature-verification)
- [OWASP SSRF Prevention Cheat Sheet](https://cheatsheetseries.owasp.org/cheatsheets/Server_Side_Request_Forgery_Prevention_Cheat_Sheet.html)
