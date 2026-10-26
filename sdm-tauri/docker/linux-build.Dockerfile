# Ubuntu 22.04 keeps the glibc requirement low (see the Tauri AppImage/deb docs)
# and provides libwebkit2gtk-4.1-dev from its standard repositories.
FROM ubuntu:22.04
ENV DEBIAN_FRONTEND=noninteractive
RUN apt-get update && apt-get install -y --no-install-recommends \
    build-essential curl ca-certificates pkg-config git file xz-utils \
    libwebkit2gtk-4.1-dev libssl-dev libgtk-3-dev libayatana-appindicator3-dev \
    librsvg2-dev libxdo-dev patchelf \
 && rm -rf /var/lib/apt/lists/*
RUN curl https://sh.rustup.rs -sSf | sh -s -- -y --profile minimal --default-toolchain stable
ENV PATH="/root/.cargo/bin:${PATH}"
# Node runs the Tauri CLI and the Vite build; pnpm is pinned to the version in
# sdm-tauri/package.json (`packageManager`).
ARG NODE_VERSION=24.22.1
ARG PNPM_VERSION=12.15.0
RUN arch="$(dpkg --print-architecture)"; \
    case "$arch" in amd64) node_arch=x64 ;; arm64) node_arch=arm64 ;; \
      *) echo "unsupported architecture: $arch" >&2; exit 1 ;; esac \
 && curl -fsSL "https://nodejs.org/dist/v${NODE_VERSION}/node-v${NODE_VERSION}-linux-${node_arch}.tar.xz" \
    | tar -xJ -C /usr/local --strip-components=1 \
 && npm install -g "pnpm@${PNPM_VERSION}" \
 && node --version && pnpm --version
WORKDIR /work
