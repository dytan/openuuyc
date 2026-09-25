# Continue OpenUUYC Linux work on a new physical box

**Status (2026-09-25):** Build + GUI shell + soak docs are done on the development machine.
**Blocked on:** a real Linux desktop with Secret Service, display, network, and a UU account for live `login` → `devices` → `connect` soak.

Identity stays **OpenUUYC** Linux controller support (upstream-shaped feature), not a separate fork product. Respect `LICENSE` / `THIRD_PARTY_NOTICES`. Do not push to `origin` unless you intend to open the collab PR.

Companion soak details: [`LINUX.md`](LINUX.md).

---

## 0. What “done” means so far

| Milestone | State |
|-----------|--------|
| M0 Linux `cargo build` | Done (`x86_64-unknown-linux-gnu`) |
| M1 auth/devices CLI scaffolding | Done; live session needs keyring |
| M2 egui-winit + glow control center + CPU RGBA viewer path | Wired; not live-soak tested |
| Connect soak checklist | Documented in `LINUX.md` |
| Live QR login + remote frames/input | **Your next job on the physical box** |

Local tip of work (not on GitHub yet):

```
7fe1e6e docs(linux): connect soak checklist; auth/GL harden
8c04fef feat(linux): egui-glow GUI shell and CPU RGBA viewer path
b12086a feat(linux): initial controller compile path for native Linux
```

Base upstream tip these sit on: `5a9210b` (v0.7.0 area on `origin/main`).

---

## 1. Bring the Linux commits onto the new box

The three commits were **never pushed**. Pick one transfer method.

### Option A — git bundle (recommended)

On the machine that already has the work (`/workspace/openuuyc` or a copy):

```bash
# already produced under handoff/ if you cloned from that tree:
#   handoff/openuuyc-linux-ahead3.bundle
```

On the **new** box:

```bash
git clone https://github.com/djkcyl/openuuyc.git
cd openuuyc
git fetch ../path/to/openuuyc-linux-ahead3.bundle HEAD:linux-controller
# or, if the bundle is copied next to the repo:
git fetch ./handoff/openuuyc-linux-ahead3.bundle HEAD:linux-controller
git checkout linux-controller
# expect tip: 7fe1e6e
```

### Option B — format-patch series

```bash
# new box, clean clone on origin/main:
git clone https://github.com/djkcyl/openuuyc.git
cd openuuyc
git am /path/to/handoff/linux-commits/*.patch
```

Patches live in `handoff/linux-commits/` in this tree:

- `0001-feat-linux-initial-controller-compile-path-for-nativ.patch`
- `0002-feat-linux-egui-glow-GUI-shell-and-CPU-RGBA-viewer-p.patch`
- `0003-docs-linux-connect-soak-checklist-auth-GL-harden.patch`

### Option C — copy the whole working tree

```bash
rsync -a --exclude target/ /path/from/openuuyc/ ~/src/openuuyc/
cd ~/src/openuuyc
```

Then rebuild (section 3). Prefer A/B if you want a clean git history.

---

## 2. Host prerequisites

### OS / desktop

- Linux **x86_64** with a logged-in graphical session (X11 `DISPLAY` or Wayland).
- User D-Bus session bus + **unlocked Secret Service** (e.g. gnome-keyring).  
  There is **no plaintext credential fallback**.

Check:

```bash
echo "DISPLAY=$DISPLAY"
echo "WAYLAND_DISPLAY=$WAYLAND_DISPLAY"
echo "DBUS_SESSION_BUS_ADDRESS=$DBUS_SESSION_BUS_ADDRESS"
# expect a unix:path=... bus; often $XDG_RUNTIME_DIR/bus exists
ls -l "$XDG_RUNTIME_DIR/bus"
```

### Packages (Debian / Ubuntu)

```bash
sudo apt-get update
sudo apt-get install -y \
  build-essential pkg-config cmake \
  libegl1 libegl-dev \
  libxkbcommon0 libxkbcommon-x11-0 \
  libxcb-xkb1 \
  libwayland-client0 \
  gnome-keyring libsecret-1-0 \
  dbus-user-session
```

Missing `libxkbcommon-x11.so` panics the GUI on X11 via `xkbcommon-dl`.

### Rust

