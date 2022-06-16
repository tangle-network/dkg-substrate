#!/usr/bin/env bash
set -e

pushd .

# The following line ensure we run from the project root
PROJECT_ROOT=$(git rev-parse --show-toplevel)
cd "$PROJECT_ROOT"

echo "*** Start Webb DKG Node ***"
./target/release/dkg-standalone-node --base-path=./tmp/alice -lerror --alice \
  --rpc-cors all --ws-external \
  --port 30304 \
  --ws-port 9944 &
./target/release/dkg-standalone-node --base-path=./tmp/bob -lerror --bob \
  --rpc-cors all --ws-external \
  --port 30305 \
  --ws-port 9945 &
./target/release/dkg-standalone-node --base-path=./tmp/charlie -linfo \
    --ws-port 9946 \
    --rpc-cors all \
    --ws-external \
    --port 30306 \
    -ldkg=debug \
    -ldkg_metadata=debug \
    -lruntime::offchain=debug \
    -ldkg_proposal_handler=debug \
    --charlie
popd
