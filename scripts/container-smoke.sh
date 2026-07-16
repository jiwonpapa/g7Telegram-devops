#!/bin/sh
set -eu

[ "$#" -eq 1 ] || {
    echo "usage: container-smoke.sh /path/to/package.deb" >&2
    exit 64
}

package=$1
export DEBIAN_FRONTEND=noninteractive
/usr/bin/apt-get update -qq
/usr/bin/apt-get install -y -qq time "$package"

[ "$(/usr/bin/stat -c %U /var/lib/g7telegram-devops)" = g7tg-agent ]
[ "$(/usr/bin/stat -c %a /etc/sudoers.d/g7telegram-devops)" = 440 ]
/usr/sbin/visudo -c -f /etc/sudoers.d/g7telegram-devops >/dev/null

metrics=$(/usr/bin/mktemp)
output=$(/usr/bin/mktemp)
trap '/usr/bin/rm -f "$metrics" "$output"' EXIT HUP INT TERM
/usr/sbin/runuser -u g7tg-agent -- \
    /usr/bin/time -v /usr/bin/g7tg \
    --config /etc/g7telegram-devops/agent.toml doctor \
    >"$output" 2>"$metrics"
/usr/bin/grep -F -q 'PASS: configuration for my-vps (not-paired)' "$output"
/usr/bin/grep -F -q \
    'Thresholds: CPU 90.0%, Load 1.50/CPU, Memory 90.0%, Swap 80.0% with memory pressure, Disk 85.0%' \
    "$output"

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

echo "PASS: Ubuntu package smoke under 2GB limit; RSS=${rss_kib}KiB; SQLite=${database_bytes}B"
