#!/bin/sh
set -eu

repository=$(unset CDPATH; cd -- "$(dirname -- "$0")/.." && pwd)
cd "$repository"

for command in cargo docker jq; do
    command -v "$command" >/dev/null 2>&1 || {
        echo "required command not found: $command" >&2
        exit 69
    }
done

crate_version=$(cargo metadata --no-deps --format-version 1 \
    | jq -r '.packages[] | select(.name == "g7tg-agent") | .version')
version=${1:-$crate_version}
case "$version" in
    ''|*[!0-9A-Za-z.-]*)
        echo "invalid version: $version" >&2
        exit 64
        ;;
esac
[ "$crate_version" = "$version" ] || {
    echo "Cargo version $crate_version does not match requested version $version" >&2
    exit 1
}

image=g7telegram-devops-release:rust-1.96.0
package="g7telegram-devops_${version}_amd64.deb"
mkdir -p "$repository/dist"

docker build \
    --platform linux/amd64 \
    --tag "$image" \
    "$repository/packaging/release"

docker run --rm \
    --platform linux/amd64 \
    --volume "$repository:/workspace:ro" \
    --volume "$repository/dist:/dist" \
    --volume g7telegram-devops-cargo-registry:/opt/cargo/registry \
    --volume g7telegram-devops-amd64-target:/workspace/target \
    --workdir /workspace \
    "$image" \
    sh -ceu '
        version=$1
        package=$2
        debian_version=$(printf "%s\n" "$version" | sed "s/-/~/")
        scripts/check.sh
        cargo deb -p g7tg-agent
        built=$(find target/debian -maxdepth 1 -type f \
            -name "g7telegram-devops_${debian_version}-*_amd64.deb" -print -quit)
        test -n "$built"
        cp "$built" "/dist/$package"
        scripts/check-package.sh "/dist/$package"
        cd /dist
        sha256sum -- "$package" > SHA256SUMS
    ' sh "$version" "$package"

for ubuntu in 22.04 24.04; do
    docker run --rm \
        --platform linux/amd64 \
        --memory 2g \
        --volume "$repository/dist:/dist:ro" \
        --volume "$repository/scripts:/workspace-scripts:ro" \
        "ubuntu:$ubuntu" \
        /workspace-scripts/container-smoke.sh "/dist/$package"
done

echo "PASS: local amd64 package $repository/dist/$package"
