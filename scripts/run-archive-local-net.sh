#!/usr/bin/env bash


RUST_LOG=dkg=trace ./target/release/dkg-standalone-node   --tmp --alice   --pruning=archive --rpc-cors=all &
RUST_LOG=dkg=trace ./target/release/dkg-standalone-node   --tmp --bob --pruning=archive --rpc-cors=all &
RUST_LOG=dkg=trace ./target/release/dkg-standalone-node  --tmp --charlie --pruning=archive --rpc-cors=all &
