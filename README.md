# Neo Zed

[![CI](https://github.com/zed-industries/zed/actions/workflows/run_tests.yml/badge.svg)](https://github.com/zed-industries/zed/actions/workflows/run_tests.yml)

Welcome to Neo Zed, a high-performance, multiplayer code editor forked from Zed.

Neo Zed is maintained as a product fork. Upstream Zed links remain in places where URL migration is not yet part of this fork.

---

### Installation

Neo Zed release distribution is still being separated from upstream. Until dedicated Neo Zed downloads are published, upstream [Zed downloads](https://zed.dev/download) and package-manager instructions remain the installation reference ([macOS](https://zed.dev/docs/installation#macos)/[Linux](https://zed.dev/docs/linux#installing-via-a-package-manager)/[Windows](https://zed.dev/docs/windows#package-managers)).

Other platforms are not yet available:

- Web ([tracking issue](https://github.com/zed-industries/zed/issues/5396))

### Developing Neo Zed

- [Building Neo Zed for macOS](./docs/src/development/macos.md)
- [Building Neo Zed for Linux](./docs/src/development/linux.md)
- [Building Neo Zed for Windows](./docs/src/development/windows.md)

### Contributing

See [CONTRIBUTING.md](./CONTRIBUTING.md) for ways you can contribute to Neo Zed.

### Licensing

License information for third party dependencies must be correctly provided for CI to pass.

We use [`cargo-about`](https://github.com/EmbarkStudios/cargo-about) to automatically comply with open source licenses. If CI is failing, check the following:

- Is it showing a `no license specified` error for a crate you've created? If so, add `publish = false` under `[package]` in your crate's Cargo.toml.
- Is the error `failed to satisfy license requirements` for a dependency? If so, first determine what license the project has and whether this system is sufficient to comply with this license's requirements. If you're unsure, ask a lawyer. Once you've verified that this system is acceptable add the license's SPDX identifier to the `accepted` array in `script/licenses/zed-licenses.toml`.
- Is `cargo-about` unable to find the license for a dependency? If so, add a clarification field at the end of `script/licenses/zed-licenses.toml`, as specified in the [cargo-about book](https://embarkstudios.github.io/cargo-about/cli/generate/config.html#crate-configuration).

## Sponsorship

Neo Zed is maintained as an open-source fork. If sponsorship options are available for this repository, they are the best way to support ongoing maintenance.
There are no perks or entitlements associated with sponsorship.
