# syntax=docker/dockerfile:1
# The Slack bot on a server that has neither Node nor a checkout of e: e comes
# from the release, the channel from this directory. Build from the repository:
#
#   docker build -t e-slack channels/slack
FROM node:24-slim

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

# The e release the bot drives. Empty means the latest stable release.
ARG E_VERSION=""
RUN set -eux; \
    curl -fsSL https://e.intuitum.sh/install.sh -o /tmp/install.sh; \
    if [ -n "$E_VERSION" ]; then \
      E_INSTALL_DIR=/usr/local/bin sh /tmp/install.sh --version "$E_VERSION"; \
    else \
      E_INSTALL_DIR=/usr/local/bin sh /tmp/install.sh; \
    fi; \
    rm -f /tmp/install.sh; \
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
