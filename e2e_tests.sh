#!/bin/sh
cargo build --release
cd dkg-test-suite && yarn && yarn test
