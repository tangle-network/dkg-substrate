#!/usr/bin/env bash
set -e

pushd .

# The following line ensure we run from the project root
PROJECT_ROOT=`git rev-parse --show-toplevel`
cd $PROJECT_ROOT

node1="Tim"
node2="Jon"
node3="Lou"

# ========================= TIMS KEYS ========================= #
# ========================= account key ========================= #
echo "*** Adding account key to keystore for ${node1} in $1/${node1} ***" 
./target/release/dkg-standalone-node key insert --base-path $1/${node1} \
--chain "./chainspecs/arana-raw.json" \
--scheme Sr25519 \
--suri "gown surprise mirror hotel cash alarm raccoon you frog rose midnight enter//webb//0" \
--key-type acco
# ========================= stash key ========================= #
echo "*** Adding stash key to keystore for ${node1} in $1/${node1} ***" 
./target/release/dkg-standalone-node key insert --base-path $1/${node1} \
--chain "./chainspecs/arana-raw.json" \
--scheme Sr25519 \
--suri "gown surprise mirror hotel cash alarm raccoon you frog rose midnight enter//webb//0//stash" \
--key-type acco
# ========================= aura key ========================= #
echo "*** Adding aura key to keystore for ${node1} in $1/${node1} ***" 
./target/release/dkg-standalone-node key insert --base-path $1/${node1} \
--chain "./chainspecs/arana-raw.json" \
--scheme Sr25519 \
--suri "gown surprise mirror hotel cash alarm raccoon you frog rose midnight enter//webb//0//aura" \
--key-type aura

# ========================= grandpa key ========================= #
echo "*** Adding grandpa key to keystore for ${node1} in $1/${node1} ***" 
./target/release/dkg-standalone-node key insert --base-path $1/${node1} \
--chain "./chainspecs/arana-raw.json" \
--scheme Ed25519 \
--suri "gown surprise mirror hotel cash alarm raccoon you frog rose midnight enter//webb//0//grandpa" \
--key-type gran

# ========================= dkg key ========================= #
echo "*** Adding dkg key to keystore for ${node1} in $1/${node1} ***" 
./target/release/dkg-standalone-node key insert --base-path $1/${node1} \
--chain "./chainspecs/arana-raw.json"  \
--scheme Ecdsa \
--suri "gown surprise mirror hotel cash alarm raccoon you frog rose midnight enter//webb//0//dkg" \
--key-type wdkg

# ========================= JONS KEYS ========================= #
# ========================= account key ========================= #
echo "*** Adding account key to keystore for ${node2} in $1/${node2} ***" 
./target/release/dkg-standalone-node key insert --base-path $1/${node2} \
--chain "./chainspecs/arana-raw.json" \
--scheme Sr25519 \
--suri "gown surprise mirror hotel cash alarm raccoon you frog rose midnight enter//webb//1" \
--key-type acco
# ========================= stash key ========================= #
echo "*** Adding stash key to keystore for ${node2} in $1/${node2} ***" 
./target/release/dkg-standalone-node key insert --base-path $1/${node2} \
--chain "./chainspecs/arana-raw.json" \
--scheme Sr25519 \
--suri "gown surprise mirror hotel cash alarm raccoon you frog rose midnight enter//webb//1//stash" \
--key-type acco
# ========================= aura key ========================= #
echo "*** Adding aura key to keystore for ${node2} in $1/${node2} ***" 
./target/release/dkg-standalone-node key insert --base-path $1/${node2} \
--chain "./chainspecs/arana-raw.json" \
--scheme Sr25519 \
--suri "gown surprise mirror hotel cash alarm raccoon you frog rose midnight enter//webb//1//aura" \
--key-type aura

# ========================= grandpa key ========================= #
echo "*** Adding grandpa key to keystore for ${node2} in $1/${node2} ***" 
./target/release/dkg-standalone-node key insert --base-path $1/${node2} \
--chain "./chainspecs/arana-raw.json" \
--scheme Ed25519 \
--suri "gown surprise mirror hotel cash alarm raccoon you frog rose midnight enter//webb//1//grandpa" \
--key-type gran

# ========================= dkg key ========================= #
echo "*** Adding dkg key to keystore for ${node2} in $1/${node2} ***" 
./target/release/dkg-standalone-node key insert --base-path $1/${node2} \
--chain "./chainspecs/arana-raw.json"  \
--scheme Ecdsa \
--suri "gown surprise mirror hotel cash alarm raccoon you frog rose midnight enter//webb//1//dkg" \
--key-type wdkg

# ========================= LOU KEYS ========================= #
# ========================= account key ========================= #
echo "*** Adding account key to keystore for ${node3} in $1/${node3} ***" 
./target/release/dkg-standalone-node key insert --base-path $1/${node3} \
--chain "./chainspecs/arana-raw.json" \
--scheme Sr25519 \
--suri "gown surprise mirror hotel cash alarm raccoon you frog rose midnight enter//webb//2" \
--key-type acco
# ========================= stash key ========================= #
echo "*** Adding stash key to keystore for ${node3} in $1/${node3} ***" 
./target/release/dkg-standalone-node key insert --base-path $1/${node3} \
--chain "./chainspecs/arana-raw.json" \
--scheme Sr25519 \
--suri "gown surprise mirror hotel cash alarm raccoon you frog rose midnight enter//webb//2//stash" \
--key-type acco
# ========================= aura key ========================= #
echo "*** Adding aura key to keystore for ${node3} in $1/${node3} ***" 
./target/release/dkg-standalone-node key insert --base-path $1/${node3} \
--chain "./chainspecs/arana-raw.json" \
--scheme Sr25519 \
--suri "gown surprise mirror hotel cash alarm raccoon you frog rose midnight enter//webb//2//aura" \
--key-type aura

# ========================= grandpa key ========================= #
echo "*** Adding grandpa key to keystore for ${node3} in $1/${node3} ***" 
./target/release/dkg-standalone-node key insert --base-path $1/${node3} \
--chain "./chainspecs/arana-raw.json" \
--scheme Ed25519 \
--suri "gown surprise mirror hotel cash alarm raccoon you frog rose midnight enter//webb//2//grandpa" \
--key-type gran

# ========================= dkg key ========================= #
echo "*** Adding dkg key to keystore for ${node3} in $1/${node3} ***" 
./target/release/dkg-standalone-node key insert --base-path $1/${node3} \
--chain "./chainspecs/arana-raw.json"  \
--scheme Ecdsa \
--suri "gown surprise mirror hotel cash alarm raccoon you frog rose midnight enter//webb//2//dkg" \
--key-type wdkg

# echo "*** Starting x3 Webb DKG Nodes ***"
# #  Add below in start for verbose debugging
# # -lerror -ldkg=debug -ldkg_metadata=debug -lruntime::offchain=debug -ldkg_proposal_handler=debug
# trap 'exit 130' INT
# ./target/release/dkg-standalone-node --base-path $1/${node1} --name ${node1}  --chain "./chainspecs/arana-raw.json" --validator --rpc-cors "*" --rpc-port 9934 --ws-port 9944 --port 30333 &
# ./target/release/dkg-standalone-node --base-path $1/${node2} --name ${node2}  --chain "./chainspecs/arana-raw.json" --validator --rpc-cors "*" --rpc-port 9935 --ws-port 9945 --port 30334 &
# ./target/release/dkg-standalone-node --base-path $1/${node3} --name ${node3}  --chain "./chainspecs/arana-raw.json" --validator --rpc-cors "*" --rpc-port 9936 --ws-port 9947  --port 30335
# popd
