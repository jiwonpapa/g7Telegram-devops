# 제품 범위

## 한 문장 정의

`VPS 1대 = Telegram Bot 1개 = Rust Agent 1개`로 연결하는 메뉴형 웹서버 관리 도구입니다.

## 포함

- 메뉴 버튼 기반 서버 상태 조회
- CPU, 메모리, swap, load, 디스크, inode 기본 상태
- systemd 서비스 자동 탐지와 상태 표시
- 웹서버, PHP-FPM, DB, Redis 및 G7 관련 서비스 분류
- 선택한 서비스의 헬스체크
- 허용된 서비스 restart/reload 전 영향 안내와 재승인
- HTTP 응답과 TLS 만료의 최소 외부 검증
- 장애 중복 제거, 복구 알림, silence
- SQLite 기반 update cursor, 설정 상태, incident 및 감사로그
- Ubuntu 22.04 이상 `.deb` 설치, 업데이트, 제거

## 제외

- 중앙 관제, 다중 서버 통합 화면, 멀티테넌시
- GnuBoard 코어 수정 또는 별도 공개 health endpoint
- 임의 shell과 임의 systemd unit 실행
- SQL 실행, DB 복원 및 데이터 삭제
- 방화벽, SSH, 사용자 계정, OS 전체 업데이트
- 파일 편집, 업로드, 다운로드 및 무제한 로그 스트리밍
- Telegram을 통한 Agent 자체 업데이트

## 알려진 한계

Agent와 VPS가 함께 중단되면 Telegram으로 장애를 전송할 수 없습니다. 완전한 서버 다운 감지는 향후 선택형 외부 dead-man monitor의 범위입니다.

