<p align="center">
  <a href="https://e.intuitum.sh">
    <picture>
      <source srcset="assets/logo-dark.svg" media="(prefers-color-scheme: dark)">
      <source srcset="assets/logo.svg" media="(prefers-color-scheme: light)">
      <img src="assets/logo.svg" alt="e" height="40">
    </picture>
  </a>
</p>
<p align="center">The coding agent you can put anywhere.</p>
<p align="center">
  <a href="https://github.com/intuitums/e/releases"><img alt="Release" src="https://img.shields.io/github/v/release/intuitums/e?style=flat-square&label=release&labelColor=grey&color=blue" /></a>
  <a href="https://github.com/intuitums/e/actions/workflows/checks.yml"><img alt="CI" src="https://img.shields.io/github/actions/workflow/status/intuitums/e/checks.yml?style=flat-square&branch=main&label=CI" /></a>
</p>

[![e reading a file, making an edit, and running tests in the terminal](assets/readme.png)](https://e.intuitum.sh)

---

### Installation

```sh
# macOS and Linux
curl -fsSL https://e.intuitum.sh/install.sh | sh

# npm / Bun (beta)
npm install -g @intuitums/e@beta
bun add -g @intuitums/e@beta
```

See [installation](docs/guides/start/install.md) for updates, preview channels,
and platform requirements.

### Usage

```sh
cd your-project
e
```

Run `/login` to connect a provider, then `/models` to choose a model.
For a beta install, use `e-beta` instead of `e`.

### Documentation

Read the [docs](https://e.intuitum.sh/docs), or run `e docs` in your terminal.

### Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md) to get started.

---

[Intuitum](https://intuitum.sh) · [MIT](LICENSE)
