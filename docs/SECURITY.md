# 보안 경계

## 인증

- 개인 채팅만 허용합니다.
- username이 아닌 64-bit Telegram user ID를 사용합니다.
- 최초 연결은 SSH에서 발급한 5분 유효 일회용 pairing code로 수행합니다.
- 미등록 사용자와 group/supergroup/channel update는 처리하지 않습니다.

## 비밀값

- Bot token은 설정 TOML, 명령행 인자, 로그에 넣지 않습니다.
- 운영에서는 root 소유 credential 파일을 systemd `LoadCredential=`로 전달합니다.
- Agent는 Telegram API URL과 오류에 token이 기록되지 않도록 별도 redaction을 적용합니다.

## 명령

- 사용자 문자열을 shell에 전달하지 않습니다.
- unit ID는 root 소유 allowlist와 발견된 unit을 교차 검증합니다.
- callback 승인은 owner, action, unit, nonce, expiry에 묶습니다.
- 동일 callback은 한 번만 소비합니다.
- restart 뒤 systemd 상태를 다시 읽어 결과를 판정합니다.

## Telegram에서 금지하는 작업

- 임의 shell, SQL, DB 복원·삭제
- 방화벽, SSH, 사용자 계정 변경
- 파일 편집·다운로드, 비밀값 출력
- OS 전체 업데이트와 Agent 자체 업데이트
