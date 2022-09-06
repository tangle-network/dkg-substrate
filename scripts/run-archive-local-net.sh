#!/usr/bin/env bash


RUST_LOG=dkg=trace ./target/release/dkg-standalone-node  --base-path=./tmp/alice  --alice   --pruning=archive --rpc-cors=all &
RUST_LOG=dkg=trace ./target/release/dkg-standalone-node  --base-path=./tmp/bob  --bob --pruning=archive --rpc-cors=all &
RUST_LOG=dkg=trace ./target/release/dkg-standalone-node  --base-path=./tmp/charlie --charlie --pruning=archive --rpc-cors=all &
