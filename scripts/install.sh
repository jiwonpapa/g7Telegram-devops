#!/bin/sh
set -eu

REPOSITORY=jiwonpapa/g7Telegram-devops

if [ "$(/usr/bin/id -u)" -ne 0 ]; then
    echo "Run as root: curl ... | sudo sh" >&2
    exit 77
fi

if [ ! -r /etc/os-release ]; then
    echo "Ubuntu 22.04 or newer is required" >&2
    exit 1
fi
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

if [ -n "${VERSION:-}" ]; then
    tag=v${VERSION#v}
else
    latest_url=$(/usr/bin/curl -fsSLI -o /dev/null -w '%{url_effective}' \
        "https://github.com/$REPOSITORY/releases/latest")
    tag=${latest_url##*/}
fi
version=${tag#v}
asset="g7telegram-devops_${version}_${architecture}.deb"
base="https://github.com/$REPOSITORY/releases/download/$tag"
temporary=$(/usr/bin/mktemp -d)
trap '/usr/bin/rm -rf "$temporary"' EXIT HUP INT TERM

/usr/bin/curl -fL "$base/$asset" -o "$temporary/$asset"
/usr/bin/curl -fL "$base/SHA256SUMS" -o "$temporary/SHA256SUMS"
(
    cd "$temporary"
    /usr/bin/grep -F "  $asset" SHA256SUMS > SHA256SUMS.selected
    /usr/bin/sha256sum -c SHA256SUMS.selected
)
/usr/bin/apt-get install -y "$temporary/$asset"

echo "Installed $asset"
echo "Next: sudo g7tg setup --server-name <name>"

