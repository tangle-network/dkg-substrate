#!/bin/sh
cargo build --release --features=integration-tests
cd dkg-test-suite && yarn && yarn test
