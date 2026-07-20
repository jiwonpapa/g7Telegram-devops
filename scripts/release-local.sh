#!/bin/sh
set -eu

repository=$(unset CDPATH; cd -- "$(dirname -- "$0")/.." && pwd)
cd "$repository"

for command in cargo gh git jq; do
    command -v "$command" >/dev/null 2>&1 || {
        echo "required command not found: $command" >&2
        exit 69
    }
done

[ "$(git branch --show-current)" = main ] || {
    echo "local release must run from main" >&2
    exit 1
}
[ -z "$(git status --porcelain)" ] || {
    echo "working tree must be clean" >&2
    exit 1
}

crate_version=$(cargo metadata --no-deps --format-version 1 \
    | jq -r '.packages[] | select(.name == "g7tg-agent") | .version')
version=${1:-$crate_version}
[ "$version" = "$crate_version" ] || {
    echo "Cargo version $crate_version does not match requested version $version" >&2
    exit 1
}
tag=v$version
package="dist/g7telegram-devops_${version}_amd64.deb"

scripts/verify-local.sh
scripts/build-package-local.sh "$version"

head_commit=$(git rev-parse HEAD)
if git rev-parse --verify --quiet "refs/tags/$tag" >/dev/null; then
    [ "$(git rev-list -n 1 "$tag")" = "$head_commit" ] || {
        echo "tag $tag already points to another commit" >&2
        exit 1
    }
else
    git tag -a "$tag" -m "$tag"
fi

git push origin main
git push origin "$tag"

if gh release view "$tag" >/dev/null 2>&1; then
    echo "GitHub Release already exists: $tag" >&2
    exit 1
fi
gh release create "$tag" \
    "$package" \
    dist/SHA256SUMS \
    --generate-notes \
    --verify-tag

echo "PASS: local release $tag"
if [ -n "${G7TG_DEPLOY_TARGET:-}" ]; then
    scripts/deploy-local.sh "$G7TG_DEPLOY_TARGET" "$version"
fi
