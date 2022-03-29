/*
 * Copyright 2022 Webb Technologies Inc.
 *
 * Licensed under the Apache License, Version 2.0 (the "License");
 * you may not use this file except in compliance with the License.
 * You may obtain a copy of the License at
 *
 * http://www.apache.org/licenses/LICENSE-2.0
 *
 * Unless required by applicable law or agreed to in writing, software
 * distributed under the License is distributed on an "AS IS" BASIS,
 * WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
 * See the License for the specific language governing permissions and
 * limitations under the License.
 *
 */
import {
  encodeFunctionSignature,
  registerResourceId,
  waitForEvent,
} from '../src/utils';
import { Keyring } from '@polkadot/api';
import {hexToNumber, u8aToHex} from '@polkadot/util';
import {Option} from '@polkadot/types';
import {HexString} from '@polkadot/util/types';
import {MaxDepositLimitProposal, signAndSendUtil, ChainIdType, encodeMaxDepositLimitProposal} from '../src/evm/util/utils';
import { BLOCK_TIME } from '../src/constants';
import { localChain, polkadotApi, signatureVBridge } from './utils/util';
import { expect } from 'chai';
import { ethers } from 'ethers';

it('should be able to update max deposit limit', async () => {
  const vAnchor = signatureVBridge.getVAnchor(localChain.chainId)!;
  const resourceId = await vAnchor.createResourceId();
  // Create Mintable Token to add to GovernedTokenWrapper
  //Create an ERC20 Token
  const proposalPayload: MaxDepositLimitProposal = {
    header: {
      resourceId,
      functionSignature: encodeFunctionSignature(
        vAnchor.contract.interface.functions['configureMaximumDepositLimit(uint256)'].format()
      ),
      nonce: Number(await vAnchor.contract.getProposalNonce()) + 1,
      chainId: localChain.chainId,
      chainIdType: ChainIdType.EVM
    },
    maxDepositLimitBytes: "0x50000000",
  };
  // register proposal resourceId.
  await registerResourceId(polkadotApi, proposalPayload.header.resourceId);
  const proposalBytes = encodeMaxDepositLimitProposal(proposalPayload);
  // get alice account to send the transaction to the dkg node.
  const keyring = new Keyring({type: 'sr25519'});
  const alice = keyring.addFromUri('//Alice');
  const prop = u8aToHex(proposalBytes);
  const chainIdType = polkadotApi.createType('WebbProposalsHeaderTypedChainId', {
    Evm: localChain.chainId,
  });
  const kind = polkadotApi.createType('DkgRuntimePrimitivesProposalProposalKind', 'MaxDepositLimitUpdate');
  const maxDepositLimitProposal = polkadotApi.createType('DkgRuntimePrimitivesProposal', {
    Unsigned: {
      kind: kind,
      data: prop
    }
  });
  const proposalCall = polkadotApi.tx.dKGProposalHandler.forceSubmitUnsignedProposal(maxDepositLimitProposal);

  await signAndSendUtil(polkadotApi, proposalCall, alice);

  // now we need to wait until the proposal to be signed on chain.
  await waitForEvent(polkadotApi, 'dKGProposalHandler', 'ProposalSigned', { key: 'MaxDepositLimitUpdateProposal' });
  // now we need to query the proposal and its signature.
  const key = {
    MaxDepositLimitUpdateProposal: proposalPayload.header.nonce,
  };
  const proposal = await polkadotApi.query.dKGProposalHandler.signedProposals(chainIdType, key);
  const value = new Option(polkadotApi.registry, 'DkgRuntimePrimitivesProposal', proposal);
  expect(value.isSome).to.eq(true);
  const dkgProposal = value.unwrap().toJSON() as {
    signed: {
      kind: 'MaxDepositLimitUpdate';
      data: HexString;
      signature: HexString;
    };
  };
  // perfect! now we need to send it to the signature bridge.
  const bridgeSide = await signatureVBridge.getVBridgeSide(localChain.chainId);
  const contract = bridgeSide.contract;

  const governor = await contract.governor();
  const isSignedByGovernor = await contract.isSignatureFromGovernor(
    dkgProposal.signed.data,
    dkgProposal.signed.signature
  );
  expect(isSignedByGovernor).to.eq(true);
  // check that we have the resouceId mapping.
  const tx2 = await contract.executeProposalWithSignature(
    dkgProposal.signed.data,
    dkgProposal.signed.signature
  );
  await tx2.wait();
  // Want to check that fee was updated
  const maxDepositLimit = await vAnchor.contract.maximumDepositAmount();
  expect(hexToNumber("0x50000000").toString()).to.eq(maxDepositLimit.toString());
});
