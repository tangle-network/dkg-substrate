// Import the API
const { ApiPromise } = require('@polkadot/api');
const { exit } = require('process');
const assert = require('assert').strict;

const BLOCKS_TO_WAIT_FOR = 5;

async function main () {
  // Here we don't pass the (optional) provider, connecting directly to the default
  // node/port, i.e. `ws://127.0.0.1:9944`. Await for the isReady promise to ensure
  // the API has connected to the node and completed the initialisation process
  const api = await ApiPromise.create('ws://127.0.0.1:9946');

  // We only display a couple, then unsubscribe
  let count = 0;

  // Subscribe to the new headers on-chain. The callback is fired when new headers
  // are found, the call itself returns a promise with a subscription that can be
  // used to unsubscribe from the newHead subscription
  // lets wait for first few blocks
  const unsubscribe = await api.rpc.chain.subscribeNewHeads(async (header) => {
    console.log(`Chain is at block: #${header.number}`);

    if (count > BLOCKS_TO_WAIT_FOR) {
      // ensure the keys are added onchain as expected
      await querydkgPublicKey(api);
      await querynextDkgPublicKey(api);
      await queryDkgPublicKeySignature(api);
      unsubscribe();
      process.exit(0);
    }
  });
}

// check if the dkgPublicKey has been added onchain
async function querydkgPublicKey(api) {
  try {
    let dkg_key = JSON.parse(await api.query.dkg.dkgPublicKey());
    console.log("DKG Public key is ", dkg_key);
    assert(dkg_key[0] != 0);
    assert(dkg_key[1] != undefined);
  } catch(err) {
    console.log("Unable to query dkg public key");
    exit(1)
  }
}

// check if the nextDkgPublicKey has been added onchain
async function querynextDkgPublicKey(api) {
  try{
  let dkg_key = JSON.parse(await api.query.dkg.nextDKGPublicKey());
  console.log("NEXT DKG Public key is ", dkg_key);
  //assert(dkg_key.len() != 0);
  assert(dkg_key[0] != 0);
  assert(dkg_key[1] != undefined);
} catch(err) {
  console.log("Unable to query next dkg public key");
  exit(1)
}
}

// check if the dkgPublicKey has been added onchain
async function queryDkgPublicKeySignature(api) {
  try {
  let dkg_key = JSON.parse(await api.query.dkg.dkgPublicKeySignature());
  console.log("DKG Public key signarure is ", dkg_key);
  assert(dkg_key != '0x');
  } catch(err) {
    console.log("Unable to query dkg public key signature");
    exit(1)
  }
}

main().catch(console.error);