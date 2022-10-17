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

echo "*** Start Webb DKG Node ***"
# Alice
./target/release/dkg-standalone-node --base-path=./tmp/alice -lerror --alice \
  --rpc-cors all --ws-external \
  --port 30304 \
  --ws-port 9944 &
# Bob
./target/release/dkg-standalone-node --base-path=./tmp/bob -lerror --bob \
  --rpc-cors all --ws-external \
  --port 30305 \
  --ws-port 9945 &
# Charlie
./target/release/dkg-standalone-node --base-path=./tmp/charlie -lerror --charlie \
  --rpc-cors all --ws-external \
  --port 30306 \
  --ws-port 9946 &
# Dave
./target/release/dkg-standalone-node --base-path=./tmp/dave -lerror --dave \
  --rpc-cors all --ws-external \
  --port 30307 \
  --ws-port 9947 &
# Eve
./target/release/dkg-standalone-node --base-path=./tmp/eve -linfo --eve \
    --rpc-cors all --ws-external \
    --ws-port 9948 \
    --port 30308 \
    -ldkg=debug \
    -ldkg_gadget::worker=debug \
    -lruntime::dkg_metadata=debug \
    -ldkg_metadata=debug \
    -lruntime::dkg_proposal_handler=debug \
    -lruntime::offchain=debug \
    -ldkg_proposal_handler=debug
popd
