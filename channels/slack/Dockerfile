# syntax=docker/dockerfile:1
# The Slack bot on a server that has neither Node nor a checkout of e: e comes
# from the release, the channel from this directory. Build from the repository:
#
#   docker build -t e-slack channels/slack
#
# Debian 13 rather than the default bookworm base: the released Linux binaries
# link against glibc 2.39, which bookworm's 2.36 is too old for.
FROM node:24-trixie-slim

# git is what e's tools need on a checkout; curl and ca-certificates are for the
# installer.
RUN apt-get update \
 && apt-get install -y --no-install-recommends ca-certificates curl git \
 && rm -rf /var/lib/apt/lists/*

# e's own home: the trust decision, credentials, sessions, packages. Writable by
# any uid so the bot can run as the checkout's owner (`--user "$(id -u):$(id -g)"`)
# instead of leaving root-owned files in the repository it edits. Mount a volume
# here to keep it across restarts.
ENV E_HOME=/home/e
RUN mkdir -p /home/e && chmod 0777 /home/e

# The e release the bot drives: `production` or `beta`, and empty for the latest
# of that channel. The channel matters as much as the version — a beta version
# is only valid against the beta channel. `production` is the installer default,
# so it passes no channel; that also works with the pre-rename installer whose
# default is `stable`.
ARG E_CHANNEL=production
ARG E_VERSION=""
# The container check (`./x container`) builds without a published release, so it
# installs this stub `e` instead of downloading one. Published images leave it
# empty and install the real release.
ARG E_RELEASE_STUB=""
RUN set -eux; \
    if [ -n "$E_RELEASE_STUB" ]; then \
      printf '#!/bin/sh\necho "e 0.0.0-stub"\n' > /usr/local/bin/e; \
      chmod 0755 /usr/local/bin/e; \
    else \
      curl -fsSL https://e.intuitum.sh/install.sh -o /tmp/install.sh; \
      channel=""; \
      if [ -n "$E_CHANNEL" ] && [ "$E_CHANNEL" != production ]; then channel="--channel $E_CHANNEL"; fi; \
      if [ -n "$E_VERSION" ]; then \
        E_INSTALL_DIR=/usr/local/bin sh /tmp/install.sh $channel --version "$E_VERSION"; \
      else \
        E_INSTALL_DIR=/usr/local/bin sh /tmp/install.sh $channel; \
      fi; \
      rm -f /tmp/install.sh; \
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
