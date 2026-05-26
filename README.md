<div align="center">

# rproc

**A resource & process monitor for Linux, inspired by the Windows 11 Task Manager.**

Built in Rust with [`egui`](https://github.com/emilk/egui).

[![CI](https://github.com/Trystan-SA/rproc/actions/workflows/ci.yml/badge.svg)](https://github.com/Trystan-SA/rproc/actions/workflows/ci.yml)
[![Release](https://github.com/Trystan-SA/rproc/actions/workflows/release.yml/badge.svg)](https://github.com/Trystan-SA/rproc/actions/workflows/release.yml)
[![Latest release](https://img.shields.io/github/v/release/Trystan-SA/rproc?sort=semver)](https://github.com/Trystan-SA/rproc/releases/latest)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](#license)
![Platform: Linux](https://img.shields.io/badge/platform-Linux-informational)

<img src="example1.png" alt="Performance view" width="900">

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

### From source

Requires the stable Rust toolchain ([rustup](https://rustup.rs/)).

```bash
git clone https://github.com/Trystan-SA/rproc.git
cd rproc
cargo run --release
```

## Features

- **Processes** — CPU, memory, disk I/O, threads and status. Sort, filter and kill.
- **Performance** — live charts for CPU (global + per-core), memory, disks, network and GPU (NVIDIA / AMD / Intel).
- **Startup** — XDG autostart entries and enabled systemd units.
- **Services** — systemctl system and user units.
- **Settings** — adjustable refresh rate.

<div align="center">
  <img src="example2.png" alt="Processes tab" width="450">
  <img src="example3.png" alt="Services tab" width="450">
</div>

## Requirements

- Linux (X11 or Wayland)
- `systemctl` — for the Services and Startup tabs
- NVIDIA driver — for NVIDIA GPU metrics (optional)

## Background sampling

`rproc` keeps a 60-sample rolling window of system metrics
(`~/.cache/rproc/history.bin`, ~2 KB, fixed size — no growth, no leak) so
re-opening the window shows the last minute of CPU and memory activity
even after a full close.

The collector runs as a detached background process, auto-spawned the
first time you launch the GUI (`setsid`-detached, so closing rproc leaves
it running). You can also start it on its own:

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

## Releasing

Maintainers cut a release with a single command:

```bash
make release
```

It prompts for the version bump (patch / minor / major), tags `vX.Y.Z`
and pushes. GitHub Actions then builds the binary, `.deb`, `.rpm` and
`.flatpak` and publishes them to the
[Releases page](https://github.com/Trystan-SA/rproc/releases).

## License

[MIT](LICENSE)