Need **rustc/cargo 1.98+** (egui 0.36 / edition 2024 crates). Distro 1.85 is too old.

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source "$HOME/.cargo/env"
rustc --version   # >= 1.98
```

### Account / network

- UU remote mobile app to scan the CLI/GUI QR.
- Network to the same UU/NRD endpoints the Windows client uses.

---

## 3. Build and smoke (no UU account required)

```bash
cd openuuyc
source "$HOME/.cargo/env"
cargo build

BIN=./target/debug/OpenUUYC
$BIN native-status
$BIN contracts
$BIN rtc-selftest
$BIN auth-status
# With Secret Service unlocked, expect:
#   platform credential store: available
# Without it, expect a clear Linux hint (fail is OK until keyring works)

$BIN gui --codec h264 --hardware-decode false
# Window title: 「OpenUUYC · 控制中心」 — close after it stays up
```

App data: `$XDG_DATA_HOME/openuuyc` or `~/.local/share/openuuyc`.

---

## 4. Live connect soak (this is the next milestone)

Full flag rationale and expected windows: [`LINUX.md`](LINUX.md) § Connect soak checklist.

```bash
BIN=./target/debug/OpenUUYC

$BIN auth-status
$BIN login                    # Unicode QR in terminal; scan with UU phone
$BIN devices                  # pick exact unique device name → DEVICE=

$BIN connect "$DEVICE" \
  --mute --codec h264 --hardware-decode false --transport auto
# optional: --device-id <id>

# or GUI:
$BIN gui --codec h264 --hardware-decode false --transport auto

# later:
$BIN logout-local
```

### Pass criteria for this stage

1. `auth-status` shows credential store **available**; after login, session **present**.
2. `devices` lists your bound host(s).
3. `connect` (or GUI connect) opens connecting → playback window.
4. Soft-decoded **H.264** frames appear (CPU RGBA presenter).
5. In-window mouse/keyboard reach the remote (`RemoteInput`).
6. Ctrl+C / window close leaves cleanly.

Record failures with: OS, session type (X11/Wayland), rustc version, exact command, stderr, whether keyring was unlocked. Strip tokens / QR payloads from logs.

### Deliberately later

- Unmute audio soak (`cpal` + NetEq).
- H.265 / VA-API.
- Clipboard, plugins, file transfer polish.
- Changing Linux default away from software decode.

---

## 5. After a successful soak — suggested code follow-ups

Keep Windows `cfg` paths untouched unless a change is truly shared.

1. Fix whatever broke in live connect (GL current context, session refresh, device name matching, leave/teardown).
2. Audio path with mute still default in soak scripts.
3. Harden multi-window EGL (`make_current` before paint/swap/destroy — already started in `src/ui/linux.rs` / `src/viewer/linux/windows_presenter.rs`).
4. Shape an upstream PR branch when collab access allows (`feat/linux-controller` or similar); do not rewrite history of the three commits without reason.
5. Refresh `LINUX.md` “what works” from soak evidence.

---

## 6. Constraints / do-not

- Do **not** strip `LICENSE` or `THIRD_PARTY_NOTICES`.
- Do **not** ship this as a competing product brand; keep OpenUUYC naming and upstream-PR intent.
- Do **not** add a plaintext session file “just for the box”.
- Do **not** force `--hardware-decode true` on Linux until VA-API (or similar) exists.
- Prefer local commits; push only when opening the agreed upstream PR.

---

## 7. Quick file map (Linux-touched)

| Area | Paths |
|------|--------|
| Docs | `LINUX.md`, this file, `handoff/` |
| Auth / login hints | `src/auth.rs`, `src/login.rs` |
| Paths (XDG) | `src/paths.rs` |
| HW-decode default | `src/media.rs`, `src/main.rs` (`default_hardware_decode()` → false on Linux) |
| GUI shell | `src/ui/linux.rs` |
| Viewer presenter | `src/viewer/linux/windows_presenter.rs` |
| Platform gates | `build.rs`, `src/lib.rs`, NetEq Linux defines |

---

## 8. One-liner checklist

```text
[ ] Transfer commits (bundle / am / rsync)
[ ] rustc >= 1.98, apt EGL/xkb/keyring packages
[ ] Graphical session + unlocked Secret Service
[ ] cargo build && native-status / contracts / rtc-selftest
[ ] auth-status → login → devices → connect --mute --codec h264 --hardware-decode false
[ ] Confirm frames + input; note bugs
[ ] Update LINUX.md; prepare upstream PR when allowed
```
