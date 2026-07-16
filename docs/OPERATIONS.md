# 설치와 운영

## Ubuntu 지원 범위

- Ubuntu 22.04 이상
- 현재 Release 패키지는 amd64 우선
- 2GB VPS에서는 빌드하지 않고 `.deb`만 설치

## 설치

간편 설치 명령은 GitHub Release의 `.deb`와 `SHA256SUMS`를 내려받아 일치할 때만 `apt`로 설치합니다.

```bash
curl -fsSL https://raw.githubusercontent.com/jiwonpapa/g7Telegram-devops/main/scripts/install.sh | sudo sh
sudo g7tg setup --server-name my-vps
```

`setup`은 다음을 수행합니다.

1. 화면에 표시하지 않는 Bot token 입력
2. root 전용 secret 저장
3. 관리 대상 systemd service 자동 탐지
4. exact unit allowlist 생성
5. 45초 재승인형 restart 기능 활성화
6. 일회용 Telegram 연결코드 출력
7. Agent systemd enable/start

## 상태 확인

```bash
sudo systemctl status g7tg-agent.service
sudo journalctl -u g7tg-agent.service --since today --no-pager
sudo g7tg doctor
```

## 추가 연결코드

owner가 아직 등록되지 않은 경우 Agent 사용자 권한으로 발급합니다.

```bash
sudo -u g7tg-agent /usr/bin/g7tg \
  --config /etc/g7telegram-devops/agent.toml pair
```

## Bot token 교체

`setup`을 다시 실행하면 owner와 incident 상태를 유지하면서 token, 서비스 탐지 결과와 설정을 갱신합니다.

```bash
sudo g7tg setup --server-name my-vps
```

## 업데이트와 롤백

같은 설치 명령을 다시 실행하면 최신 Release checksum을 검증해 설치하고, 실행 중인 Agent만 재시작합니다.

```bash
curl -fsSL https://raw.githubusercontent.com/jiwonpapa/g7Telegram-devops/main/scripts/install.sh | sudo sh
```

특정 버전 설치와 롤백은 `VERSION`을 지정합니다. 설정과 SQLite 상태는 유지됩니다.

```bash
curl -fsSL https://raw.githubusercontent.com/jiwonpapa/g7Telegram-devops/main/scripts/install.sh \
  | sudo VERSION=0.1.0 sh
```

## 제거

설정과 상태를 남기려면 remove, 모두 제거하려면 purge를 사용합니다.

```bash
sudo apt remove g7telegram-devops
sudo apt purge g7telegram-devops
```
