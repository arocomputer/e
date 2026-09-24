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

ulo is in development. Public releases and package-manager distributions are
paused. Build and run this checkout with the Rust toolchain:

```sh
git clone https://github.com/arocomputer/ulo.git
cd ulo
./x dev /path/to/project
```

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
