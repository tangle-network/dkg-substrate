#!/usr/bin/env bash

set -e

echo "*** Start Webb DKG Node ***"
./target/release/dkg-standalone-node --tmp -lerror --alice --rpc-cors all --ws-external --ws-port 9944 &
./target/release/dkg-standalone-node --tmp -lerror --bob --ws-port 9945 &
./target/release/dkg-standalone-node --tmp \
    --ws-port 9946 \
    -lerror \
    -ldkg=debug \
    -ldkg_metadata=debug \
    -lruntime::offchain=debug \
    -ldkg_proposal_handler=debug \
    --charlie
