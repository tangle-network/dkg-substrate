#!/usr/bin/env -S deno run -A --unstable

/// This a script to run and setup the dkg network parachain.

import { ApiPromise, WsProvider, Keyring } from "@polkadot/api";
import { ProposalHeader, ResourceId, ChainType, AnchorUpdateProposal } from "@webb-tools/sdk-core";
import { localChain } from "../tests/utils/util";

async function run() {
  const api = await ApiPromise.create({
    provider: new WsProvider("ws://127.0.0.1:9944"),
  });
  await api.isReady;
  const keyring = new Keyring({ type: "sr25519" });
  const alice = keyring.addFromUri("//Alice");

  // 000000000000d30c8839c1145609e564b986f667b273ddcb8496100000001389
  const resourceId = ResourceId.newFromContractAddress(
    "0xd30c8839c1145609e564b986f667b273ddcb8496",
    ChainType.EVM,
    5002,
  );

  const createHeader = (nonce: number) => new ProposalHeader(
    resourceId,
    Buffer.from('0x00000000', 'hex'),
    nonce,
  );

  // 000000000000e69a847cd5bc0c9480ada0b339d7f0a8cac2b66701000000138a
  const srcResourceId = ResourceId.newFromContractAddress(
    "0xe69a847cd5bc0c9480ada0b339d7f0a8cac2b667",
    ChainType.EVM,
    5001,
  );

  // Create a new anchor proposal every 10 seconds.
  // Each time increment the nonce by 1.
  let nonce = 0;
  setInterval(async () => {
    // Create the header
    const proposalHeader = createHeader(nonce);
    // Create the anchor proposal data structure
    const anchorUpdateProposal: AnchorUpdateProposal = new AnchorUpdateProposal(
      proposalHeader,
      '0x00',
      srcResourceId
    );
    console.log('Created anchor update proposal: ', anchorUpdateProposal);
    // Submit the unsigned proposal to the chain
    const call = api.tx.dkgProposalHandler.forceSubmitUnsignedProposal(
      {
        Unsigned: {
          kind: 'AnchorUpdate',
          data: anchorUpdateProposal,
        }
      }
    );
    // Sign and send the transaction
    const unsub = await api.tx.sudo.sudo(call).signAndSend(alice, (result) => {
      console.log(result.status.toHuman());
      if (result.isFinalized || result.isError) {
        unsub();
      }
    });

    nonce += 1;
  }, 10000);
}

run()