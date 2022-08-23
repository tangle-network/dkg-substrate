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
	waitForEvent,
	sudoTx,
} from './utils/setup';
import { Keyring } from '@polkadot/api';
import { hexToNumber, u8aToHex, hexToU8a } from '@polkadot/util';
import { Option } from '@polkadot/types';
import { HexString } from '@polkadot/util/types';
import {
	MinWithdrawalLimitProposal,
	ChainType,
	ResourceId,
	ProposalHeader,
} from '@webb-tools/sdk-core';
import {
	localChain,
	polkadotApi,
	signatureVBridge,
} from './utils/util';
import { expect } from 'chai';
import { ethers } from 'ethers';
import { registerResourceId } from '@webb-tools/test-utils';

it('should be able to update min withdrawal limit', async () => {
	const vAnchor = signatureVBridge.getVAnchor(localChain.typedChainId)!;
	const resourceId = ResourceId.newFromContractAddress(vAnchor.contract.address, ChainType.EVM, localChain.evmId);
	const functionSignature = hexToU8a(vAnchor.contract.interface.functions['configureMinimalWithdrawalLimit(uint256,uint32)'].format());
	const nonce = Number(await vAnchor.contract.getProposalNonce()) + 1;
	const proposalHeader = new ProposalHeader(resourceId, functionSignature, nonce);
	const minLimitProposal = new MinWithdrawalLimitProposal(proposalHeader, '0x50');

	// register proposal resourceId.
	await registerResourceId(polkadotApi, minLimitProposal.header.resourceId);
	const proposalBytes = minLimitProposal.toU8a();
	// get alice account to send the transaction to the dkg node.
	const keyring = new Keyring({ type: 'sr25519' });
	const alice = keyring.addFromUri('//Alice');
	const prop = u8aToHex(proposalBytes);
	const chainIdType = polkadotApi.createType(
		'WebbProposalsHeaderTypedChainId',
		{
			Evm: localChain.evmId,
		}
	);
	const minWithdrawalLimitProposal = polkadotApi.createType(
		'WebbProposalsProposal',
		{
			Unsigned: {
				kind: 'MinWithdrawalLimitUpdate',
				data: prop,
			},
		}
	);
	const proposalCall =
		polkadotApi.tx.dKGProposalHandler.forceSubmitUnsignedProposal(
			minWithdrawalLimitProposal
		);

	await sudoTx(polkadotApi, proposalCall);

	// now we need to wait until the proposal to be signed on chain.
	await waitForEvent(polkadotApi, 'dKGProposalHandler', 'ProposalSigned', {
		key: 'MinWithdrawalLimitUpdateProposal',
	});
	// now we need to query the proposal and its signature.
	const key = {
		MinWithdrawalLimitUpdateProposal: minLimitProposal.header.nonce,
	};
	const proposal = await polkadotApi.query.dKGProposalHandler.signedProposals(
		chainIdType,
		key
	);
	const value = new Option(
		polkadotApi.registry,
		'WebbProposalsProposal',
		proposal
	);
	expect(value.isSome).to.eq(true);
	const dkgProposal = value.unwrap().toJSON() as {
		signed: {
			kind: 'MinWithdrawalLimitUpdate';
			data: HexString;
			signature: HexString;
		};
	};

	let signature = dkgProposal.signed.signature.slice(2);
	let r = `0x${signature.slice(0, 32)}`;
	let s = `0x${signature.slice(32, 64)}`;
	let v = parseInt(`0x${signature[64]}`);
	let expandedSig = { r, s, v };
	ethers.utils.joinSignature(expandedSig);
	// try {
	//   sig = ethers.utils.joinSignature(expandedSig)
	// } catch (e) {
	//   expandedSig.s = '0x' + (new BN(ec.curve.n).sub(signature.s)).toString('hex');
	//   expandedSig.v = (expandedSig.v === 27) ? 28 : 27;
	//   sig = ethers.utils.joinSignature(expandedSig)
	// }

	// 66 byte string, which represents 32 bytes of data
	let messageHash = ethers.utils.solidityKeccak256(
		['bytes'],
		[dkgProposal.signed.data]
	);

	// 32 bytes of data in Uint8Array
	let messageHashBytes = ethers.utils.arrayify(messageHash);
	let recovered = ethers.utils.verifyMessage(
		messageHashBytes,
		dkgProposal.signed.signature
	);
	console.log(recovered);

	// sanity check.
	expect(dkgProposal.signed.data).to.eq(prop);
	// perfect! now we need to send it to the signature bridge.
	const bridgeSide = await signatureVBridge.getVBridgeSide(localChain.typedChainId);
	const contract = bridgeSide.contract;
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
	const minWithdrawalLimit = await vAnchor.contract.minimalWithdrawalAmount();
	expect(hexToNumber('0x50').toString()).to.eq(minWithdrawalLimit.toString());
});
