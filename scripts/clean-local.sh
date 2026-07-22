#!/bin/sh
set -eu

repository=$(unset CDPATH; cd -- "$(dirname -- "$0")/.." && pwd)
mode=${1:---workspace-only}

case "$mode" in
    --workspace-only|--purge-cache) ;;
    *)
        echo "usage: clean-local.sh [--workspace-only|--purge-cache]" >&2
        exit 64
        ;;
esac

# Git에 포함되지 않는 재생성 가능한 산출물만 제거합니다.
/bin/rm -rf -- "$repository/target" "$repository/dist" "$repository/artifacts" "$repository/coverage"

if [ "$mode" = --purge-cache ]; then
    cache_root=$("$repository/scripts/build-cache-dir.sh")
    if [ -f "$cache_root/.g7telegram-devops-build-cache" ]; then
        /bin/rm -rf -- "$cache_root"
    fi

    if command -v docker >/dev/null 2>&1; then
        for volume in \
            g7telegram-devops-amd64-target \
            g7telegram-devops-cargo-registry
        do
            if docker volume inspect "$volume" >/dev/null 2>&1; then
                docker volume rm "$volume" >/dev/null
            fi
        done
    fi
fi

echo "PASS: workspace build artifacts cleaned"
if [ "$mode" = --purge-cache ]; then
    echo "PASS: external build caches purged"
fi
