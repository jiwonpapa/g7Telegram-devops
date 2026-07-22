#!/bin/sh
set -eu

if [ -n "${G7TG_BUILD_CACHE_DIR:-}" ]; then
    cache_dir=$G7TG_BUILD_CACHE_DIR
elif [ "$(uname -s)" = Darwin ]; then
    cache_dir="${HOME:?}/Library/Caches/g7telegram-devops"
else
    cache_dir="${XDG_CACHE_HOME:-${HOME:?}/.cache}/g7telegram-devops"
fi

case "$cache_dir" in
    /*) ;;
    *)
        echo "build cache path must be absolute: $cache_dir" >&2
        exit 64
        ;;
esac

printf '%s\n' "$cache_dir"
