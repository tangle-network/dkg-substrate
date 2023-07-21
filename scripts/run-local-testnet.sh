#!/bin/bash
set -e
# ensure we kill all child processes when we exit
trap "trap - SIGTERM && kill -- -$$" SIGINT SIGTERM EXIT

#define default ports
ports=(30333 30305 30308 30309 30310)

#check to see process is not orphaned or already running
for port in ${ports[@]}; do
    if [[ $(lsof -i -P -n | grep LISTEN | grep :$port) ]]; then
      echo "Port $port has a running process. Exiting"
      exit -1
    fi
done

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
./target/release/dkg-standalone-node --tmp --chain local --validator -lerror --alice --output-path=./tmp/alice/output.log \
  --rpc-cors all --rpc-external --rpc-methods=unsafe \
  --port 30333 \
  --rpc-port 9944 --node-key 0000000000000000000000000000000000000000000000000000000000000001 &
# Bob
./target/release/dkg-standalone-node --tmp --chain local --validator -lerror --bob --output-path=./tmp/bob/output.log \
  --rpc-cors all --rpc-external --rpc-methods=unsafe \
  --port 30305 \
  --rpc-port 9945 --bootnodes /ip4/127.0.0.1/tcp/30333/p2p/12D3KooWEyoppNCUx8Yx66oV9fJnriXwCcXwDDUA2kj6vnc6iDEp &
# Charlie
./target/release/dkg-standalone-node --tmp --chain local --validator -lerror --charlie --output-path=./tmp/charlie/output.log \
  --rpc-cors all --rpc-external --rpc-methods=unsafe \
  --port 30308 \
  --rpc-port 9946 --bootnodes /ip4/127.0.0.1/tcp/30333/p2p/12D3KooWEyoppNCUx8Yx66oV9fJnriXwCcXwDDUA2kj6vnc6iDEp &
# Dave
./target/release/dkg-standalone-node --tmp --chain local --validator -lerror --dave --output-path=./tmp/dave/output.log \
  --rpc-cors all --rpc-external --rpc-methods=unsafe \
  --port 30309 \
  --rpc-port 9947 --bootnodes /ip4/127.0.0.1/tcp/30333/p2p/12D3KooWEyoppNCUx8Yx66oV9fJnriXwCcXwDDUA2kj6vnc6iDEp &
# Eve
./target/release/dkg-standalone-node --tmp --chain local --validator -linfo --eve --output-path=./tmp/eve/output.log \
    --rpc-cors all --rpc-external \
    --rpc-port 9948 \
    --port 30310 \
    -ldkg=debug \
    -ldkg_gadget::worker=debug \
    -lruntime::dkg_metadata=debug \
    -ldkg_metadata=debug \
    -lruntime::dkg_proposal_handler=debug \
    -lruntime::offchain=debug --bootnodes /ip4/127.0.0.1/tcp/30333/p2p/12D3KooWEyoppNCUx8Yx66oV9fJnriXwCcXwDDUA2kj6vnc6iDEp \
    -ldkg_proposal_handler=debug --unsafe-rpc-external --rpc-methods=unsafe
popd
