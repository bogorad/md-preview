#!/usr/bin/env bash
#
# Merge the newest upstream desktop release into the current checkout.
#
# Writes `tag` and `needed` to $GITHUB_OUTPUT. When needed=true the merge commit
# is left on HEAD for the caller to push; when needed=false HEAD is untouched.
#
# Usage: sync-upstream.sh [tag]
#   tag  optional explicit upstream tag; blank selects the newest release.

set -euo pipefail

UPSTREAM_REPO=vorojar/md-preview
UPSTREAM_URL=https://github.com/${UPSTREAM_REPO}.git

requested=${1:-}

if [[ -n $requested ]]; then
  tag=$requested
  echo "Using the requested tag: $tag"
else
  # Newest published, non-draft, non-prerelease release whose tag is a plain
  # vX.Y.Z.
  #
  # The regex is not cosmetic. Upstream publishes the Android app from the same
  # repository under mobile-android-v* tags; merging one of those would package
  # a different product. Anything that is not exactly vMAJOR.MINOR.PATCH is
  # ignored on purpose.
  tag=$(gh api "repos/${UPSTREAM_REPO}/releases" --paginate --jq '
    [ .[]
      | select(.draft == false and .prerelease == false)
      | select(.tag_name | test("^v[0-9]+\\.[0-9]+\\.[0-9]+$"))
    ]
    | sort_by(.published_at)
    | last
    | .tag_name
  ')
  echo "Newest upstream desktop release: ${tag:-<none>}"
fi

if [[ -z $tag || $tag == "null" ]]; then
  echo "::error::could not determine an upstream release tag"
  exit 1
fi

echo "tag=$tag" >>"$GITHUB_OUTPUT"

# Fetch the tag without creating a local one. A local v* tag could later be
# pushed by accident, and upstream's own release.yml fires on `push: tags: v*`,
# which would try to cut a release in this fork.
git fetch --no-tags "$UPSTREAM_URL" "refs/tags/${tag}"
target=$(git rev-parse "FETCH_HEAD^{commit}")
echo "$tag resolves to $target"

if git merge-base --is-ancestor "$target" HEAD; then
  echo "needed=false" >>"$GITHUB_OUTPUT"
  echo "Already up to date: $tag is an ancestor of HEAD. Nothing to do."
  exit 0
fi

echo "needed=true" >>"$GITHUB_OUTPUT"

git config user.name "github-actions[bot]"
git config user.email "41898282+github-actions[bot]@users.noreply.github.com"

# --no-ff so the merge is always an explicit, revertible commit, even in the
# case where our packaging commits happen to not have diverged.
git merge --no-ff -m "Merge upstream release ${tag}" "$target"

# The Nix package takes its version from Cargo.toml, not from the tag, so a
# mismatch is worth surfacing without failing the run.
version=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)
if [[ $version != "${tag#v}" ]]; then
  echo "::warning::Cargo.toml says $version but the release tag is $tag;" \
       "the package will be built as md-preview-$version"
fi

echo "Merged $tag. Cargo.toml version is $version."
