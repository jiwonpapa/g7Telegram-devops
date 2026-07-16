# 아키텍처

## 런타임

운영 중 상주하는 프로젝트 프로세스는 `g7tg-agent` 하나입니다. Agent는 Telegram `getUpdates`를 long polling하고, 로컬 상태를 수집하며, SQLite에 최소 상태를 기록합니다.

```text
Telegram 사용자
    ↕
Telegram Bot API
    ↕ outbound HTTPS long polling
g7tg-agent
    ├── 시스템·디스크 collector
    ├── systemd service discovery
    ├── 웹·TLS health probe
    ├── 메뉴 state machine
    ├── SQLite state/audit
    └── 사전 정의된 root oneshot action
```

## 권한 분리

Agent는 전용 비권한 사용자로 실행합니다. root 권한이 필요한 동작은 root 소유의 allowlist와 별도 oneshot 실행 경로를 통해서만 수행합니다. 사용자 입력을 shell 문자열로 조합하지 않습니다.

## Telegram UI

첫 화면은 `메뉴` 버튼입니다. 이후에는 inline keyboard로 `서버 상태`, `서비스`, `웹 상태`, `장애/알림`, `정보`를 이동하며 모든 하위 화면에 `뒤로가기`를 제공합니다.

restart/reload는 조회 화면에서 바로 실행하지 않습니다. Agent가 대상, 현재 상태, 영향과 만료시간을 보여준 뒤 단회 callback 승인을 받아 실행합니다.

## GnuBoard 처리

GnuBoard 전용 웹 endpoint나 코어 플러그인은 MVP에 포함하지 않습니다. 설치 경로와 systemd unit 이름을 통해 G7 환경을 보조 분류하고, 실제 판정은 웹 응답과 관련 서비스 상태를 기준으로 합니다.

