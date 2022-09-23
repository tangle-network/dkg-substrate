#!/usr/bin/env bash
set -e

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