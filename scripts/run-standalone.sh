#!/usr/bin/env bash
set -e
# ensure we kill all child processes when we exit
trap "trap - SIGTERM && kill -- -$$" SIGINT SIGTERM EXIT

#define default ports
ports=(30304 30305 30308)

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
cargo run --bin dkg-standalone-node --release -- --base-path=./tmp/alice --chain local -lerror --alice --output-path=./tmp/alice/output.log \
  --rpc-cors all --ws-external \
  --port ${ports[0]} \
  --ws-port 9944 &
# Bob
cargo run --bin dkg-standalone-node --release -- --base-path=./tmp/bob --chain local -lerror --bob --output-path=./tmp/bob/output.log \
  --rpc-cors all --ws-external \
  --port ${ports[1]} \
  --ws-port 9945 &
# Charlie
cargo run --bin dkg-standalone-node --release -- --base-path=./tmp/charlie --chain local -linfo --charlie --output-path=./tmp/charlie/output.log \
    --rpc-cors all --ws-external \
    --ws-port 9948 \
    --port ${ports[2]} \
    -ldkg=debug \
    -ldkg_gadget::worker=debug \
    -lruntime::dkg_metadata=debug \
    -ldkg_metadata=debug \
    -lruntime::dkg_proposal_handler=debug \
    -lruntime::offchain=debug \
    -ldkg_proposal_handler=debug
popd
