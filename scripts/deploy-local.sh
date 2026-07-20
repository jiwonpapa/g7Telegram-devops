#!/bin/sh
set -eu

[ "$#" -ge 1 ] && [ "$#" -le 2 ] || {
    echo "usage: deploy-local.sh SSH_TARGET [VERSION]" >&2
    exit 64
}

repository=$(unset CDPATH; cd -- "$(dirname -- "$0")/.." && pwd)
cd "$repository"

for command in cargo gh jq ssh; do
    command -v "$command" >/dev/null 2>&1 || {
        echo "required command not found: $command" >&2
        exit 69
    }
done

target=$1
crate_version=$(cargo metadata --no-deps --format-version 1 \
    | jq -r '.packages[] | select(.name == "g7tg-agent") | .version')
version=${2:-$crate_version}
case "$version" in
    ''|*[!0-9A-Za-z.-]*)
        echo "invalid version: $version" >&2
        exit 64
        ;;
esac

gh release view "v$version" >/dev/null
# version은 위 allowlist 검사 후 원격 명령에 전달합니다.
# shellcheck disable=SC2029
ssh "$target" \
    "curl -fsSL https://github.com/jiwonpapa/g7Telegram-devops/raw/main/install.sh | sudo -n env G7TG_ACCEPT_DISCLAIMER=1 G7TG_VERSION=$version G7TG_SKIP_SETUP=1 sh"
ssh "$target" \
    "g7tg --version && systemctl is-active g7tg-agent.service && systemctl show g7tg-agent.service -p NRestarts"

echo "PASS: deployed v$version to $target"
