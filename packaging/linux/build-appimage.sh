#!/usr/bin/env bash
# Build a thin x86_64 AppImage (binary + desktop + icons).
# Host still needs libva/GPU drivers, fuse3, Vulkan — same as the tarball.
#
# Usage (repo root):
#   ./packaging/linux/build-appimage.sh
#   OPENUUYC_BIN=/path/to/OpenUUYC ./packaging/linux/build-appimage.sh
#
# Output: dist/OpenUUYC-x86_64-<version>.AppImage (+ .sha256)
#
# Downloads appimagetool into dist/tools/ on first run (override with
# APPIMAGETOOL=/path/to/appimagetool).
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd "${SCRIPT_DIR}/../.." && pwd)"
cd "${ROOT}"

VERSION="${VERSION:-"$("${SCRIPT_DIR}/version.sh")"}"
OUT_DIR="${OUT_DIR:-${ROOT}/dist}"
APPDIR="${OUT_DIR}/OpenUUYC.AppDir"
TOOLS_DIR="${OUT_DIR}/tools"
APPIMAGE_NAME="OpenUUYC-x86_64-${VERSION}.AppImage"
APPIMAGETOOL_URL="${APPIMAGETOOL_URL:-https://github.com/AppImage/appimagetool/releases/download/continuous/appimagetool-x86_64.AppImage}"

resolve_bin() {
  if [[ -n "${OPENUUYC_BIN:-}" && -x "${OPENUUYC_BIN}" ]]; then
    echo "${OPENUUYC_BIN}"
    return
  fi
  if [[ -x "${ROOT}/target/release/OpenUUYC" ]]; then
    echo "${ROOT}/target/release/OpenUUYC"
    return
  fi
  if [[ -x "${ROOT}/OpenUUYC" ]]; then
    echo "${ROOT}/OpenUUYC"
    return
  fi
  echo "error: OpenUUYC binary not found; build release or set OPENUUYC_BIN" >&2
  exit 1
}

BIN_SRC="$(resolve_bin)"
mkdir -p "${TOOLS_DIR}"

if [[ -z "${APPIMAGETOOL:-}" ]]; then
  APPIMAGETOOL="${TOOLS_DIR}/appimagetool-x86_64.AppImage"
  if [[ ! -x "${APPIMAGETOOL}" ]]; then
    echo "Downloading appimagetool..."
    curl -fsSL --retry 5 --retry-delay 2 --connect-timeout 30 --max-time 180       -o "${APPIMAGETOOL}" "${APPIMAGETOOL_URL}"
    chmod +x "${APPIMAGETOOL}"
  fi
fi

rm -rf "${APPDIR}"
mkdir -p \
  "${APPDIR}/usr/bin" \
  "${APPDIR}/usr/share/applications" \
  "${APPDIR}/usr/share/icons/hicolor" \
  "${APPDIR}/usr/share/doc/openuuyc"

install -m 755 "${BIN_SRC}" "${APPDIR}/usr/bin/OpenUUYC"
ln -sfn OpenUUYC "${APPDIR}/usr/bin/openuuyc"

# Desktop entry: Exec must be a bare name for appimagetool; AppRun defaults to gui.
install -m 644 "${SCRIPT_DIR}/openuuyc.desktop" "${APPDIR}/openuuyc.desktop"
# Ensure Exec=OpenUUYC (appimagetool rewrites); keep gui via AppRun default.
sed -i 's/^Exec=.*/Exec=OpenUUYC/' "${APPDIR}/openuuyc.desktop"
install -m 644 "${APPDIR}/openuuyc.desktop" \
  "${APPDIR}/usr/share/applications/openuuyc.desktop"

if [[ -d "${SCRIPT_DIR}/icons/hicolor" ]]; then
  cp -a "${SCRIPT_DIR}/icons/hicolor/." "${APPDIR}/usr/share/icons/hicolor/"
fi
# Top-level icon for appimagetool / desktop environments
if [[ -f "${SCRIPT_DIR}/icons/hicolor/256x256/apps/openuuyc.png" ]]; then
  install -m 644 "${SCRIPT_DIR}/icons/hicolor/256x256/apps/openuuyc.png" \
    "${APPDIR}/openuuyc.png"
fi

install -m 644 "${SCRIPT_DIR}/INSTALL.txt" \
  "${APPDIR}/usr/share/doc/openuuyc/INSTALL.txt"

cat > "${APPDIR}/AppRun" << 'APPRUN'
#!/usr/bin/env bash
set -euo pipefail
HERE="$(dirname "$(readlink -f "${0}")")"
BIN="${HERE}/usr/bin/OpenUUYC"
if [[ $# -eq 0 ]]; then
  exec "${BIN}" gui
else
  exec "${BIN}" "$@"
fi
APPRUN
chmod 755 "${APPDIR}/AppRun"

mkdir -p "${OUT_DIR}"
OUT_APPIMAGE="${OUT_DIR}/${APPIMAGE_NAME}"
rm -f "${OUT_APPIMAGE}"

# CI / containers often lack FUSE for running AppImages; extract-and-run.
export ARCH=x86_64
export APPIMAGE_EXTRACT_AND_RUN="${APPIMAGE_EXTRACT_AND_RUN:-1}"
"${APPIMAGETOOL}" "${APPDIR}" "${OUT_APPIMAGE}"

(cd "${OUT_DIR}" && sha256sum "${APPIMAGE_NAME}" > "${APPIMAGE_NAME}.sha256")
# Keep AppDir for inspection; optional cleanup:
# rm -rf "${APPDIR}"

echo "Built ${OUT_APPIMAGE}"
ls -lh "${OUT_APPIMAGE}" "${OUT_APPIMAGE}.sha256"
cat "${OUT_APPIMAGE}.sha256"
