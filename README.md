<div align="center">
<a href="https://www.webb.tools/">

![Webb Logo](./assets/webb_banner_light.png#gh-light-mode-only)
![Webb Logo](./assets/webb_banner_dark.png#gh-dark-mode-only)
  </a>
  </div>
<p align="left">
    <strong>🚀 Threshold ECDSA Distributed Key Generation Protocol 🔑 </strong>
</p>

[![GitHub Workflow Status](https://img.shields.io/github/actions/workflow/status/webb-tools/dkg-substrate/checks.yml?branch=master&style=flat-square)](https://github.com/webb-tools/dkg-substrate/actions) [![Codecov](https://img.shields.io/codecov/c/gh/webb-tools/dkg-substrate?style=flat-square&token=HNT1CEZ01E)](https://codecov.io/gh/webb-tools/dkg-substrate) [![License: GPL v3](https://img.shields.io/badge/License-GPLv3-blue.svg)](https://www.gnu.org/licenses/gpl-3.0) [![Telegram](https://img.shields.io/badge/Telegram-gray?logo=telegram)](https://t.me/webbprotocol) [![Discord](https://img.shields.io/discord/833784453251596298.svg?style=flat-square&label=Discord&logo=discord)](https://discord.gg/cv8EfJu3Tn)

<!-- TABLE OF CONTENTS -->
<h2 id="table-of-contents"> 📖 Table of Contents</h2>

<details open="open">
  <summary>Table of Contents</summary>
  <ul>
    <li><a href="#start">Getting Started</a></li>
    <ul>
        <li><a href="#prerequisites">Prerequisites</a></li>
        <li><a href="#install">Installation</a></li>
    </ul>
    <li><a href="#usage">Usage</a></li>
    <ul>
        <li><a href="#standalone">Standalone Testnet</a></li>
    </ul>
    <li><a href="#test">Testing</a></li>
    <li><a href="#contribute">Contributing</a></li>
    <li><a href="#license">License</a></li>
  </ul>
</details>

<h1 id="start"> Getting Started  🎉 </h1>

The DKG is a multi-party computation protocol that generates a group public and private key. We aim to use this group keypair to sign arbitrary messages that will govern protocols deployed around the blockchain ecosystem. One primary purpose for the DKG is to govern and facilitate operations of the private signature bridge/anchor system.

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

## Installation 💻

```bash
cargo build --release
```

> NOTE: You _must_ use the release builds! The optimizations here are required
> as in debug mode, it is expected that nodes are not able to run fast enough to produce blocks.

## Installation Using Nix 💻

1. Install [Nix](https://nixos.org/download.html)
2. Enable [Flakes](https://nixos.wiki/wiki/Flakes)
3. If you have [`direnv`](https://github.com/nix-community/nix-direnv#installation) installed, everything should work out of the box.
4. Run `nix develop` in the root of this repo to get a shell with all the dependencies installed.


<h1 id="usage"> Usage </h1>

<h3 id="standalone"> Standalone Local Testnet </h3>

```bash
./scripts/run-local-testnet.sh --clean
```

This should start the local testnet, you can view the logs in `/tmp` directory for all the authorities and use [polkadotJS](https://polkadot.js.org/apps/?rpc=ws%3A%2F%2F127.0.0.1%3A9944#/explorer) to view the running testnet.


<h2 id="test"> Testing 🧪 </h2>

The following instructions outline how to run dkg-substrate's base test suite and E2E test suite.

### Unit Tests

```
cargo test
```

### Stress Tests

When debugging the internal mechanics of the DKG, it is useful to run the stress tests. These tests are designed to run the DKG with a small number of authorities and a small number of participants. This allows you to quickly debug the DKG without having to setup a local chain.

```
# Build the dkg-standalone node
cargo build --release -p dkg-standalone-node --features=integration-tests,testing

# run the orchestrator, making sure to use the proper config
cargo run --package dkg-test-orchestrator --release --features=testing -- --config /path/to/orchestrator_config.toml
```

### Local Chain Tests (Automatic)

To test the DKG against a local testnet, run:

```
cargo make integration-tests
```

### Local Chain Tests (Manual)

If manual control is needed over the nodes (e.g., for debugging and/or shutting down nodes to test fault-tolerance), first build the binary:

```
cargo make build-test
```

Then, start the bootnode (Alice):

```
cargo make alice
```

Once Alice is running, start the other nodes each in separate terminals/processes:

```
cargo make bob
cargo make charlie
cargo make dave
cargo make eve
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

Interested in contributing to the Webb DKG Protocol? Thank you so much for your interest! We are always appreciative for contributions from the open-source community!

If you have a contribution in mind, please check out our [Contribution Guide](./.github/CONTRIBUTING.md) for information on how to do so. We are excited for your first contribution!

<h2 id="license"> License </h2>

Licensed under <a href="LICENSE">GNU General Public License v3.0</a>.

Unless you explicitly state otherwise, any contribution intentionally submitted for inclusion in this crate by you, as defined in the GNU General Public License v3.0 license, shall be licensed as above, without any additional terms or conditions.


## Troubleshooting
The linking phase may fail due to not finding libgmp (i.e., "could not find library -lgmp") when building on a mac M1. To fix this problem, run:

```bash
brew install gmp
# make sure to run the commands below each time when starting a new env, or, append them to .zshrc
export LIBRARY_PATH=$LIBRARY_PATH:/opt/homebrew/lib
export INCLUDE_PATH=$INCLUDE_PATH:/opt/homebrew/include
