#!/usr/bin/env bash

set -e

echo "*** Start Webb DKG Node ***"
./target/release/dkg-standalone-node --tmp -lerror --alice --rpc-cors all --ws-external &
./target/release/dkg-standalone-node --tmp -lerror --bob &
./target/release/dkg-standalone-node --tmp \
    -lerror \
    -ldkg=debug \
    -ldkg_metadata=debug \
    -lruntime::offchain=debug \
    -ldkg_proposal_handler=debug \
    --charlie
