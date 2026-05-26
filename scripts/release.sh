#!/usr/bin/env bash
# Cut a new release of rproc.
# Prompts for the bump kind, then: bumps Cargo.toml, refreshes Cargo.lock,
# commits, tags vX.Y.Z, and pushes. CI then builds and publishes
# .deb / .rpm / .flatpak to GitHub Releases.

set -euo pipefail

cd "$(git rev-parse --show-toplevel)"

if [[ -n "$(git status --porcelain)" ]]; then
    echo "error: working tree is dirty. Commit or stash first." >&2
    exit 1
fi

if ! command -v cargo-set-version >/dev/null 2>&1; then
    echo "==> installing cargo-edit (provides cargo set-version)"
    cargo install cargo-edit --locked
fi

CURRENT=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)

# Strip any pre-release / build suffix so we can compute numeric bumps.
CORE=${CURRENT%%-*}
CORE=${CORE%%+*}
IFS=. read -r MAJ MIN PAT <<<"$CORE"
MAJ=${MAJ:-0}; MIN=${MIN:-0}; PAT=${PAT:-0}

PATCH_NEXT="${MAJ}.${MIN}.$((PAT + 1))"
MINOR_NEXT="${MAJ}.$((MIN + 1)).0"
MAJOR_NEXT="$((MAJ + 1)).0.0"

echo "current version: ${CURRENT}"
echo
echo "Select bump:"
echo "  1) patch  ${CURRENT} -> ${PATCH_NEXT}   (bug fix)"
echo "  2) minor  ${CURRENT} -> ${MINOR_NEXT}   (new feature)"
echo "  3) major  ${CURRENT} -> ${MAJOR_NEXT}   (breaking change)"
echo

BUMP=""
while [[ -z "$BUMP" ]]; do
    read -r -p "choice [1/2/3 or patch/minor/major]: " choice
    case "$choice" in
        1|patch) BUMP=patch ;;
        2|minor) BUMP=minor ;;
        3|major) BUMP=major ;;
        *) echo "invalid choice." ;;
    esac
done

cargo set-version --bump "$BUMP"
cargo update --workspace --offline 2>/dev/null || cargo update --workspace

NEW=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)
echo
echo "==> ${CURRENT} -> ${NEW}"
read -r -p "proceed with commit, tag v${NEW} and push? [y/N]: " confirm
if [[ "$confirm" != "y" && "$confirm" != "Y" ]]; then
    echo "aborted. Rolling back Cargo.toml/Cargo.lock."
    git checkout -- Cargo.toml Cargo.lock
    exit 1
fi

git add Cargo.toml Cargo.lock
git commit -m "release v${NEW}"
git tag -a "v${NEW}" -m "Release v${NEW}"
git push origin HEAD
git push origin "v${NEW}"

echo
echo "==> pushed v${NEW}. CI will build .deb / .rpm / .flatpak and publish the release."
