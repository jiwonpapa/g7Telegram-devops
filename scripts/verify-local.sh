#!/bin/sh
set -eu

repository=$(unset CDPATH; cd -- "$(dirname -- "$0")/.." && pwd)
cd "$repository"

for command in cargo cargo-audit shellcheck; do
    command -v "$command" >/dev/null 2>&1 || {
        echo "required command not found: $command" >&2
        exit 69
    }
done

scripts/check.sh
cargo audit
shellcheck \
    scripts/*.sh \
    packaging/deb/postinst \
    packaging/deb/prerm \
    packaging/deb/postrm \
    packaging/libexec/g7tg-exec

echo "PASS: local source verification"
