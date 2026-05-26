# rproc

A resource & process monitor for Linux, inspired by the Windows 11 Task Manager.

Built in Rust with [`egui`](https://github.com/emilk/egui).

<p align="center">
  <img src="example1.png" alt="Performance view" width="900">
</p>

## Features

- **Processes**: CPU, memory, disk I/O, threads, status. Sort, filter, kill.
- **Performance**: live charts for CPU (global + per-core), memory, disks, network, GPU (NVIDIA / AMD / Intel).
- **Startup**: XDG autostart entries and enabled systemd units.
- **Services**: systemctl system and user units.
- **Settings**: adjustable refresh rate.

<p align="center">
  <img src="example2.png" alt="Processes tab" width="450">
  <img src="example3.png" alt="Services tab" width="450">
</p>

## Requirements

- Linux (X11 or Wayland)
- Rust (stable), install via [rustup](https://rustup.rs/)
- `systemctl` for the Services and Startup tabs
- NVIDIA driver for NVIDIA GPU metrics (optional)

## Install

Prebuilt packages for each release are published on the
[Releases page](https://github.com/Trystan-SA/rproc/releases).

### Debian / Ubuntu (`.deb`)

```bash
sudo dpkg -i rproc_<version>_amd64.deb
sudo apt-get install -f  # pulls in any missing deps
```

### Fedora / RHEL / openSUSE (`.rpm`)

```bash
sudo dnf install ./rproc-<version>-1.x86_64.rpm
# or: sudo rpm -i rproc-<version>-1.x86_64.rpm
```

### Flatpak

```bash
flatpak install --user rproc-<version>-x86_64.flatpak
flatpak run io.github.trystan_sa.rproc
```

### From source

```bash
cargo run --release
```

## Packaging

Single-command targets via the included `Makefile`:

```bash
make flatpak           # build a local .flatpak bundle
make flatpak-install   # build + install for the current user
make deb               # build a .deb (target/debian/)
make rpm               # build an .rpm (target/generate-rpm/)
```

## Release

To cut a new release, run:

```bash
make release
```

You'll be prompted for the bump (patch / minor / major). The script bumps
the version, tags `vX.Y.Z`, and pushes — GitHub Actions then builds the
binary, `.deb`, `.rpm` and `.flatpak` and publishes them on the
[Releases page](https://github.com/Trystan-SA/rproc/releases).

## Background sampling

`rproc` keeps a 60-sample rolling window of system metrics
(`~/.cache/rproc/history.bin`, ~2 KB, fixed size — no growth, no leak)
so re-opening the window shows the last minute of CPU and memory
activity even after a full close.

The collector runs as a detached background process, auto-spawned the
first time you launch the GUI (`setsid`-detached, so closing rproc
leaves it running). You can also start it on its own:

```bash
rproc --daemon
```

To have it start at login, install the binary and enable the systemd
user unit:

```bash
cargo install --path .
mkdir -p ~/.config/systemd/user
cp packaging/rprocd.service ~/.config/systemd/user/
systemctl --user enable --now rprocd
```

## License

MIT
