#!/bin/sh
set -e
for i in {1..10}; do
    cargo run --package dkg-test-orchestrator --features=debug-tracing -- --config ./dkg-test-orchestrator/config/test_n3t2.toml --tmp ./tmp --clean
done || exit
