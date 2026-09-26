# Linux distributable packages

Beyond the release tarball + `install.sh`, this tree can produce:

| Format | Builder | Typical consumer |
|--------|---------|------------------|
| `.tar.gz` | `.github/workflows/linux-release.yml` | Any x86_64 glibc ≥ 2.35 |
| `.AppImage` | `packaging/linux/build-appimage.sh` (+ CI) | Portable single file |
| `.deb` | `packaging/linux/build-deb.sh` (+ CI) | Debian / Ubuntu |
| Arch `.pkg.tar.zst` | `packaging/arch/PKGBUILD` via `makepkg` | Arch / Omarchy / derivatives |

Flatpak and `.rpm` are not packaged yet (low priority once the above work).

All formats install (or embed) the `OpenUUYC` binary, a FreeDesktop `.desktop`
entry (`Exec` launches the GUI), and hicolor icons. **VA-API / Vulkan GPU
drivers are never bundled** — install them from the host distro (see
`INSTALL.txt`).

Version always comes from the first `version =` in `Cargo.toml` (currently
keep in sync with release tags `v*`).

## Quick install one-liners

### AppImage

```bash
chmod +x OpenUUYC-x86_64-<version>.AppImage
./OpenUUYC-x86_64-<version>.AppImage          # defaults to GUI
./OpenUUYC-x86_64-<version>.AppImage --help
```

Host still needs runtime libs (libva + GPU drivers, fuse3, Vulkan/Mesa, ALSA).

### Debian / Ubuntu (`.deb`)

```bash
sudo apt install ./openuuyc_<version>_amd64.deb
# or: sudo dpkg -i openuuyc_<version>_amd64.deb && sudo apt -f install
OpenUUYC gui
```

Built against an Ubuntu 22.04 (glibc 2.35) baseline — same as the CI tarball.

### Arch Linux / Omarchy

From a checkout (builds with `cargo` on `PATH` — pacman `cargo` or rustup):

```bash
cd packaging/arch
./sync-pkgver.sh
makepkg -f
# If rustup provides cargo (not the pacman package): makepkg -f -d
# Reuse an existing target/release/OpenUUYC: OPENUUYC_SKIP_BUILD=1 makepkg -f -d
sudo pacman -U openuuyc-<version>-1-x86_64.pkg.tar.zst
```

Or install a prebuilt release tarball without compiling:

```bash
# after downloading OpenUUYC-linux-x86_64-<version>.tar.gz next to PKGBUILD-bin
cd packaging/arch
makepkg -f -p PKGBUILD-bin
sudo pacman -U openuuyc-bin-<version>-1-x86_64.pkg.tar.zst
```

AUR publish is optional and needs an AUR account; the in-tree PKGBUILD is enough
for `pacman -U` / local repo use.

### Tarball + install.sh (existing)

```bash
tar -xzf OpenUUYC-linux-x86_64-<version>.tar.gz
cd OpenUUYC-linux-x86_64-<version>
sudo ./install.sh
# or: PREFIX="$HOME/.local" ./install.sh
```

## Build locally (developers)

```bash
cargo build --release --locked
./packaging/linux/build-deb.sh
./packaging/linux/build-appimage.sh
# Arch package:
cd packaging/arch && ./sync-pkgver.sh && makepkg -f
```

Outputs land under `dist/` (gitignored), except Arch packages which land in
`packaging/arch/` by makepkg default.

## CI

`linux-release.yml` on Ubuntu 22.04 builds and uploads:

- `OpenUUYC-linux-x86_64-<ver>.tar.gz` (+ sha256)
- `openuuyc_<ver>_amd64.deb` (+ sha256)
- `OpenUUYC-x86_64-<ver>.AppImage` (+ sha256)

Arch `.pkg.tar.zst` is **not** built in Ubuntu CI (use `makepkg` on Arch, or a
future Arch container job). Tag pushes `v*` attach the portable/deb artifacts to
the GitHub Release.
