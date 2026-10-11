#!/usr/bin/env bash
# Rewrite pkgver= in PKGBUILD from Cargo.toml.
set -euo pipefail
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd "${SCRIPT_DIR}/../.." && pwd)"
VER="$("${ROOT}/packaging/linux/version.sh")"
# pacman forbids '-' in pkgver: 1.0.0-rc.2 -> 1.0.0rc2 (sorts before 1.0.0).
PKGVER="${VER}"
if [[ "${VER}" == *-* ]]; then
  PKGVER="${VER%%-*}$(printf '%s' "${VER#*-}" | tr -d '.-')"
fi
for f in PKGBUILD PKGBUILD-bin; do
  sed -i "s/^pkgver=.*/pkgver=${PKGVER}/" "${SCRIPT_DIR}/${f}"
done
sed -i "s/^_upstreamver=.*/_upstreamver=${VER}/" "${SCRIPT_DIR}/PKGBUILD-bin"
echo "PKGBUILD pkgver=${PKGVER} (upstream ${VER})"
