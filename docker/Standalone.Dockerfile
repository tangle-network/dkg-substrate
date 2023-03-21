# Copyright 2022 Webb Technologies Inc.
#
# Licensed under the Apache License, Version 2.0 (the "License");
# you may not use this file except in compliance with the License.
# You may obtain a copy of the License at
#
# http://www.apache.org/licenses/LICENSE-2.0
#
# Unless required by applicable law or agreed to in writing, software
# distributed under the License is distributed on an "AS IS" BASIS,
# WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
# See the License for the specific language governing permissions and
# limitations under the License.
#
FROM lukemathwalker/cargo-chef:latest-rust-1 AS chef
WORKDIR /dkg

FROM chef AS planner
COPY . .
RUN cargo chef prepare --recipe-path recipe.json

FROM chef AS builder
# Install Required Packages
RUN apt-get update && apt-get install -y git cmake clang curl libssl-dev llvm libudev-dev libgmp3-dev protobuf-compiler && rm -rf /var/lib/apt/lists/*
COPY --from=planner /dkg/recipe.json recipe.json
COPY rust-toolchain.toml .
# Build dependencies - this is the caching Docker layer!
RUN cargo chef cook -Z sparse-registry --release --recipe-path recipe.json 
COPY . .
# Build application
RUN cargo +nightly build -Z sparse-registry --release -p dkg-standalone-node

# This is the 2nd stage: a very small image where we copy the DKG binary."

FROM ubuntu:20.04

COPY --from=builder /dkg/target/release/dkg-standalone-node /usr/local/bin

RUN useradd -m -u 1000 -U -s /bin/sh -d /dkg dkg && \
  mkdir -p /data /dkg/.local/share/dkg && \
  chown -R dkg:dkg /data && \
  ln -s /data /dkg/.local/share/dkg && \
  # Sanity checks
  ldd /usr/local/bin/dkg-standalone-node && \
  /usr/local/bin/dkg-standalone-node --version

USER dkg
EXPOSE 30333 9933 9944 9615
VOLUME ["/data"]
