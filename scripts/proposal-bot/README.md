## Prepare tests:

1. Run `cd scripts/proposal-bot`
2. Run `yarn install`
3. Install [ts-node](https://typestrong.org/ts-node/)

# Run tests:

From terminal 1:

1. Run `./scripts/run-standalone.sh`
2. Wait until Keygen completes

From terminal 2:

1. Run `ts-node ./src/submitProposals.ts` for anchor proposal tests
