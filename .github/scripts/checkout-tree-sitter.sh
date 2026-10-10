#!/usr/bin/env bash
# Use the same reviewed grammar sources in Debug CI, Release proof and releases.
set -euo pipefail
package_root=${1:-apps/purepoint-macos/LocalPackages}
mkdir -p "$package_root"
checkout_package() {
  local repo=$1 revision=$2 destination="$package_root/$3"
  git init --quiet "$destination"
  git -C "$destination" remote add origin "https://github.com/$repo.git"
  git -C "$destination" fetch --quiet --depth=1 origin "$revision"
  git -C "$destination" checkout --quiet --detach "$revision"
  test "$(git -C "$destination" rev-parse HEAD)" = "$revision"
}
checkout_package tree-sitter/tree-sitter-css dda5cfc5722c429eaba1c910ca32c2c0c5bb1a3f tree-sitter-css
checkout_package tree-sitter/tree-sitter-javascript 58404d8cf191d69f2674a8fd507bd5776f46cb11 tree-sitter-javascript
checkout_package tree-sitter-grammars/tree-sitter-lua 10fe0054734eec83049514ea2e718b2a56acd0c9 tree-sitter-lua
checkout_package tree-sitter/tree-sitter-python 26855eabccb19c6abf499fbc5b8dc7cc9ab8bc64 tree-sitter-python
checkout_package tree-sitter-grammars/tree-sitter-yaml a1c4812a73ec5e089de8e441fdea3a921e8d5079 tree-sitter-yaml
