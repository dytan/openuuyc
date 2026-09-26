#!/usr/bin/env bash
# Build an amd64 .deb from a release binary + desktop/icons (Ubuntu 22.04 baseline).
#
# Usage (repo root):
#   ./packaging/linux/build-deb.sh
#   OPENUUYC_BIN=/path/to/OpenUUYC VERSION=0.7.0 ./packaging/linux/build-deb.sh
#
# Output: dist/openuuyc_<version>_amd64.deb (+ .sha256)
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd "${SCRIPT_DIR}/../.." && pwd)"
cd "${ROOT}"

VERSION="${VERSION:-"$("${SCRIPT_DIR}/version.sh")"}"
ARCH_DEB="amd64"
PKG_NAME="openuuyc"
OUT_DIR="${OUT_DIR:-${ROOT}/dist}"
STAGE="${OUT_DIR}/deb-root"
DEBIAN_DIR="${STAGE}/DEBIAN"

if ! command -v dpkg-deb >/dev/null 2>&1; then
  echo "error: dpkg-deb not found (install dpkg)" >&2
  exit 1
fi

rm -rf "${STAGE}"
mkdir -p "${DEBIAN_DIR}"

# Stage FHS tree under /usr
DESTDIR="${STAGE}" PREFIX=/usr OPENUUYC_BIN="${OPENUUYC_BIN:-}" \
  "${SCRIPT_DIR}/install.sh"

# Docs
install -d "${STAGE}/usr/share/doc/${PKG_NAME}"
install -m 644 "${SCRIPT_DIR}/INSTALL.txt" \
  "${STAGE}/usr/share/doc/${PKG_NAME}/INSTALL.txt"
if [[ -f "${ROOT}/LICENSE" ]]; then
  install -m 644 "${ROOT}/LICENSE" \
    "${STAGE}/usr/share/doc/${PKG_NAME}/copyright"
fi

# Hard-linked libs from ldd of the Ubuntu/Arch release binary, plus fuse3 for
# clipboard file offer. GPU VA-API / Vulkan ICDs stay on the host (Recommends).
cat > "${DEBIAN_DIR}/control" << CTRL
Package: ${PKG_NAME}
Version: ${VERSION}
Section: net
Priority: optional
Architecture: ${ARCH_DEB}
Maintainer: OpenUUYC Linux packagers <linux-port@local>
Homepage: https://github.com/dytan/openuuyc
Depends: libc6 (>= 2.35), libstdc++6, libgcc-s1, libxcb1, libxau6, libxdmcp6, libva2, libva-drm2, libdrm2, libasound2
Recommends: libfuse3-3 | fuse3, libvulkan1, mesa-vulkan-drivers | vulkan-icd, mesa-va-drivers | intel-media-va-driver | i965-va-driver, fonts-noto-cjk | fonts-noto-cjk-extra
Description: Native Linux controller for UU Remote (OpenUUYC)
 OpenUUYC is a third-party Rust client for UU Remote protocol
 interoperability. This package installs the Linux GUI controller
 (viewer/controller only — not a Linux host/agent).
 .
 Runtime VA-API hardware decode needs working local libva GPU drivers.
 See /usr/share/doc/${PKG_NAME}/INSTALL.txt.
CTRL

# Installed-size in KiB
INSTALLED_SIZE="$(du -sk "${STAGE}" | awk '{print $1}')"
echo "Installed-Size: ${INSTALLED_SIZE}" >> "${DEBIAN_DIR}/control"

chmod 755 "${DEBIAN_DIR}"
find "${STAGE}" -type d -exec chmod 755 {} +

DEB_FILE="${OUT_DIR}/${PKG_NAME}_${VERSION}_${ARCH_DEB}.deb"
mkdir -p "${OUT_DIR}"
# fakeroot helps produce root-owned metadata without needing root
if command -v fakeroot >/dev/null 2>&1; then
  fakeroot dpkg-deb --build --root-owner-group "${STAGE}" "${DEB_FILE}"
else
  dpkg-deb --build --root-owner-group "${STAGE}" "${DEB_FILE}"
fi

(cd "${OUT_DIR}" && sha256sum "$(basename "${DEB_FILE}")" > "$(basename "${DEB_FILE}").sha256")
rm -rf "${STAGE}"

echo "Built ${DEB_FILE}"
ls -lh "${DEB_FILE}" "${DEB_FILE}.sha256"
cat "${DEB_FILE}.sha256"
