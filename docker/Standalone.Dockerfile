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
FROM rust:1 as builder
WORKDIR /dkg

# Install Required Packages
RUN apt-get update && apt-get install -y git clang curl libssl-dev llvm libudev-dev libgmp3-dev && rm -rf /var/lib/apt/lists/*

COPY . .
# Build Standalone Node
RUN cargo build --release -p dkg-standalone-node

# This is the 2nd stage: a very small image where we copy the DKG binary."

FROM ubuntu:20.04

COPY --from=builder /dkg/target/release/dkg-standalone-node /usr/local/bin

RUN apt-get update && apt-get install -y clang libssl-dev llvm libudev-dev libgmp3-dev protobuf-compiler && rm -rf /var/lib/apt/lists/*

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
