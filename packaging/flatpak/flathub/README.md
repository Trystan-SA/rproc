# Flathub submission

This folder holds the **submission-ready** Flatpak manifest for
[flathub/flathub](https://github.com/flathub/flathub). It differs from
`../io.github.trystan_sa.rproc.yml` (the local-build manifest, which uses a
`type: dir` source) in one way only: the source is a tagged git checkout, as
Flathub requires reproducible sources.

Keep the pinned `tag`/`commit` in the manifest in sync with the release you
submit.

## Cut the release first

Flathub builds from the tagged tree, so the corrected `metainfo.xml` (with the
full `<releases>` list) must be part of the tag. Land the packaging changes on
`main`, then cut `0.3.6`:

```sh
cargo set-version 0.3.6
cargo update --workspace
git commit -am "release v0.3.6"
git tag -a v0.3.6 -m "Release v0.3.6"
git push origin HEAD v0.3.6     # CI builds and publishes the release
```

> `make release`'s patch bump would target `v0.3.5`, which already exists as a
> divergent tag and would make `git tag` fail — hence the explicit
> `cargo set-version 0.3.6`, skipping the taken `0.3.5`.

After the tag is pushed, pin the manifest to it:

```sh
git rev-list -n1 v0.3.6    # paste this into the manifest's `commit:` field
```

## Submitting

1. Generate the vendored crate sources for the pinned tag and keep the output
   next to the manifest (it is `.gitignore`d in this repo but **must** be
   committed to the Flathub PR):

   ```sh
   python3 build/flatpak-cargo-generator.py Cargo.lock \
     -o packaging/flatpak/flathub/cargo-sources.json
   ```

2. Build and lint locally with the official builder:

   ```sh
   flatpak run org.flatpak.Builder --install-deps-from=flathub \
     --force-clean build-dir packaging/flatpak/flathub/io.github.trystan_sa.rproc.yml
   flatpak run --command=flatpak-builder-lint org.flatpak.Builder \
     manifest packaging/flatpak/flathub/io.github.trystan_sa.rproc.yml
   flatpak run --command=flatpak-builder-lint org.flatpak.Builder \
     appstream packaging/io.github.trystan_sa.rproc.metainfo.xml
   ```

3. Fork https://github.com/flathub/flathub (uncheck "Copy the master branch
   only"), then:

   ```sh
   git clone --branch=new-pr git@github.com:Trystan-SA/flathub.git && cd flathub
   git checkout -b rproc-submission new-pr
   ```

4. Copy `io.github.trystan_sa.rproc.yml` and the generated
   `cargo-sources.json` to the root of the fork, commit, and push.

5. Open a PR against the **`new-pr`** base branch, titled
   `Add io.github.trystan_sa.rproc`. Comment `bot, build` to trigger a test
   build once review comments are resolved.
