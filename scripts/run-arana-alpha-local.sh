#!/bin/sh

#!/usr/bin/env bash
set -e

CLEAN=${CLEAN:-false}
# Parse arguments for the script

while [[ $# -gt 0 ]]; do
    key="$1"
    case $key in
        -c|--clean)
            CLEAN=true
            shift # past argument
            ;;
        *)    # unknown option
            shift # past argument
            ;;
    esac
done

pushd .

# Check if we should clean the tmp directory
if [ "$CLEAN" = true ]; then
  echo "Cleaning tmp directory"
  rm -rf ./tmp
fi

# The following line ensure we run from the project root
PROJECT_ROOT=$(git rev-parse --show-toplevel)
cd "$PROJECT_ROOT"

echo "**Generate new chainspec**"
./target/release/dkg-standalone-node build-spec --chain arana-local > ./chainspecs/arana-alpha-local.json

echo "** Inserting keys **"
./scripts/insert_keys.sh

echo "*** Start Webb DKG Standalone | Alpha Arana Config ***"
# Node 1
./target/release/dkg-standalone-node --base-path=./tmp/standalone1 -lerror --chain ./chainspecs/arana-alpha-local.json --validator \
  --rpc-cors all --unsafe-rpc-external --unsafe-ws-external \
  --port 30304 --node-key a0a85717f48521fb98f98c2d6ef035f4e372ed7b2fff145a0404365f5e248b52 \
  --ws-port 9944 &
# Node 2
./target/release/dkg-standalone-node --base-path=./tmp/standalone2 -lerror --chain ./chainspecs/arana-alpha-local.json --validator \
  --rpc-cors all --unsafe-rpc-external --unsafe-ws-external \
  --port 30305 --bootnodes /ip4/127.0.0.1/tcp/30304/p2p/12D3KooWHCQfqSALVHEM5yeUFQw1BwKYd1vywYa2Pgym47fnVf6U \
  --ws-port 9945 &
# Node 3
./target/release/dkg-standalone-node --base-path=./tmp/standalone3 -lerror --chain ./chainspecs/arana-alpha-local.json --validator \
  --rpc-cors all --unsafe-rpc-external --unsafe-ws-external \
  --port 30306 --bootnodes /ip4/127.0.0.1/tcp/30304/p2p/12D3KooWHCQfqSALVHEM5yeUFQw1BwKYd1vywYa2Pgym47fnVf6U \
  --ws-port 9946 &
# Node 4
./target/release/dkg-standalone-node --base-path=./tmp/standalone4 -lerror --chain ./chainspecs/arana-alpha-local.json --validator \
  --rpc-cors all --unsafe-rpc-external --unsafe-ws-external \
  --port 30307 --bootnodes /ip4/127.0.0.1/tcp/30304/p2p/12D3KooWHCQfqSALVHEM5yeUFQw1BwKYd1vywYa2Pgym47fnVf6U \
  --ws-port 9947 &
# Node 5
./target/release/dkg-standalone-node --base-path=./tmp/standalone5 -linfo --validator --chain ./chainspecs/arana-alpha-local.json \
    --rpc-cors all --unsafe-rpc-external --unsafe-ws-external \
    --ws-port 9948 \
    --port 30308 --bootnodes /ip4/127.0.0.1/tcp/30304/p2p/12D3KooWHCQfqSALVHEM5yeUFQw1BwKYd1vywYa2Pgym47fnVf6U \
    -ldkg=debug \
    -ldkg_gadget::worker=debug \
    -lruntime::dkg_metadata=debug \
    -ldkg_metadata=debug \
    -lruntime::dkg_proposal_handler=debug \
    -lruntime::offchain=debug \
    -ldkg_proposal_handler=debug
popd
