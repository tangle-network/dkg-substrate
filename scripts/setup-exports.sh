#!/usr/bin/env bash

set -e

echo "Setup Exports for easy testing"
NODE=./target/release/dkg-standalone-node
COMMON_ARGS="-lerror -ldkg=debug -ldkg_metadata=debug -ldkg_proposal_handler=debug"

function run-alice-node() {
    PROMPT_COMMAND='echo -en "\033]0;Alice Node\a"'
    eval "$NODE --alice --tmp $COMMON_ARGS"
}

function run-bob-node() {
    PROMPT_COMMAND='echo -en "\033]0;Bob Node\a"'
    eval "$NODE --bob --tmp $COMMON_ARGS"
}

function run-charlie-node() {
    PROMPT_COMMAND='echo -en "\033]0;Charlie Node\a"'
    eval "$NODE --charlie --tmp $COMMON_ARGS"
}

echo "To use this script, run the following command:"
echo ". ./scripts/setup-exports.sh"
echo "Then, you can use the following commands:"
echo
echo "'run-alice-node' to run Alice node"
echo "'run-bob-node' to run Bob node"
echo "'run-charlie-node' to run Charlie node"
