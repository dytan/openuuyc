#!/usr/bin/env bash
# Rewrite pkgver= in PKGBUILD from Cargo.toml.
set -euo pipefail
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd "${SCRIPT_DIR}/../.." && pwd)"
VER="$("${ROOT}/packaging/linux/version.sh")"
sed -i "s/^pkgver=.*/pkgver=${VER}/" "${SCRIPT_DIR}/PKGBUILD"
echo "PKGBUILD pkgver=${VER}"
