#!/bin/sh
set -eu

cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace

if command -v shellcheck >/dev/null 2>&1; then
    shellcheck \
        scripts/*.sh \
        packaging/deb/postinst \
        packaging/deb/prerm \
        packaging/deb/postrm \
        packaging/libexec/g7tg-exec
fi

