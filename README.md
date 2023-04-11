<div align="center">
<a href="https://www.webb.tools/">
    
![Webb Logo](./assets/webb_banner_light.png#gh-light-mode-only)
![Webb Logo](./assets/webb_banner_dark.png#gh-dark-mode-only)
  </a>
  </div>
<p align="left">
    <strong>🚀 Threshold ECDSA Distributed Key Generation Protocol 🔑 </strong>
</p>

[![GitHub Workflow Status](https://img.shields.io/github/actions/workflow/status/webb-tools/dkg-substrate/checks.yml?branch=master&style=flat-square)](https://github.com/webb-tools/dkg-substrate/actions) [![Codecov](https://img.shields.io/codecov/c/gh/webb-tools/dkg-substrate?style=flat-square&token=HNT1CEZ01E)](https://codecov.io/gh/webb-tools/dkg-substrate) [![License: GPL v3](https://img.shields.io/badge/License-GPLv3-blue.svg)](https://www.gnu.org/licenses/gpl-3.0) [![Twitter](https://img.shields.io/twitter/follow/webbprotocol.svg?style=flat-square&label=Twitter&color=1DA1F2)](https://twitter.com/webbprotocol) [![Telegram](https://img.shields.io/badge/Telegram-gray?logo=telegram)](https://t.me/webbprotocol) [![Discord](https://img.shields.io/discord/833784453251596298.svg?style=flat-square&label=Discord&logo=discord)](https://discord.gg/cv8EfJu3Tn)

<!-- TABLE OF CONTENTS -->
<h2 id="table-of-contents"> 📖 Table of Contents</h2>

<details open="open">
  <summary>Table of Contents</summary>
  <ul>
    <li><a href="#start"> Getting Started</a></li>
    <ul>
        <li><a href="#prerequisites">Prerequisites</a></li>
        <li><a href="#install">Installation</a></li>
        <ul>
          <li><a href="#trouble">Troubleshooting Apple Silicon</a>
          </li>
        </ul>
    </ul>
    <li><a href="#usage">Usage</a></li>
    <ul>
        <li><a href="#standalone">Standalone Testnet</a></li>
        <li><a href="#launch">Run local testnet with polkadot-launch</a></li>
        <li><a href="#para">Run local testnet with parachain-launch</a></li>
    </ul>
    <li><a href="#test">Testing</a></li>
    <li><a href="#contribute">Contributing</a></li>
    <li><a href="#license">License</a></li>
  </ul>  
</details>

<h1 id="start"> Getting Started  🎉 </h1>

The DKG is a multi-party computation protocol that generates a group public and private key. We aim to use this group keypair to sign arbitrary messages that will govern protocols deployed around the blockchain ecosystem. One primary purpose for the DKG is to govern and facilitate operations of the private signature bridge/anchor protocol.

The DKG is meant to be coupled with the relayer network of the system. Currently, there is a fixed set of proposers that can propose messages to be signed by the DKG. This set includes only the active validators or collators of the underlying chain. We hope to increase the set of proposers to any relayer that is participating around the system as well.

For additional information, please refer to the [Webb DKG Rust Docs](https://webb-tools.github.io/dkg-substrate/) 📝. Have feedback on how to improve the dkg? Or have a specific question to ask? Checkout the [DKG Feedback Discussion](https://github.com/webb-tools/feedback/discussions/categories/dkg-feedback) 💬.

## Prerequisites

This guide uses <https://rustup.rs> installer and the `rustup` tool to manage the Rust toolchain.

First install and configure `rustup`:

```bash
# Install
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
# Configure
source ~/.cargo/env
```

Configure the Rust toolchain to default to the latest stable version, add nightly and the nightly wasm target:

```bash
rustup default nightly
rustup update
rustup update nightly
rustup target add wasm32-unknown-unknown
```

Great! Now your Rust environment is ready! 🚀🚀

Lastly, install 

  - [DVC](https://dvc.org/) is used for fetching large ZK files and managing them alongside git
  - [substrate.io](https://docs.substrate.io/main-docs/install/) may require additional dependencies

🚀🚀 Your environment is complete! 🚀🚀

## Installation 💻

Once the development environment is set up, build the DKG. This command will build the [Wasm Runtime](https://docs.substrate.io/v3/advanced/executor/#wasm-execution) and [native](https://docs.substrate.io/v3/advanced/executor/#native-execution) code:

```bash
cargo build --release
```

> NOTE: You _must_ use the release builds! The optimizations here are required
> as in debug mode, it is expected that nodes are not able to run fast enough to produce blocks.

### Troubleshooting for Apple Silicon users

Install Homebrew if you have not already. You can check if you have it installed with the following command:

```bash
brew help
```

If you do not have it installed open the Terminal application and execute the following commands:

```bash
# Install Homebrew if necessary https://brew.sh/
/bin/bash -c "$(curl -fsSL https://raw.githubusercontent.com/Homebrew/install/master/install.sh)"

# Make sure Homebrew is up-to-date, install openssl and llvm
brew update
brew install openssl
brew install llvm
```

Before compiling, you should point CC and AR to use llvm in your environment (run `ls /opt/homebrew/Cellar/llvm/` to get a list of LLVM_VERSIONs):

```bash
export CC=/opt/homebrew/Cellar/llvm/<LLVM_VERSION>/bin/clang
export AR=/opt/homebrew/Cellar/llvm/<LLVM_VERSION>/bin/llvm-ar
export PATH="/opt/homebrew/Cellar/llvm/<LLVM_VERSION>/bin/:$PATH"

```

The linking phase may fail due to not finding libgmp (i.e., "could not find library -lgmp") when building on a mac M1. To fix this problem, run:

```bash
xcode-select --reset && xcode-select --install
brew install gmp
# make sure to run the commands below each time when starting a new env, or, append them to .zshrc
export LIBRARY_PATH=$LIBRARY_PATH:/opt/homebrew/lib
export INCLUDE_PATH=$INCLUDE_PATH:/opt/homebrew/include
```

❗ **Note:** Native ARM Homebrew installations are only going to be supported at `/opt/homebrew`. After Homebrew installs, make sure to add `/opt/homebrew/bin` to your PATH.

```bash
echo 'export PATH=/opt/homebrew/bin:$PATH' >> ~/.bash_profile
```

An example `bash_profile` for reference may look like the following:

```bash
export PATH=/opt/homebrew/bin:$PATH
export PATH=/opt/homebrew/opt/llvm/bin:$PATH
export CC=/opt/homebrew/opt/llvm/bin/clang
export AR=/opt/homebrew/opt/llvm/bin/llvm-ar
export LDFLAGS=-L/opt/homebrew/opt/llvm/lib
export CPPFLAGS=-I/opt/homebrew/opt/llvm/include
export RUSTFLAGS='-L /opt/homebrew/lib'
```

In order to build **dkg-substrate** in `--release` mode using `aarch64-apple-darwin` Rust toolchain you need to set the following environment variables:

```bash
echo 'export RUSTFLAGS="-L /opt/homebrew/lib"' >> ~/.bash_profile
```

Ensure `gmp` dependency is installed correctly.

```bash
brew install gmp
```

If you are still receiving an issue with `gmp`, you may need to adjust your path to the `gmp` lib. Below is a suggestive fix, but paths are machine / environment specific. 

Run:

```bash
cargo clean
```

Then:

```bash
export LIBRARY_PATH=$LIBRARY_PATH:$(brew --prefix)/lib:$(brew --prefix)/opt/gmp/lib
```

This should be added to your bash_profile as well. 

Ensure `protobuf` dependency is installed correctly.

```bash
brew install protobuf
```

<h1 id="usage"> Usage </h1>

<h2 style="border-bottom:none"> Quick Start ⚡ </h2>

<h3 id="standalone"> Standalone Local Testnet </h3>

Currently the easiest way to run the DKG is to use a 3-node local testnet using `dkg-standalone-node`. We will call those nodes `Alice`, `Bob` and
`Charlie`. Each node will use the built-in development account with the same name, i.e. node `Alice` will use the `Alice` development
account and so on. Each of the three accounts has been configured as an initial authority at genesis. So, we are using three validators
for our testnet.

`Alice` is our bootnode and is started like so:

```
$ RUST_LOG=dkg=trace ./target/release/dkg-standalone-node --tmp --alice
```

`Bob` is started like so:

```
RUST_LOG=dkg=trace ./target/release/dkg-standalone-node --tmp --bob
```

`Charlie` is started like so:

```
RUST_LOG=dkg=trace ./target/release/dkg-standalone-node --tmp --charlie
```

Note that the examples above use an ephemeral DB due to the `--tmp` CLI option. If you want a persistent DB, use `--/tmp/[node-name]`
instead. Replace `node-name` with the actual node name (e.g. `alice`) in order to assure separate dirctories for the DB.

<h3 id="para"> Run local testnet with <a href="https://github.com/open-web3-stack/parachain-launch">parachain-launch </a>🐳 </h3>

This section describes how to build and run a RelayChain and Parachain local testnet to develop using Docker.

```
cd launch

# install dependencies
yarn

# generate docker-compose.yml and genesis
# e.g.: docker pull webb-tools/dkg-node:3:0:0
yarn run start generate --config=config.yml

# start relaychain and parachain
cd output

# NOTE: If regenerate the output directory, need to rebuild the images.
docker-compose up -d --build
```

**Note:** Due to usage of offchain workers you will need to add the sr25519 account keys to the node's local keystore by using the `author_insertKey` RPC on the Polkadot UI. If you do not add a sr25519 account key to each of the parachain nodes keystore the node will fail.

<h2 id="test"> Testing 🧪 </h2>

The following instructions outlines how to run dkg-substrate's base test suite and E2E test suite.

### To run base tests

```
cargo test
```

### To run the test orchestrator E2E tests (recommended)

```
# Build the dkg-standalone node
cargo build --release -p dkg-standalone-node --features=integration-tests,testing

# run the orchestrator, making sure to use the proper config
cargo run --package dkg-test-orchestrator --release --features=testing -- --config /path/to/orchestrator_config.toml
```

### To run E2E tests

1. Run `cargo build --release -p dkg-standalone-node --features integration-tests`
2. Run `cd dkg-test-suite && dvc pull`
3. Run `cd dkg-test-suite`
4. Run `yarn install`
5. To run all tests: `yarn test`

**Note:** You may also run individual E2E tests. Please review test script commands in `dkg-test-suite/package.json` for verbose list of test cases. See below examples.

### E2E tests w/tracing
`tracing` is a feature that can be useful for analyzing causality inside the DKG. To enable tracing, the `dkg-standalone-node` must be compiled with the feature `tracing` enabled.
```
1. Compile dkg-standalone-node with tracing: `cargo build --package dkg-standalone-node --release --features=tracing,integration-tests`
2. Run the nodes, setting the env-vars to trace: `RUST_LOG=dkg=trace ./target/release/dkg-standalone-node --tmp --alice`
```

### Anchor Proposal tests:

**From terminal 1:**

1. Run `./scripts/run-standalone.sh`
2. Wait until Keygen completes

**From terminal 2:**

3. Run `yarn anchor-proposals` for anchor proposal tests

### DKG Refresh tests:

**From terminal 1:**

1. Run `./scripts/run-standalone.sh`

**From terminal 2:**

2. Run `yarn dkg-refresh` for DKG refresh tests immediately the node starts

### Code Coverage

You need to have docker installed to generate code coverage.

> Build docker image:

```sh
docker build -t cov -f docker/Coverage.Dockerfile .
```

> Run docker image and generate code coverage reports:

```sh
docker run --security-opt seccomp=unconfined cov
```

### Setting up debugging logs

If you would like to run the dkg with verbose logs you may add the following arguments during initial setup. You may change the target to include `debug | error | info| trace | warn`. You may also want to review [Substrate runtime debugging](https://docs.substrate.io/v3/runtime/debugging/).

```
-ldkg=debug \
-ldkg_metadata=debug \
-lruntime::offchain=debug \
-ldkg_proposal_handler=debug \
-ldkg_proposals=debug
```

<h2 id="contribute"> Contributing </h2>

Interested in contributing to the Webb Relayer Network? Thank you so much for your interest! We are always appreciative for contributions from the open-source community!

If you have a contribution in mind, please check out our [Contribution Guide](./.github/CONTRIBUTING.md) for information on how to do so. We are excited for your first contribution!

<h2 id="license"> License </h2>

Licensed under <a href="LICENSE">GNU General Public License v3.0</a>.

Unless you explicitly state otherwise, any contribution intentionally submitted for inclusion in this crate by you, as defined in the GNU General Public License v3.0 license, shall be licensed as above, without any additional terms or conditions.
