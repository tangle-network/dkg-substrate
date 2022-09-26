#!/usr/bin/env bash
set -e

# build the standalone binaries
cargo b -rp dkg-standalone-node

rm -rf ./tmp

# start the standalone network
echo "*** Start DKG Standalone node ***"
./target/release/dkg-standalone-node --base-path=./tmp/alice -lerror --alice \
  --rpc-cors all --ws-external \
  --port 30304 \
  --ws-port 9944 &
./target/release/dkg-standalone-node --base-path=./tmp/bob -lerror --bob \
  --rpc-cors all --ws-external \
  --port 30305 \
  --ws-port 9945 &
./target/release/dkg-standalone-node --base-path=./tmp/charlie -lerror \
    --ws-port 9946 \
    --rpc-cors all \
    --ws-external \
    --port 30306 \
    --charlie &

# setup and run the verify script 
echo "*** Verify DKG keys ***"
npm --prefix ./scripts/verify-dkg install ./scripts/verify-dkg
node ./scripts/verify-dkg/index.js