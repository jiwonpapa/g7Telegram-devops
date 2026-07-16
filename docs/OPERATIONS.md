# 설치와 운영

## Ubuntu 지원 범위

- Ubuntu 22.04 이상
- 현재 Release 패키지는 amd64 우선
- 2GB VPS에서는 빌드하지 않고 `.deb`만 설치

## 설치

간편 설치 명령은 GitHub Release의 `.deb`와 `SHA256SUMS`를 내려받아 일치할 때만 `apt`로 설치합니다.

```bash
curl -fsSL https://raw.githubusercontent.com/jiwonpapa/g7Telegram-devops/main/scripts/install.sh | sudo sh
```

최초 설치는 초기설정 시작 여부를 묻고, 동의하면 `setup`을 같은 터미널에서 실행합니다. 업데이트 설치는 기존 token과 owner ID를 유지합니다. 초기설정을 건너뛰었다면 다음 명령으로 다시 시작합니다.

```bash
sudo g7tg setup
```

`setup`은 다음을 수행합니다.

1. hostname을 기본값으로 서버 이름 입력
2. 화면에 표시하지 않는 Bot token 입력
3. Telegram `getMe`로 token과 Bot 계정 검증
4. 선택형 대표 웹 URL 입력과 HTTP·응답시간·TLS 검사 등록
5. token을 root 전용 secret으로 저장
6. 관리 대상 systemd service 자동 탐지
7. exact unit allowlist와 45초 재승인형 restart 기능 설정
8. Agent systemd enable/start
9. 일회용 Telegram 연결코드 출력
10. Bot 개인채팅에 코드를 보낸 발신자의 숫자 user/chat ID 자동 저장

사용자명이나 수동 입력한 숫자 ID는 신뢰하지 않습니다. Telegram이 전달한 실제 private chat 발신자 ID만 단회 연결코드와 함께 저장합니다. 연결 대기를 생략하려면 `--no-wait-for-pairing`을 사용합니다.

자동 설치에서 초기설정을 건너뛰려면 다음처럼 실행합니다.

```bash
curl -fsSL https://raw.githubusercontent.com/jiwonpapa/g7Telegram-devops/main/scripts/install.sh \
  | sudo G7TG_SKIP_SETUP=1 sh
```

## 상태 확인

```bash
sudo systemctl status g7tg-agent.service
sudo journalctl -u g7tg-agent.service --since today --no-pager
sudo g7tg doctor
```

`doctor`는 실제 적용 중인 감시 주기와 임계값을 함께 출력합니다. 기본값은 60초마다 검사하고 같은 문제가 2회 연속 관측될 때 알림을 확정합니다.

- CPU 사용률 90% 이상
- 논리 CPU 한 개당 1분 Load Average 1.5 이상
- 메모리 사용률 90% 이상
- 메모리 경고와 Swap 사용률 80% 이상이 동시에 발생한 압박 상태
- 디스크 사용률 85% 이상, 95% 이상은 치명 등급

CPU 순간 스파이크와 오래된 Swap 페이지만으로는 알림을 보내지 않습니다. 임계값은 `/etc/g7telegram-devops/agent.toml`에서 조정합니다.

## 추가 연결코드

owner가 아직 등록되지 않은 경우 Agent 사용자 권한으로 발급합니다.

```bash
sudo -u g7tg-agent /usr/bin/g7tg \
  --config /etc/g7telegram-devops/agent.toml pair
```

## Bot token 교체

`setup`을 다시 실행하면 owner와 incident 상태를 유지하면서 token, 서비스 탐지 결과와 설정을 갱신합니다.

```bash
sudo g7tg setup
```

## 업데이트와 롤백

같은 설치 명령을 다시 실행하면 최신 Release checksum을 검증해 설치하고, 실행 중인 Agent만 재시작합니다.

```bash
curl -fsSL https://raw.githubusercontent.com/jiwonpapa/g7Telegram-devops/main/scripts/install.sh | sudo sh
```

특정 버전 설치와 롤백은 `VERSION`을 지정합니다. 설정과 SQLite 상태는 유지됩니다.

```bash
curl -fsSL https://raw.githubusercontent.com/jiwonpapa/g7Telegram-devops/main/scripts/install.sh \
  | sudo G7TG_VERSION=0.2.0 sh
```

## 제거

설정과 상태를 남기려면 remove, 모두 제거하려면 purge를 사용합니다.

```bash
sudo apt remove g7telegram-devops
sudo apt purge g7telegram-devops
```
