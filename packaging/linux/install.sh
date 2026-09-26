#!/usr/bin/env bash
# Install OpenUUYC binary + desktop entry + icons to a standard FHS prefix.
#
# Usage (from repo root or extracted release tarball):
#   ./packaging/linux/install.sh              # PREFIX=/usr/local
#   PREFIX=/usr DESTDIR=/tmp/stage ./packaging/linux/install.sh
#   PREFIX="$HOME/.local" ./packaging/linux/install.sh
#
# Binary search order:
#   1) $OPENUUYC_BIN if set
#   2) ./OpenUUYC (release tarball layout)
#   3) ./target/release/OpenUUYC (local cargo build)
#   4) dirname(script)/../../target/release/OpenUUYC when run from packaging/linux/
set -euo pipefail

PREFIX="${PREFIX:-/usr/local}"
DESTDIR="${DESTDIR:-}"
ROOT="${DESTDIR}${PREFIX}"

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

resolve_bin() {
  if [[ -n "${OPENUUYC_BIN:-}" && -x "${OPENUUYC_BIN}" ]]; then
    echo "${OPENUUYC_BIN}"
    return
  fi
  if [[ -x "./OpenUUYC" ]]; then
    echo "$(pwd)/OpenUUYC"
    return
  fi
  if [[ -x "./target/release/OpenUUYC" ]]; then
    echo "$(pwd)/target/release/OpenUUYC"
    return
  fi
  if [[ -x "${SCRIPT_DIR}/../../target/release/OpenUUYC" ]]; then
    echo "$(cd "${SCRIPT_DIR}/../../target/release" && pwd)/OpenUUYC"
    return
  fi
  echo "error: OpenUUYC binary not found. Build with 'cargo build --release --locked'" >&2
  echo "       or extract a release tarball and run this script from that directory." >&2
  echo "       Or set OPENUUYC_BIN=/path/to/OpenUUYC" >&2
  exit 1
}

resolve_desktop() {
  if [[ -f "./openuuyc.desktop" ]]; then
    echo "./openuuyc.desktop"
  elif [[ -f "${SCRIPT_DIR}/openuuyc.desktop" ]]; then
    echo "${SCRIPT_DIR}/openuuyc.desktop"
  else
    echo "error: openuuyc.desktop not found" >&2
    exit 1
  fi
}

resolve_icons_root() {
  if [[ -d "./icons/hicolor" ]]; then
    echo "./icons"
  elif [[ -d "${SCRIPT_DIR}/icons/hicolor" ]]; then
    echo "${SCRIPT_DIR}/icons"
  else
    echo ""
  fi
}

BIN_SRC="$(resolve_bin)"
DESKTOP_SRC="$(resolve_desktop)"
ICONS_SRC="$(resolve_icons_root)"

echo "Installing OpenUUYC"
echo "  binary : ${BIN_SRC}"
echo "  prefix : ${PREFIX}"
echo "  destdir: ${DESTDIR:-"(none)"}"
echo "  root   : ${ROOT}"

install -d "${ROOT}/bin"
install -d "${ROOT}/share/applications"
install -m 755 "${BIN_SRC}" "${ROOT}/bin/OpenUUYC"
# Convenience lowercase name on PATH
ln -sfn OpenUUYC "${ROOT}/bin/openuuyc"

# Desktop entry: keep Exec as basename so PATH lookup works under PREFIX/bin
install -m 644 "${DESKTOP_SRC}" "${ROOT}/share/applications/openuuyc.desktop"

if [[ -n "${ICONS_SRC}" ]]; then
  install -d "${ROOT}/share/icons/hicolor"
  # Copy size trees (256, 512, …)
  if command -v rsync >/dev/null 2>&1; then
    rsync -a "${ICONS_SRC}/hicolor/" "${ROOT}/share/icons/hicolor/"
  else
    cp -a "${ICONS_SRC}/hicolor/." "${ROOT}/share/icons/hicolor/"
  fi
  # Ensure modes are sane after cp -a from a user tree
  find "${ROOT}/share/icons/hicolor" -type d -exec chmod 755 {} +
  find "${ROOT}/share/icons/hicolor" -type f -name 'openuuyc.png' -exec chmod 644 {} +
else
  echo "warning: no icons/ tree found; desktop entry Icon=openuuyc may be missing" >&2
fi

if command -v update-desktop-database >/dev/null 2>&1; then
  update-desktop-database "${ROOT}/share/applications" 2>/dev/null || true
fi
if command -v gtk-update-icon-cache >/dev/null 2>&1 && [[ -d "${ROOT}/share/icons/hicolor" ]]; then
  gtk-update-icon-cache -f -t "${ROOT}/share/icons/hicolor" 2>/dev/null || true
fi

echo
echo "Installed:"
echo "  ${ROOT}/bin/OpenUUYC"
echo "  ${ROOT}/bin/openuuyc -> OpenUUYC"
echo "  ${ROOT}/share/applications/openuuyc.desktop"
[[ -n "${ICONS_SRC}" ]] && echo "  ${ROOT}/share/icons/hicolor/*/apps/openuuyc.png"
echo
echo "Ensure ${PREFIX}/bin is on PATH, then launch via the app launcher as \"OpenUUYC\","
echo "or run: OpenUUYC gui"
