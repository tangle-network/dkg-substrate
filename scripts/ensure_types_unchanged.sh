#!/bin.sh
set -e
cd $(git rev-parse --show-toplevel)/types
if [[ $(git status --porcelain | grep -v "yarn.lock\|package.json" | grep "M") ]]; then
    echo "The following types have changed"
    git status --porcelain | grep -v "yarn.lock\|package.json" | grep "M"
    exit 1
else
    exit 0
fi
