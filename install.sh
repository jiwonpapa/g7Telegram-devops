#!/bin/sh
set -eu

REPOSITORY=jiwonpapa/g7Telegram-devops
DEFAULT_RELEASE_VERSION=0.6.1-beta.6
requested_version=${G7TG_VERSION:-$DEFAULT_RELEASE_VERSION}
skip_setup=${G7TG_SKIP_SETUP:-0}
force_setup=${G7TG_RUN_SETUP:-0}
accept_disclaimer=${G7TG_ACCEPT_DISCLAIMER:-0}
configured=0
[ -s /etc/g7telegram-devops/secrets/bot-token ] && configured=1
tty_available=0
if ( : < /dev/tty && : > /dev/tty ) 2>/dev/null; then
    tty_available=1
fi

if [ "$(/usr/bin/id -u)" -ne 0 ]; then
    echo "Run as root: curl ... | sudo sh" >&2
    exit 77
fi

if [ ! -r /etc/os-release ]; then
    echo "Ubuntu 22.04 or newer is required" >&2
    exit 1
fi
# shellcheck disable=SC1091
. /etc/os-release
major=${VERSION_ID%%.*}
if [ "${ID:-}" != ubuntu ] || [ "$major" -lt 22 ]; then
    echo "Ubuntu 22.04 or newer is required" >&2
    exit 1
fi

case "$(/usr/bin/dpkg --print-architecture)" in
    amd64) architecture=amd64 ;;
    *)
        echo "This release currently supports Ubuntu amd64 only" >&2
        exit 1
        ;;
esac

print_disclaimer() {
    /usr/bin/printf '%s\n' \
        '[중요: 무보증 및 책임 제한]' \
        "G7Telegram DevOps 공개 Beta는 Apache-2.0에 따라 '있는 그대로' 제공됩니다." \
        '서비스 중단, 설정 오류, 데이터 손실 등 사용에 따른 위험은 사용자가 검토하고 부담합니다.' \
        '사용 전 백업과 비핵심 서버 검증을 권장합니다.' \
        '관련 법률이 허용하는 범위에서 저작권자와 기여자는 사용으로 인한 손해를 책임지지 않습니다.'
}

if [ "$accept_disclaimer" = 1 ]; then
    print_disclaimer
    echo 'G7TG_ACCEPT_DISCLAIMER=1로 책임 제한 고지를 확인했습니다.'
elif [ "$tty_available" = 1 ]; then
    print_disclaimer > /dev/tty
    /usr/bin/printf '위 내용을 확인했으며 설치를 계속하시겠습니까? [y/N] ' > /dev/tty
    disclaimer_answer=
    IFS= read -r disclaimer_answer < /dev/tty || disclaimer_answer=n
    case "$disclaimer_answer" in
        y|Y|yes|YES) ;;
        *)
            echo '설치를 취소했습니다. 시스템은 변경되지 않았습니다.' > /dev/tty
            exit 0
            ;;
    esac
else
    print_disclaimer >&2
    echo '대화형 Y/N 확인이 필요합니다.' >&2
    echo '자동화에서는 G7TG_ACCEPT_DISCLAIMER=1을 명시하십시오.' >&2
    exit 77
fi

version=${requested_version#v}
/usr/bin/printf '%s\n' "$version" \
    | /usr/bin/grep -E -q '^[0-9]+\.[0-9]+\.[0-9]+([-.][0-9A-Za-z.-]+)?$' || {
    echo "Invalid release version: $version" >&2
    exit 1
}
tag=v$version
asset="g7telegram-devops_${version}_${architecture}.deb"
base="https://github.com/$REPOSITORY/releases/download/$tag"
temporary=$(/usr/bin/mktemp -d)
trap '/usr/bin/rm -rf "$temporary"' EXIT HUP INT TERM

/usr/bin/curl -fL "$base/$asset" -o "$temporary/$asset"
/usr/bin/curl -fL "$base/SHA256SUMS" -o "$temporary/SHA256SUMS"
(
    cd "$temporary"
    /usr/bin/awk -v asset="$asset" '$2 == asset { print }' \
        SHA256SUMS > SHA256SUMS.selected
    [ "$(/usr/bin/wc -l < SHA256SUMS.selected)" -eq 1 ]
    /usr/bin/sha256sum -c SHA256SUMS.selected
)
DEBIAN_FRONTEND=noninteractive /usr/bin/apt-get \
    -o Dpkg::Options::=--force-confold \
    install -y --allow-downgrades "$temporary/$asset"

run_setup=0
if [ "$force_setup" = 1 ]; then
    if [ "$tty_available" = 1 ]; then
        run_setup=1
    else
        echo "G7TG_RUN_SETUP=1 requires an interactive terminal" >&2
        exit 1
    fi
elif [ "$configured" = 0 ] && [ "$skip_setup" != 1 ] \
    && [ "$tty_available" = 1 ]; then
    /usr/bin/printf '지금 Telegram 초기설정을 시작하시겠습니까? [Y/n] ' > /dev/tty
    answer=
    IFS= read -r answer < /dev/tty || answer=n
    case "$answer" in
        ''|y|Y|yes|YES) run_setup=1 ;;
    esac
fi

if [ "$run_setup" = 1 ]; then
    /usr/bin/g7tg setup < /dev/tty > /dev/tty
elif [ "$configured" = 1 ]; then
    echo "기존 Telegram 설정과 owner ID를 유지했습니다."
else
    echo "Next: sudo g7tg setup"
fi

if [ -s /etc/g7telegram-devops/secrets/bot-token ]; then
    /usr/sbin/runuser -u g7tg-agent -- \
        /usr/bin/g7tg --config /etc/g7telegram-devops/agent.toml doctor
    /usr/bin/systemctl is-active --quiet g7tg-agent.service
    echo "Agent health: PASS"
fi

echo "Installed $asset"
