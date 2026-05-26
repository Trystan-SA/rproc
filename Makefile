VERSION := $(shell sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)
APP_ID  := io.github.trystan_sa.rproc

.PHONY: help flatpak flatpak-install deb rpm clean release

help:
	@echo "rproc packaging targets:"
	@echo "  make flatpak         build a local .flatpak bundle"
	@echo "  make flatpak-install build + install for the current user"
	@echo "  make deb             build a .deb (target/debian/)"
	@echo "  make rpm             build an .rpm (target/generate-rpm/)"
	@echo "  make release         interactive: bump version, tag, push -> CI publishes release"
	@echo "  make clean           remove build artefacts"

# --- Flatpak ----------------------------------------------------------------

flatpak: build/repo
	flatpak build-bundle build/repo \
		rproc-$(VERSION)-x86_64.flatpak \
		$(APP_ID)
	@echo "==> rproc-$(VERSION)-x86_64.flatpak"

flatpak-install: build/repo
	flatpak --user remote-add --if-not-exists --no-gpg-verify \
		rproc-local build/repo
	flatpak --user install --reinstall --assumeyes rproc-local $(APP_ID)

build/repo: packaging/flatpak/cargo-sources.json
	@command -v flatpak-builder >/dev/null 2>&1 || { \
		echo "flatpak-builder not found. Install it: sudo dnf install flatpak-builder (or apt)"; exit 1; }
	@flatpak info org.freedesktop.Sdk//24.08 >/dev/null 2>&1 || \
		flatpak install --user --assumeyes flathub \
		  org.freedesktop.Platform//24.08 \
		  org.freedesktop.Sdk//24.08 \
		  org.freedesktop.Sdk.Extension.rust-stable//24.08
	flatpak-builder --user --force-clean --repo=build/repo \
		build/flatpak packaging/flatpak/$(APP_ID).yml

packaging/flatpak/cargo-sources.json: Cargo.lock
	@mkdir -p build
	@test -f build/flatpak-cargo-generator.py || \
		curl -sSfL -o build/flatpak-cargo-generator.py \
		  https://raw.githubusercontent.com/flatpak/flatpak-builder-tools/master/cargo/flatpak-cargo-generator.py
	python3 build/flatpak-cargo-generator.py $< -o $@

# --- Debian / RPM -----------------------------------------------------------

deb:
	@command -v cargo-deb >/dev/null 2>&1 || cargo install cargo-deb --locked
	cargo deb

rpm:
	@command -v cargo-generate-rpm >/dev/null 2>&1 || cargo install cargo-generate-rpm --locked
	cargo build --release --locked
	strip target/release/rproc
	cargo generate-rpm

# --- Release flow -----------------------------------------------------------
# Interactive. Prompts for the bump kind, bumps version, commits, tags
# vX.Y.Z and pushes — the Release workflow then builds and publishes
# .deb, .rpm and .flatpak to GitHub Releases.

release:
	@bash scripts/release.sh

# --- Housekeeping -----------------------------------------------------------

clean:
	rm -rf build rproc-*.flatpak \
	       target/debian target/generate-rpm \
	       packaging/flatpak/cargo-sources.json
