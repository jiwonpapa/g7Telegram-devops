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

for required in \
    usr/bin/g7tg \
    usr/lib/g7telegram-devops/g7tg-exec \
    usr/lib/systemd/system/g7tg-agent.service \
    etc/g7telegram-devops/agent.toml \
    etc/g7telegram-devops/allowed-units \
    etc/sudoers.d/g7telegram-devops
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

echo "PASS: package structure and permissions"

