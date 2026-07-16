#!/bin/sh
set -eu

REPOSITORY=jiwonpapa/g7Telegram-devops
requested_version=${G7TG_VERSION:-}
skip_setup=${G7TG_SKIP_SETUP:-0}
force_setup=${G7TG_RUN_SETUP:-0}
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

if [ -n "$requested_version" ]; then
    tag=v${requested_version#v}
else
    latest_url=$(/usr/bin/curl -fsSLI -o /dev/null -w '%{url_effective}' \
        "https://github.com/$REPOSITORY/releases/latest")
    tag=${latest_url##*/}
fi
version=${tag#v}
/usr/bin/printf '%s\n' "$version" \
    | /usr/bin/grep -E -q '^[0-9]+\.[0-9]+\.[0-9]+([-.][0-9A-Za-z.-]+)?$' || {
    echo "Invalid release version: $version" >&2
    exit 1
}
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
    install -y "$temporary/$asset"

echo "Installed $asset"

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
