#!/bin/sh

# The following line ensure we run from the project root
PROJECT_ROOT=$(git rev-parse --show-toplevel)
cd "$PROJECT_ROOT"

echo "****************** GENERATE RAW CHAINSPEC ******************"
./target/release/dkg-collator build-spec --disable-default-bootnode --chain dev --raw > ./chainspecs/dkg-parachain-chainspec.json
./target/release/dkg-collator export-genesis-state --chain dev > ./chainspecs/dkg-parachain-genesis-state
./target/release/dkg-collator export-genesis-wasm --chain dev > ./chainspecs/dkg-parachain-genesis-wasm