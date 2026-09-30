# syntax=docker/dockerfile:1
# The Slack bot on a server that has neither Node nor a checkout of e: e comes
# from a release when one is named, otherwise it is built from source; the
# channel comes from this directory. Build from the repository:
#
#   docker build -t e-slack services/slack
#
# Debian 13 supplies a maintained runtime above the release's glibc 2.31 floor.
FROM node:24-trixie-slim

# git is what e's tools need on a checkout; curl and ca-certificates fetch the
# release installer or the Rust toolchain.
RUN apt-get update \
 && apt-get install -y --no-install-recommends ca-certificates curl git \
 && rm -rf /var/lib/apt/lists/*

# Match the checkout's owner at build time when needed. The runtime always has
# an unprivileged default user and a private credential/session directory.
ARG E_UID=1000
ARG E_GID=1000
RUN test "$E_UID" -gt 0 && test "$E_GID" -gt 0 \
 && if ! getent group "$E_GID" >/dev/null; then groupmod -g "$E_GID" node; fi \
 && usermod -u "$E_UID" -g "$E_GID" node \
 && install -d -m 0700 -o "$E_UID" -g "$E_GID" /home/e /work
ENV E_HOME=/home/e
ENV E_SLACK_STATE=/home/e/slack-state.json

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
RUN --mount=type=secret,id=proxy_ca \
    if [ -f /run/secrets/proxy_ca ]; then export NODE_EXTRA_CA_CERTS=/run/secrets/proxy_ca; fi; \
    npm ci --omit=dev --ignore-scripts --no-fund --no-audit
COPY src ./src
COPY .env.example manifest.json README.md ./

# The repository the bot works in. Mount one here.
WORKDIR /work
ENV E_CWD=/work
USER node
ENTRYPOINT ["node", "--experimental-strip-types", "/app/src/index.ts"]
