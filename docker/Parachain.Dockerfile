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
FROM rust:buster as builder
WORKDIR /app

RUN rustup default nightly-2022-12-26 && \
	rustup target add wasm32-unknown-unknown --toolchain nightly-2022-12-26

# Install Required Packages
RUN apt-get update && apt-get install -y git clang curl libssl-dev llvm libudev-dev libgmp3-dev protobuf-compiler && rm -rf /var/lib/apt/lists/*

ARG GIT_COMMIT=
ENV GIT_COMMIT=$GIT_COMMIT
ARG BUILD_ARGS

COPY . .
# Build DKG Parachain Node
RUN cargo +nightly -Z sparse-registry build --release -p dkg-collator

# =============

FROM phusion/baseimage:bionic-1.0.0

RUN useradd -m -u 1000 -U -s /bin/sh -d /dkg dkg

COPY --from=builder /app/target/release/dkg-collator /usr/local/bin

# checks
RUN ldd /usr/local/bin/dkg-collator && \
  /usr/local/bin/dkg-collator --version

# Shrinking
RUN rm -rf /usr/lib/python* && \
	rm -rf /usr/bin /usr/sbin /usr/share/man

USER dkg
EXPOSE 30333 9933 9944 9615

RUN mkdir /dkg/data
RUN chown -R dkg:dkg /dkg/data

VOLUME ["/dkg/data"]

ENTRYPOINT [ "/usr/local/bin/dkg-collator" ]