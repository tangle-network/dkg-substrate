FROM rust:buster as builder
WORKDIR /dkg

RUN rustup default nightly-2021-11-07 && \
	rustup target add wasm32-unknown-unknown --toolchain nightly-2021-11-07

# Install Required Packages
RUN apt-get update && apt-get install -y git clang curl libssl-dev llvm libudev-dev libgmp3-dev && rm -rf /var/lib/apt/lists/*

ARG GIT_COMMIT=
ENV GIT_COMMIT=$GIT_COMMIT
ARG BUILD_ARGS

COPY . .
# Build DKG Parachain Node
RUN cargo build --release -p dkg-node

# =============

FROM ubuntu:20.04

COPY --from=builder /dkg/target/release/dkg-node /usr/local/bin

RUN useradd -m -u 1000 -U -s /bin/sh -d /dkg dkg  && \
  mkdir -p /data /dkg/.local/share/dkg && \
  chown -R dkg:dkg /data && \
  ln -s /data /dkg/.local/share/dkg

RUN ldd /usr/local/bin/dkg-node && \
  /usr/local/bin/dkg-node --version

USER dkg
EXPOSE 30333 9933 9944 9615
VOLUME ["/data"]

ENTRYPOINT [ "/usr/local/bin/dkg-node" ]