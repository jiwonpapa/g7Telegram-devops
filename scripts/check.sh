#!/bin/sh
set -eu

cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace

installer_version=$(sed -n 's/^DEFAULT_RELEASE_VERSION=//p' install.sh)
workspace_version=$(sed -n 's/^version = "\([^"]*\)"/\1/p' Cargo.toml | sed -n '1p')
[ "$installer_version" = "$workspace_version" ] || {
    echo "install.sh default $installer_version != Cargo version $workspace_version" >&2
    exit 1
}

grep -F -q '위 내용을 확인했으며 설치를 계속하시겠습니까? [y/N]' install.sh
grep -F -q 'G7TG_ACCEPT_DISCLAIMER=1' install.sh
grep -F -q 'G7TG_ACCEPT_DISCLAIMER=1' scripts/deploy-local.sh
grep -F -q 'license = "Apache-2.0"' Cargo.toml
[ -s LICENSE ]
[ -s NOTICE ]

if command -v shellcheck >/dev/null 2>&1; then
    shellcheck \
        install.sh \
        scripts/*.sh \
        packaging/deb/postinst \
        packaging/deb/prerm \
        packaging/deb/postrm \
        packaging/libexec/g7tg-exec
fi
