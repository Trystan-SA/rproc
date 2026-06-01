<div align="center">

# rproc

**A resource & process monitor for Linux, inspired by the Windows 11 Task Manager.**

Built in Rust with [`Slint`](https://slint.dev), rendered by its software backend (no GPU context) for a small memory footprint.

[![CI](https://github.com/Trystan-SA/rproc/actions/workflows/ci.yml/badge.svg)](https://github.com/Trystan-SA/rproc/actions/workflows/ci.yml)
[![Release](https://github.com/Trystan-SA/rproc/actions/workflows/release.yml/badge.svg)](https://github.com/Trystan-SA/rproc/actions/workflows/release.yml)
[![Latest release](https://img.shields.io/github/v/release/Trystan-SA/rproc?sort=semver)](https://github.com/Trystan-SA/rproc/releases/latest)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](#license)
![Platform: Linux](https://img.shields.io/badge/platform-Linux-informational)

<img src="img/capture1.png" alt="Performance view" width="900">

</div>

## Features

- **Processes**: CPU, memory, disk I/O, threads and status, with app icons from the freedesktop icon theme. Sort, filter and kill.
- **Performance**: live charts for CPU (global + per-core), memory, disks, network and GPU (NVIDIA / AMD / Intel).
- **Per-process graph attribution** *(opt-in)*: hover a point on a Performance graph to see the top 5 processes behind that sample (CPU, RAM, disk and GPU). Off by default; enable it in Settings.
- **Startup**: XDG autostart entries and enabled systemd units.
- **Services**: systemctl system and user units.
- **Settings**: adjustable refresh rate, GPU monitoring toggle (off avoids loading NVML/CUDA, ~20 MB), background-history toggle and the per-process attribution toggle.

### RAM Usage

rproc is far more memory-frugal than most monitors with similar features. The
Slint software renderer needs no OpenGL driver or texture atlas, GPU monitoring
is an opt-out toggle (NVML/CUDA cost ~20 MB), and the background daemon is off
by default.

| Solution | RAM |
| ------------- | ------------- |
| rproc (GPU off) | ~25 MB |
| rproc (GPU on, default) | ~45 MB |
| Gnome System Monitor | 185 MB |
| Resources | 200 MB |
| Mission Center | 239 MB |

The optional background daemon adds ~28 MB while enabled.

<div align="center">
  <img src="./img/capture2.png" alt="Processes tab" width="450">
  <img src="./img/capture3.png" alt="Startup apps" width="450">
</div>

## Install

Prebuilt packages for every release are on the
[**Releases page**](https://github.com/Trystan-SA/rproc/releases/latest).
Download the file for your distribution, then:

### Debian / Ubuntu (`.deb`)

```bash
sudo apt install ./rproc_<version>_amd64.deb
```

### Fedora / RHEL / openSUSE (`.rpm`)

```bash
sudo dnf install ./rproc-<version>-1.x86_64.rpm
```

### Flatpak

```bash
flatpak install --user ./rproc-<version>-x86_64.flatpak
flatpak run io.github.trystan_sa.rproc
```

### NixOS / Nix

```bash
nix run github:trystan-sa/rproc
```

```nix
# Install via Flake:
inputs = {
  rproc = {
      url = "github:trystan-sa/rproc";
      inputs.nixpkgs.follows = "nixpkgs";
    };
};

# In nix configuration:
{inputs, pkgs, ...}:{
  environment.systemPackages = with pkgs; [
    inputs.rproc.packages.${pkgs.stdenv.hostPlatform.system}.default
  ];
}
```

### From source

Requires the stable Rust toolchain ([rustup](https://rustup.rs/)) and a few
system libraries. On Debian/Ubuntu:

```bash
sudo apt install pkg-config libfontconfig1-dev libxkbcommon-dev \
    libx11-dev libxcb1-dev libwayland-dev
```

```bash
git clone https://github.com/Trystan-SA/rproc.git
cd rproc
cargo run --release
```

## Requirements

- Linux (X11 or Wayland)
- `systemctl` for the Services and Startup tabs

## Background sampling

`rproc` can keep a 60-sample rolling window of system metrics
(`~/.cache/rproc/history.bin`, ~2 KB, fixed size) so re-opening the window
shows the last minute of activity even after a full close.

It is **off by default** (the collector is a second process that also loads
NVML/CUDA); enable *Background history* in the Settings page to turn it on.
When enabled the collector runs as a detached background process
(`setsid`-detached, so closing rproc leaves it running). You can also start it
on its own:

```bash
rproc --daemon
```

Packages install a systemd **user** unit that you can enable to start the
sampler at login:

```bash
systemctl --user enable --now rprocd
```

> Installing from source instead? Copy the unit first:
> `mkdir -p ~/.config/systemd/user && cp packaging/rprocd.service ~/.config/systemd/user/`

## Building packages

Single-command targets via the included `Makefile`:

```bash
make deb               # build a .deb  -> target/debian/
make rpm               # build an .rpm -> target/generate-rpm/
make flatpak           # build a local .flatpak bundle
make flatpak-install   # build + install the Flatpak for the current user
```

## License

[MIT](LICENSE)
