#!/usr/bin/env bash
set -e

pushd .

# The following line ensure we run from the project root
PROJECT_ROOT=`git rev-parse --show-toplevel`
cd $PROJECT_ROOT

echo "*** Start Webb DKG Node ***"
./target/release/dkg-standalone-node --tmp -lerror --alice --rpc-cors all --ws-external --ws-port 9944 &
./target/release/dkg-standalone-node --tmp -lerror --bob --ws-port 9945 &
./target/release/dkg-standalone-node --tmp \
    --ws-port 9946 \
    -ldkg=debug \
    -ldkg_gossip=trace \
    -ldkg_metadata=debug \
    -lruntime::offchain=debug \
    -ldkg_proposal_handler=debug \
    --charlie
popd
