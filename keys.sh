#!/bin/sh

echo "`\n`****************** NODE-1 KEY INSERTION ******************`\n`"

./target/release/dkg-standalone-node key insert --base-path /tmp/blockstorage/standalone-1 \
--chain "./chainspecs/arana-alpha-raw.json" \
--scheme Sr25519 \
--suri "gown surprise mirror hotel cash alarm raccoon you frog rose midnight enter//webb//0" \
--key-type acco

./target/release/dkg-standalone-node key insert --base-path /tmp/blockstorage/standalone-1 \
--chain "./chainspecs/arana-alpha-raw.json" \
--scheme Sr25519 \
--suri "gown surprise mirror hotel cash alarm raccoon you frog rose midnight enter//webb//0//stash" \
--key-type acco

./target/release/dkg-standalone-node key insert --base-path /tmp/blockstorage/standalone-1 \
--chain "./chainspecs/arana-alpha-raw.json" \
--scheme Sr25519 \
--suri "gown surprise mirror hotel cash alarm raccoon you frog rose midnight enter//webb//0//aura" \
--key-type aura

./target/release/dkg-standalone-node key insert --base-path /tmp/blockstorage/standalone-1 \
--chain "./chainspecs/arana-alpha-raw.json" \
--scheme Ed25519 \
--suri "gown surprise mirror hotel cash alarm raccoon you frog rose midnight enter//webb//0//grandpa" \
--key-type gran

./target/release/dkg-standalone-node key insert --base-path /tmp/blockstorage/standalone-1 \
--chain "./chainspecs/arana-alpha-raw.json" \
--scheme Ecdsa \
--suri "gown surprise mirror hotel cash alarm raccoon you frog rose midnight enter//webb//0//dkg" \
--key-type wdkg

echo "`\n`****************** NODE-2 KEY INSERTION ******************`\n`"

./target/release/dkg-standalone-node key insert --base-path /tmp/blockstorage/standalone-2 \
--chain "./chainspecs/arana-alpha-raw.json" \
--scheme Sr25519 \
--suri "gown surprise mirror hotel cash alarm raccoon you frog rose midnight enter//webb//1" \
--key-type acco

./target/release/dkg-standalone-node key insert --base-path /tmp/blockstorage/standalone-2 \
--chain "./chainspecs/arana-alpha-raw.json" \
--scheme Sr25519 \
--suri "gown surprise mirror hotel cash alarm raccoon you frog rose midnight enter//webb//1//stash" \
--key-type acco

./target/release/dkg-standalone-node key insert --base-path /tmp/blockstorage/standalone-2 \
--chain "./chainspecs/arana-alpha-raw.json" \
--scheme Sr25519 \
--suri "gown surprise mirror hotel cash alarm raccoon you frog rose midnight enter//webb//1//aura" \
--key-type aura

./target/release/dkg-standalone-node key insert --base-path /tmp/blockstorage/standalone-2 \
--chain "./chainspecs/arana-alpha-raw.json" \
--scheme Ed25519 \
--suri "gown surprise mirror hotel cash alarm raccoon you frog rose midnight enter//webb//1//grandpa" \
--key-type gran

./target/release/dkg-standalone-node key insert --base-path /tmp/blockstorage/standalone-2 \
--chain "./chainspecs/arana-alpha-raw.json" \
--scheme Ecdsa \
--suri "gown surprise mirror hotel cash alarm raccoon you frog rose midnight enter//webb//1//dkg" \
--key-type wdkg


echo "`\n`****************** NODE-3 KEY INSERTION ******************`\n`"

./target/release/dkg-standalone-node key insert --base-path /tmp/blockstorage/standalone-3 \
--chain "./chainspecs/arana-alpha-raw.json" \
--scheme Sr25519 \
--suri "gown surprise mirror hotel cash alarm raccoon you frog rose midnight enter//webb//2" \
--key-type acco

./target/release/dkg-standalone-node key insert --base-path /tmp/blockstorage/standalone-3 \
--chain "./chainspecs/arana-alpha-raw.json" \
--scheme Sr25519 \
--suri "gown surprise mirror hotel cash alarm raccoon you frog rose midnight enter//webb//2//stash" \
--key-type acco

./target/release/dkg-standalone-node key insert --base-path /tmp/blockstorage/standalone-3 \
--chain "./chainspecs/arana-alpha-raw.json" \
--scheme Sr25519 \
--suri "gown surprise mirror hotel cash alarm raccoon you frog rose midnight enter//webb//2//aura" \
--key-type aura

./target/release/dkg-standalone-node key insert --base-path /tmp/blockstorage/standalone-3 \
--chain "./chainspecs/arana-alpha-raw.json" \
--scheme Ed25519 \
--suri "gown surprise mirror hotel cash alarm raccoon you frog rose midnight enter//webb//2//grandpa" \
--key-type gran

./target/release/dkg-standalone-node key insert --base-path /tmp/blockstorage/standalone-3 \
--chain "./chainspecs/arana-alpha-raw.json" \
--scheme Ecdsa \
--suri "gown surprise mirror hotel cash alarm raccoon you frog rose midnight enter//webb//2//dkg" \
--key-type wdkg