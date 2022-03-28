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
import 'jest-extended';
import {jest} from "@jest/globals";
import {
	encodeFunctionSignature,
	registerResourceId,
	waitForEvent,
} from '../../../src/utils';
import {ApiPromise, Keyring} from '@polkadot/api';
import {hexToNumber, u8aToHex} from '@polkadot/util';
import {Option} from '@polkadot/types';
import {HexString} from '@polkadot/util/types';
import {MinWithdrawalLimitProposal, signAndSendUtil, ChainIdType, encodeMinWithdrawalLimitProposal} from '../../../src/evm/util/utils';
import { LocalChain } from '../../../src/localEvm';
import { VBridge } from '@webb-tools/protocol-solidity';

export async function minWithdrawalTest(signatureVBridge: VBridge.VBridge, localChain: LocalChain, polkadotApi: ApiPromise) {
	const vAnchor = signatureVBridge.getVAnchor(localChain.chainId)!;
	const resourceId = await vAnchor.createResourceId();
	// Create Mintable Token to add to GovernedTokenWrapper
	//Create an ERC20 Token
	const proposalPayload: MinWithdrawalLimitProposal = {
		header: {
			resourceId,
			functionSignature: encodeFunctionSignature(
				vAnchor.contract.interface.functions['configureMinimalWithdrawalLimit(uint256)'].format()
			),
			nonce: Number(await vAnchor.contract.getProposalNonce()) + 1,
			chainId: localChain.chainId,
			chainIdType: ChainIdType.EVM
		},
		minWithdrawalLimitBytes: "0x50",
	};
	// register proposal resourceId.
	await expect(registerResourceId(polkadotApi, proposalPayload.header.resourceId)).toResolve();
	const proposalBytes = encodeMinWithdrawalLimitProposal(proposalPayload);
	// get alice account to send the transaction to the dkg node.
	const keyring = new Keyring({type: 'sr25519'});
	const alice = keyring.addFromUri('//Alice');
	const prop = u8aToHex(proposalBytes);
	const chainIdType = polkadotApi.createType('WebbProposalsHeaderTypedChainId', {
		Evm: localChain.chainId,
	});
	const kind = polkadotApi.createType('DkgRuntimePrimitivesProposalProposalKind', 'MinWithdrawalLimitUpdate');
	const minWithdrawalLimitProposal = polkadotApi.createType('DkgRuntimePrimitivesProposal', {
		Unsigned: {
			kind: kind,
			data: prop
		}
	});
	const proposalCall = polkadotApi.tx.dKGProposalHandler.forceSubmitUnsignedProposal(minWithdrawalLimitProposal);

	await signAndSendUtil(polkadotApi, proposalCall, alice);

	// now we need to wait until the proposal to be signed on chain.
	await waitForEvent(polkadotApi, 'dKGProposalHandler', 'ProposalSigned');
	// now we need to query the proposal and its signature.
	const key = {
		MinWithdrawalLimitUpdateProposal: proposalPayload.header.nonce,
	};
	const proposal = await polkadotApi.query.dKGProposalHandler.signedProposals(chainIdType, key);
	const value = new Option(polkadotApi.registry, 'DkgRuntimePrimitivesProposal', proposal);
	expect(value.isSome).toBeTrue();
	const dkgProposal = value.unwrap().toJSON() as {
		signed: {
			kind: 'MinWithdrawalLimitUpdate';
			data: HexString;
			signature: HexString;
		};
	};
	// sanity check.
	expect(dkgProposal.signed.data).toEqual(prop);
	// perfect! now we need to send it to the signature bridge.
	const bridgeSide = await signatureVBridge.getVBridgeSide(localChain.chainId);
	const contract = bridgeSide.contract;
	const isSignedByGovernor = await contract.isSignatureFromGovernor(
		dkgProposal.signed.data,
		dkgProposal.signed.signature
	);
	expect(isSignedByGovernor).toBeTrue();
	// check that we have the resouceId mapping.
	const tx2 = await contract.executeProposalWithSignature(
		dkgProposal.signed.data,
		dkgProposal.signed.signature
	);
	await expect(tx2.wait()).toResolve();
	// Want to check that fee was updated
	const minWithdrawalLimit = await vAnchor.contract.minimalWithdrawalAmount();
	expect(hexToNumber("0x50").toString()).toEqual(minWithdrawalLimit.toString());
}
