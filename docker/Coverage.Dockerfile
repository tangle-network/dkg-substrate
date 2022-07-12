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
FROM rust:1
WORKDIR /webb

# Install Required Packages
RUN apt-get update && \
    apt-get install -y git pkg-config clang curl libssl-dev llvm libudev-dev libgmp3-dev protobuf-compiler && \
    rm -rf /var/lib/apt/lists/*

COPY . .

RUN rustup default nightly

RUN cargo install cargo-tarpaulin

CMD SKIP_WASM_BUILD=1 cargo +nightly tarpaulin --out Xml \
    -p pallet-dkg-metadata \
    -p pallet-dkg-proposal-handler \
    -p pallet-dkg-proposals \
    -p dkg-primitives \
    -p dkg-runtime-primitives \
    --timeout 3600
