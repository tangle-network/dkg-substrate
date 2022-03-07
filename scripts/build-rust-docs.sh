#!/bin/bash

cd target/doc
git init
echo '<meta http-equiv="refresh" content="0; url=https://webb-tools.github.io/dkg-substrate/dkg_gadget/index.html">' > index.html
git add .
git config --global -l
git -c user.name='ci' -c user.email='ci' commit -m 'Deploy documentation'
git push -f -q https://git:${GITHUB_TOKEN}@github.com/webb-tools/dkg-substrate HEAD:gh-pages