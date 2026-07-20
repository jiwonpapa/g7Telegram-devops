#!/bin/sh
set -eu

[ "$#" -eq 1 ] || {
    echo "usage: check-package.sh PACKAGE.deb" >&2
    exit 64
}

package=$1
[ -f "$package" ]
[ "$(/usr/bin/dpkg-deb -f "$package" Package)" = g7telegram-devops ]
[ "$(/usr/bin/dpkg-deb -f "$package" Architecture)" = amd64 ]

root=$(/usr/bin/mktemp -d)
trap '/usr/bin/rm -rf "$root"' EXIT HUP INT TERM
/usr/bin/dpkg-deb -x "$package" "$root"
/usr/bin/dpkg-deb -e "$package" "$root/DEBIAN"

for required in \
    usr/bin/g7tg \
    usr/lib/g7telegram-devops/g7tg-exec \
    usr/lib/systemd/system/g7tg-agent.service \
    etc/g7telegram-devops/agent.toml \
    etc/g7telegram-devops/allowed-units \
    etc/sudoers.d/g7telegram-devops \
    usr/share/doc/g7telegram-devops/LICENSE \
    usr/share/doc/g7telegram-devops/NOTICE
do
    [ -f "$root/$required" ] || {
        echo "missing package asset: $required" >&2
        exit 1
    }
done

[ "$(/usr/bin/stat -c %a "$root/usr/bin/g7tg")" = 755 ]
[ "$(/usr/bin/stat -c %a "$root/usr/lib/g7telegram-devops/g7tg-exec")" = 755 ]
[ "$(/usr/bin/stat -c %a "$root/etc/g7telegram-devops/agent.toml")" = 640 ]
[ "$(/usr/bin/stat -c %a "$root/etc/sudoers.d/g7telegram-devops")" = 440 ]
/usr/sbin/visudo -c -f "$root/etc/sudoers.d/g7telegram-devops" >/dev/null

config="$root/etc/g7telegram-devops/agent.toml"
for threshold in \
    'cpu_warning_percent = 90.0' \
    'load_warning_per_cpu = 1.5' \
    'memory_warning_percent = 90.0' \
    'swap_warning_percent = 80.0' \
    'disk_warning_percent = 85.0'
do
    /usr/bin/grep -F -x -q "$threshold" "$config"
done

service="$root/usr/lib/systemd/system/g7tg-agent.service"
/usr/bin/grep -F -x -q \
    'CapabilityBoundingSet=CAP_SETUID CAP_SETGID CAP_AUDIT_WRITE' \
    "$service"
/usr/bin/grep -F -x -q 'AmbientCapabilities=' "$service"
/usr/bin/grep -F -x -q 'UMask=0027' "$service"

postinst="$root/DEBIAN/postinst"
[ -f "$postinst" ]
/usr/bin/grep -F -q \
    '/usr/bin/systemctl restart g7tg-agent.service' \
    "$postinst"
if /usr/bin/grep -F -q 'try-restart g7tg-agent.service' "$postinst"; then
    echo "postinst must not hide Agent restart failures" >&2
    exit 1
fi

postrm="$root/DEBIAN/postrm"
[ -f "$postrm" ]
/usr/bin/grep -F -x -q \
    '    /usr/bin/rm -rf /etc/g7telegram-devops' \
    "$postrm"

/usr/bin/grep -F -q 'install -y --allow-downgrades' install.sh
/usr/bin/grep -F -q 'Agent health: PASS' install.sh

echo "PASS: package structure and permissions"
