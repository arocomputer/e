<p align="center">
  <a href="https://ulo.sh">
    <picture>
      <source srcset="assets/logo-dark.svg" media="(prefers-color-scheme: dark)">
      <source srcset="assets/logo.svg" media="(prefers-color-scheme: light)">
      <img src="assets/logo.svg" alt="ulo" height="40">
    </picture>
  </a>
</p>
<p align="center">The coding agent you can put anywhere.</p>
<p align="center">
  <a href="https://github.com/arocomputer/ulo/releases"><img alt="Release" src="https://img.shields.io/github/v/release/arocomputer/ulo?style=flat-square&label=release&labelColor=grey&color=blue" /></a>
  <a href="https://github.com/arocomputer/ulo/actions/workflows/checks.yml"><img alt="Tests" src="https://img.shields.io/github/actions/workflow/status/arocomputer/ulo/checks.yml?style=flat-square&branch=main&label=Tests" /></a>
</p>

[![ulo using GPT-5.6 Sol with low reasoning effort to fix code and run tests](assets/readme.png)](https://ulo.sh)

---

### Installation

```sh
# macOS and Linux
curl -fsSL https://ulo.sh/install.sh | sh
```

Or download a binary from the [latest release](https://github.com/arocomputer/ulo/releases/latest):

| Platform | Download |
| --- | --- |
| macOS · Apple Silicon | [ARM64](https://github.com/arocomputer/ulo/releases/latest/download/ulo-aarch64-apple-darwin.tar.gz) |
| macOS · Intel | [x86-64](https://github.com/arocomputer/ulo/releases/latest/download/ulo-x86_64-apple-darwin.tar.gz) |
| Linux · ARM64 | [ARM64](https://github.com/arocomputer/ulo/releases/latest/download/ulo-aarch64-unknown-linux-gnu.tar.gz) |
| Linux · x86-64 | [x86-64](https://github.com/arocomputer/ulo/releases/latest/download/ulo-x86_64-unknown-linux-gnu.tar.gz) |

[Checksums](https://github.com/arocomputer/ulo/releases/latest/download/checksums.txt) ·
[Installation guide](docs/guides/start/install.md)

### Usage

```sh
cd your-project
ulo
```

Run `/login` to connect a provider, then `/models` to choose a model.

### Documentation

Read the [docs](https://ulo.sh/docs), or run `ulo docs` in your terminal.

### Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md) to get started.

---

<p align="center">
  <a href="https://aro.computer">ARO</a> · <a href="LICENSE">MIT</a>
</p>
