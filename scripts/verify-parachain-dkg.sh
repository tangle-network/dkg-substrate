#!/usr/bin/env bash
set -e

# build the relaychain binaries
git clone --depth 1 --branch release-v0.9.28 https://github.com/paritytech/polkadot.git
cd polkadot
cargo build --release
cd ..

# build the parachain binaries
cargo b -rp dkg-collator

rm -rf ./tmp

# start the parachain network
echo "*** Start parachain network ***"
#npm i -g polkadot-launch
polkadot-launch /scripts/polkadot-launch/config.js

# setup and run the verify script 
echo "*** Verify DKG keys ***"
cd ./scripts/verify-dkg
npm install
node index.js