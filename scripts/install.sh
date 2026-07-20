#!/bin/sh
set -eu

# v0.6.1-beta.1까지 공개한 기존 URL의 호환 진입점입니다.
url=https://github.com/jiwonpapa/g7Telegram-devops/raw/main/install.sh
temporary=$(/usr/bin/mktemp)
trap '/usr/bin/rm -f "$temporary"' EXIT HUP INT TERM
/usr/bin/curl -fsSL "$url" -o "$temporary"
/bin/sh "$temporary"
