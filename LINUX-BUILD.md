# Building OpenUUYC on Linux

Tested on a Linux workstation running **Arch Linux**, `rustc 1.98.1`.
Package lists below are grounded in a working Arch install; Debian/Fedora
names are the usual equivalents and should be verified on first build.

## Rust

```bash
# rustup recommended
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
rustc --version   # known good: 1.98.1
```

## System packages

### Arch Linux (verified)

```bash
sudo pacman -S --needed \
  base-devel clang pkgconf openssl \
  libxkbcommon libx11 libxi libxcursor libxrandr libxss \
  wayland wayland-protocols \
  mesa vulkan-icd-loader \
  libva \
  fuse3 \
  fontconfig noto-fonts-cjk
```

Optional GPU ICD (pick for your GPU), e.g. `vulkan-intel` / `vulkan-radeon` /
`nvidia-utils`.

### Debian / Ubuntu (usually needed)

```bash
sudo apt update
sudo apt install --no-install-recommends \
  build-essential pkg-config clang \
  libssl-dev \
  libxkbcommon-dev libx11-dev libxi-dev libxcursor-dev libxrandr-dev libxss-dev \
  libwayland-dev wayland-protocols \
  libgl1-mesa-dev libvulkan-dev \
  libva-dev \
  libfuse3-dev \
  fonts-noto-cjk
```

Exact package names can vary by release; if `cargo build` complains about a
missing `.pc` file, install the matching `-dev` package.

### Fedora (usually needed)

```bash
sudo dnf install \
  gcc gcc-c++ make pkgconf-pkg-config clang \
  openssl-devel \
  libxkbcommon-devel libX11-devel libXi-devel libXcursor-devel libXrandr-devel libXScrnSaver-devel \
  wayland-devel wayland-protocols-devel \
  mesa-libGL-devel vulkan-loader-devel \
  libva-devel \
  fuse3-devel \
  google-noto-sans-cjk-fonts
```

### Generic notes

- **bindgen / cros-libva** needs `clang` + `libclang` and `pkg-config` able to
  find `libva`.
- **winit / egui-wgpu** need X11 and/or Wayland client libs; Arch/Hyprland
  users typically have both.
- **FUSE clipboard files** need `fuse3` (users in the `fuse` group / user_allow_other
  per distro policy if mounts fail).
- **Fonts**: Simplified Chinese UI expects Noto Sans CJK SC. For
  `NotoSansCJK-Regular.ttc` the face index is **2** (0=JP 1=KR 2=SC 3=TC 4=HK).

## Build

```bash
git clone <repo> openuuyc && cd openuuyc   # or use an existing worktree
cargo build                 # debug → target/debug/OpenUUYC
cargo build --release       # release → target/release/OpenUUYC
```

## Run

```bash
# GUI (inherit DISPLAY/WAYLAND_DISPLAY/DBUS from your session)
./target/debug/OpenUUYC gui

# Soak-oriented defaults
./target/debug/OpenUUYC gui \
  --codec h264 --hardware-decode true --transport auto --auto-mouse-control true

./target/debug/OpenUUYC --help
./target/debug/OpenUUYC gui --help
```

Auth stores tokens in the platform Secret Service. On headless SSH you must
forward/import the user D-Bus session.

## Troubleshooting

| Symptom | Check |
|---------|--------|
| `libva` / bindgen errors | `pkg-config --modversion libva`; install `libva-dev` / `libva` + clang |
| wgpu adapter missing | Vulkan ICD installed; `vulkaninfo` briefly |
| CJK glyphs wrong (JP forms) | Ensure SC face index 2 path exists under `/usr/share/fonts/...` |
| Clipboard file paste fails | `fuse3` installed; session permits FUSE mounts |
| Duplicate instance | flock under `$XDG_RUNTIME_DIR`; stale locks clear on logout |



## Install (desktop / app launcher)

After `cargo build --release --locked`:

```bash
# System-wide (default PREFIX=/usr/local)
sudo ./packaging/linux/install.sh

# Per-user
PREFIX="$HOME/.local" ./packaging/linux/install.sh
```

This installs the binary onto `PATH`, a `.desktop` entry, and hicolor icons so
desktop launchers can find **OpenUUYC**. Prebuilt release tarballs ship the same
`install.sh` at the archive root — see `packaging/linux/INSTALL.txt`.

## CI / prebuilt binaries

GitHub Actions on this fork (`.github/workflows/linux-release.yml`) builds a
release binary on **Ubuntu 22.04** and packages
`OpenUUYC-linux-x86_64-<version>.tar.gz` (binary, desktop entry/icons, `install.sh`, `INSTALL.txt`).

- **`workflow_dispatch`**: upload Actions artifacts only (no Release).
- **Push tag `v*`** (e.g. `v0.7.0` matching `Cargo.toml`): create a GitHub Release.

Consumers on a compatible x86_64 glibc distro can run the binary without a local
Rust toolchain. They still need runtime packages (libva + drivers, fuse3,
Vulkan/Mesa, ALSA — see `packaging/linux/INSTALL.txt`). Debug `cargo build`
outputs are not portable across distros; use the release artifact or compile on
the target machine for development.
