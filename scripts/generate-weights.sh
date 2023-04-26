#!/usr/bin/env bash
set -e

# This script will run the benchmark script and save the results in the weights file of the given pallet
# To run this script, first build the node with the benchmark features, like 'cargo b --features runtime-benchmarks'
# Then execute the script with the name of the pallet to benchmark
# Example : ./generate-weights dkg-proposals

echo "Generate Weights for : $1"

./target/release/dkg-standalone-node benchmark pallet \
  --chain=dev \
  --steps=20 \
  --repeat=1 \
  --log=warn \
  --pallet="pallet-${1}" \
  --extrinsic="*" \
  --execution=wasm \
  --wasm-execution=compiled \
  --output="./pallets/$1/src/weights.rs" \
  --template=./.maintain/frame-weight-template.hbs