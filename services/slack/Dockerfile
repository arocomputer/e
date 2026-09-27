# syntax=docker/dockerfile:1
# The Slack bot on a server that has neither Node nor a checkout of e: e comes
# from a release when one is named, otherwise it is built from source; the
# channel comes from this directory. Build from the repository:
#
#   docker build -t e-slack services/slack
#
# Debian 13 rather than the default bookworm base: the released Linux binaries
# link against glibc 2.39, which bookworm's 2.36 is too old for.
FROM node:24-trixie-slim

# git is what e's tools need on a checkout; curl and ca-certificates fetch the
# release installer or the Rust toolchain.
RUN apt-get update \
 && apt-get install -y --no-install-recommends ca-certificates curl git \
 && rm -rf /var/lib/apt/lists/*

# e's own home: the trust decision, credentials, sessions, packages. Writable by
# any uid so the bot can run as the checkout's owner (`--user "$(id -u):$(id -g)"`)
# instead of leaving root-owned files in the repository it edits. Mount a volume
# here to keep it across restarts.
ENV E_HOME=/home/e
RUN mkdir -p /home/e && chmod 0777 /home/e

# The e release the bot drives: a pinned `E_VERSION`, passed by the release
# workflow. Empty builds e from source (`main`), since no release is published
# yet; the Rust toolchain and compiler are removed again in the same layer.
ARG E_VERSION=""
# The container check (`./x container`) builds without a published release, so it
# installs this stub `e` instead of downloading one. Published images leave it
# empty and install the real release.
ARG E_RELEASE_STUB=""
RUN set -eux; \
    if [ -n "$E_RELEASE_STUB" ]; then \
      printf '#!/bin/sh\necho "e 0.0.0-stub"\n' > /usr/local/bin/e; \
      chmod 0755 /usr/local/bin/e; \
    elif [ -n "$E_VERSION" ]; then \
      curl -fsSL https://e.aro.computer/install.sh -o /tmp/install.sh; \
      E_INSTALL_DIR=/usr/local/bin sh /tmp/install.sh --version "$E_VERSION"; \
      rm -f /tmp/install.sh; \
    else \
      apt-get update; \
      apt-get install -y --no-install-recommends gcc libc6-dev; \
      export RUSTUP_HOME=/tmp/rustup CARGO_HOME=/tmp/cargo; \
      curl -fsSL https://sh.rustup.rs | sh -s -- -y --no-modify-path --profile minimal --default-toolchain 1.98.0; \
      /tmp/cargo/bin/cargo install --locked --git https://github.com/arocomputer/e --root /usr/local e; \
      rm -rf /tmp/rustup /tmp/cargo /usr/local/.crates.toml /usr/local/.crates2.json; \
      apt-get purge -y --auto-remove gcc libc6-dev; \
      rm -rf /var/lib/apt/lists/*; \
    fi; \
    e --version

WORKDIR /app
COPY package.json package-lock.json ./
RUN npm ci --omit=dev --ignore-scripts --no-fund --no-audit
COPY src ./src
COPY .env.example manifest.json README.md ./

# The repository the bot works in. Mount one here.
WORKDIR /work
ENV E_CWD=/work
ENTRYPOINT ["node", "--experimental-strip-types", "/app/src/index.ts"]
