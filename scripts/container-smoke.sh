#!/bin/sh
set -eu

[ "$#" -eq 1 ] || {
    echo "usage: container-smoke.sh /path/to/package.deb" >&2
    exit 64
}

package=$1
export DEBIAN_FRONTEND=noninteractive
/usr/bin/apt-get update -qq
temporary=$(/usr/bin/mktemp -d)
trap '/usr/bin/rm -rf "$temporary"' EXIT HUP INT TERM

# 실제 운영처럼 이전 기본 설정과 사용자가 수정한 conffile을 준비합니다.
old_root="$temporary/old-root"
/usr/bin/dpkg-deb -R "$package" "$old_root"
/usr/bin/sed -i \
    -e '/^cpu_warning_percent = /d' \
    -e '/^load_warning_per_cpu = /d' \
    -e '/^swap_warning_percent = /d' \
    -e '/^server_reboot_enabled = /d' \
    "$old_root/etc/g7telegram-devops/agent.toml"
/usr/bin/sed -i 's/^Version: .*/Version: 0.3.99-1/' "$old_root/DEBIAN/control"
old_package="$temporary/g7telegram-devops-old.deb"
/usr/bin/dpkg-deb -b "$old_root" "$old_package" >/dev/null
/usr/bin/apt-get install -y -qq time "$old_package"
/usr/bin/printf '\n# local-config-must-survive-upgrade\n' \
    >> /etc/g7telegram-devops/agent.toml

# 신버전 기본 설정이 바뀌어도 비대화형 업그레이드는 운영 설정을 보존합니다.
/usr/bin/apt-get -o Dpkg::Options::=--force-confold \
    install -y -qq "$package"
/usr/bin/grep -F -x -q \
    '# local-config-must-survive-upgrade' \
    /etc/g7telegram-devops/agent.toml

[ "$(/usr/bin/stat -c %U /var/lib/g7telegram-devops)" = g7tg-agent ]
[ "$(/usr/bin/stat -c %a /etc/sudoers.d/g7telegram-devops)" = 440 ]
/usr/sbin/visudo -c -f /etc/sudoers.d/g7telegram-devops >/dev/null

metrics="$temporary/metrics"
output="$temporary/output"
/usr/sbin/runuser -u g7tg-agent -- \
    /usr/bin/time -v /usr/bin/g7tg \
    --config /etc/g7telegram-devops/agent.toml doctor \
    >"$output" 2>"$metrics"
/usr/bin/grep -F -q 'PASS: configuration for my-vps (not-paired)' "$output"
/usr/bin/grep -F -q \
    'Thresholds: CPU 90.0%, Load 1.50/CPU, Memory 90.0%, Swap 80.0% with memory pressure, Disk 85.0%' \
    "$output"
/usr/bin/grep -F -x -q 'Server reboot: disabled' "$output"

power_output="$temporary/power-output"
/usr/bin/g7tg --config /etc/g7telegram-devops/agent.toml power status >"$power_output"
/usr/bin/grep -F -x -q 'Telegram 서버 재시작: 사용 안 함' "$power_output"
/usr/bin/grep -F -x -q 'Root helper: disabled' "$power_output"

# systemd가 없는 package container에서는 고정 성공 stub으로 CLI 설정 왕복만 검증합니다.
fake_bin="$temporary/fake-bin"
/usr/bin/mkdir -p "$fake_bin"
/usr/bin/printf '#!/bin/sh\nexit 0\n' >"$fake_bin/systemctl"
/usr/bin/chmod 0755 "$fake_bin/systemctl"
PATH="$fake_bin:/usr/sbin:/usr/bin:/sbin:/bin" \
    /usr/bin/g7tg --config /etc/g7telegram-devops/agent.toml power enable \
    >"$power_output"
/usr/bin/grep -F -x -q 'Telegram 서버 재시작: 사용' "$power_output"
/usr/bin/grep -F -x -q 'Agent 재시작 및 상태 확인: PASS' "$power_output"
/usr/bin/grep -F -x -q '# local-config-must-survive-upgrade' \
    /etc/g7telegram-devops/agent.toml
/usr/bin/grep -F -x -q 'server_reboot_enabled = true' \
    /etc/g7telegram-devops/agent.toml
/usr/sbin/runuser -u g7tg-agent -- \
    /usr/lib/g7telegram-devops/g7tg-exec check-reboot
PATH="$fake_bin:/usr/sbin:/usr/bin:/sbin:/bin" \
    /usr/bin/g7tg --config /etc/g7telegram-devops/agent.toml power disable \
    >"$power_output"
/usr/bin/grep -F -x -q 'Telegram 서버 재시작: 사용 안 함' "$power_output"
/usr/bin/grep -F -x -q 'server_reboot_enabled = false' \
    /etc/g7telegram-devops/agent.toml
[ ! -e /etc/g7telegram-devops/allow-server-reboot ]

helper=/usr/lib/g7telegram-devops/g7tg-exec
if /usr/sbin/runuser -u g7tg-agent -- "$helper" check-reboot; then
    echo "server reboot must be disabled by default" >&2
    exit 1
fi
/usr/bin/printf 'enabled\n' > /etc/g7telegram-devops/allow-server-reboot
/usr/bin/chown root:g7tg-agent /etc/g7telegram-devops/allow-server-reboot
/usr/bin/chmod 0640 /etc/g7telegram-devops/allow-server-reboot
/usr/sbin/runuser -u g7tg-agent -- "$helper" check-reboot
/usr/bin/printf 'enabled\nextra\n' > /etc/g7telegram-devops/allow-server-reboot
if /usr/sbin/runuser -u g7tg-agent -- "$helper" check-reboot; then
    echo "server reboot permission accepted an invalid body" >&2
    exit 1
fi
/usr/bin/rm -f /etc/g7telegram-devops/allow-server-reboot

rss_kib=$(/usr/bin/awk '/Maximum resident set size/ {print $NF}' "$metrics")
[ -n "$rss_kib" ]
[ "$rss_kib" -le 65536 ] || {
    echo "doctor RSS gate failed: ${rss_kib}KiB" >&2
    exit 1
}

database_bytes=$(/usr/bin/stat -c %s /var/lib/g7telegram-devops/state.sqlite3)
[ "$database_bytes" -le 1048576 ] || {
    echo "initial SQLite size gate failed: ${database_bytes}B" >&2
    exit 1
}

# 패키지에 속하지 않는 이전 수동 설정 백업도 purge에서 남기지 않습니다.
/usr/bin/touch /etc/g7telegram-devops/agent.toml.pre-purge-test
/usr/bin/apt-get purge -y -qq g7telegram-devops
[ ! -e /etc/g7telegram-devops ]
[ ! -e /var/lib/g7telegram-devops ]
if /usr/bin/getent passwd g7tg-agent >/dev/null 2>&1; then
    echo "purge left the g7tg-agent user behind" >&2
    exit 1
fi

echo "PASS: Ubuntu package smoke under 2GB limit; RSS=${rss_kib}KiB; SQLite=${database_bytes}B"
