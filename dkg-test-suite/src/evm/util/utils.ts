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
import { u8aToHex, hexToU8a, assert } from '@polkadot/util';
import {ApiPromise} from "@polkadot/api";
import {Bytes, Option} from "@polkadot/types";
import {KeyringPair} from "@polkadot/keyring/types";
import {Keyring} from "@polkadot/keyring";
import {ethers} from "ethers";

const LE = true;
const BE = false;
export const enum ChainIdType {
	UNKNOWN = 0x0000,
	EVM = 0x0100,
	SUBSTRATE = 0x0200,
	POLKADOT_RELAYCHAIN = 0x0301,
	KUSAMA_RELAYCHAIN = 0x0302,
	COSMOS = 0x0400,
	SOLANA = 0x0500,
}

/**
 * Proposal Header is the first 40 bytes of any proposal and it contains the following information:
 * - resource id (32 bytes)
 * - target chain id (4 bytes) encoded as the last 4 bytes of the resource id.
 * - target function signature (4 bytes)
 * - nonce (4 bytes).
 */
export interface ProposalHeader {
	/**
	 * 32 bytes Hex-encoded string of the `ResourceID` for this proposal.
	 */
	readonly resourceId: string;
	/**
	 * 2 bytes (u16) encoded as the last 2 bytes of the resource id **just** before the chainId.
	 *
	 * **Note**: this value is optional here since we can read it from the `ResourceID`, but would be provided for you if
	 * you want to decode the proposal header from bytes.
	 **/
	chainIdType?: ChainIdType;
	/**
	 * 4 bytes number (u32) of the `chainId` this also encoded in the last 4 bytes of the `ResourceID`.
	 *
	 * **Note**: this value is optional here since we can read it from the `ResourceID`, but would be provided for you if
	 * you want to decode the proposal header from bytes.
	 */
	chainId?: number;
	/**
	 * 4 bytes Hex-encoded string of the `functionSig` for this proposal.
	 */
	readonly functionSignature: string;
	/**
	 * 4 bytes Hex-encoded string of the `nonce` for this proposal.
	 */
	readonly nonce: number;
}

export function encodeProposalHeader(data: ProposalHeader): Uint8Array {
	const header = new Uint8Array(40);
	const resourceId = hexToU8a(data.resourceId).slice(0, 32);
	const functionSignature = hexToU8a(data.functionSignature).slice(0, 4);
	header.set(resourceId, 0); // 0 -> 32
	header.set(functionSignature, 32); // 32 -> 36
	const view = new DataView(header.buffer);
	view.setUint32(36, data.nonce, false); // 36 -> 40
	return header;
}

export function decodeProposalHeader(header: Uint8Array): ProposalHeader {
	const resourceId = u8aToHex(header.slice(0, 32));
	const chainIdTypeInt = new DataView(header.buffer).getUint16(32 - 6, BE);
	const chainIdType = castToChainIdType(chainIdTypeInt);
	const chainId = new DataView(header.buffer).getUint32(32 - 4, BE);
	const functionSignature = u8aToHex(header.slice(32, 36));
	const nonce = new DataView(header.buffer).getUint32(36, BE);
	return {
		resourceId,
		chainId,
		chainIdType,
		functionSignature,
		nonce,
	};
}

function castToChainIdType(v: number): ChainIdType {
	switch (v) {
		case 0x0100:
			return ChainIdType.EVM;
		case 0x0200:
			return ChainIdType.SUBSTRATE;
		case 0x0301:
			return ChainIdType.POLKADOT_RELAYCHAIN;
		case 0x0302:
			return ChainIdType.KUSAMA_RELAYCHAIN;
		case 0x0400:
			return ChainIdType.COSMOS;
		case 0x0500:
			return ChainIdType.SOLANA;
		default:
			return ChainIdType.UNKNOWN;
	}
}

/**
 * Anchor Update Proposal is the next 42 bytes (after the header) and it contains the following information:
 * - src chain type (2 bytes) just before the src chain id.
 * - src chain id (4 bytes) encoded as the 4 bytes.
 * - last leaf index (4 bytes).
 * - merkle root (32 bytes).
 */
 export interface AnchorUpdateProposal {
	/**
	 * The Anchor Proposal Header.
	 * This is the first 40 bytes of the proposal.
	 * See `encodeProposalHeader` for more details.
	 */
	readonly header: ProposalHeader;
	/**
	 * 2 bytes (u16) encoded as the last 2 bytes.
	 *
	 **/
	readonly chainIdType: ChainIdType;
	/**
	 * 4 bytes number (u32) of the `srcChainId`.
	 */
	readonly srcChainId: number;
	/**
	 * 4 bytes number (u32) of the `lastLeafIndex`.
	 */
	readonly lastLeafIndex: number;
	/**
	 * 32 bytes Hex-encoded string of the `merkleRoot`.
	 */
	readonly merkleRoot: string;
}
export function encodeUpdateAnchorProposal(proposal: AnchorUpdateProposal): Uint8Array {
	const header = encodeProposalHeader(proposal.header);
	const updateProposal = new Uint8Array(40 + 42);
	updateProposal.set(header, 0); // 0 -> 40
	const view = new DataView(updateProposal.buffer);
	view.setUint16(40, proposal.chainIdType, false); // 40 -> 42
	view.setUint32(42, proposal.srcChainId, false); // 42 -> 46
	view.setUint32(46, proposal.lastLeafIndex, false); // 46 -> 50
	const merkleRoot = hexToU8a(proposal.merkleRoot).slice(0, 32);
	updateProposal.set(merkleRoot, 50); // 50 -> 82
	return updateProposal;
}
export function decodeUpdateAnchorProposal(data: Uint8Array): AnchorUpdateProposal {
	const header = decodeProposalHeader(data.slice(0, 40)); // 0 -> 40
	const chainIdTypeInt = new DataView(data.buffer).getUint16(40, false); // 40 -> 42
	const chainIdType = castToChainIdType(chainIdTypeInt);
	const srcChainId = new DataView(data.buffer).getUint32(42, false); // 42 -> 46
	const lastLeafIndex = new DataView(data.buffer).getUint32(46, false); // 46 -> 50
	const merkleRoot = u8aToHex(data.slice(50, 80)); // 50 -> 82
	return {
		header,
		chainIdType,
		srcChainId,
		lastLeafIndex,
		merkleRoot,
	};
}

export interface TokenAddProposal {
	/**
	 * The Token Add Proposal Header.
	 * This is the first 40 bytes of the proposal.
	 * See `encodeProposalHeader` for more details.
	 */
	readonly header: ProposalHeader;
	/**
	 * 20 bytes Hex-encoded string.
	 */
	readonly newTokenAddress: string;
}

export interface TokenRemoveProposal {
	/**
	 * The Token Remove Proposal Header.
	 * This is the first 40 bytes of the proposal.
	 * See `encodeProposalHeader` for more details.
	 */
	readonly header: ProposalHeader;
	/**
	 * 20 bytes Hex-encoded string.
	 */
	readonly removeTokenAddress: string;
}

export function encodeTokenAddProposal(proposal: TokenAddProposal): Uint8Array {
	const header = encodeProposalHeader(proposal.header);
	const tokenAddProposal = new Uint8Array(40 + 20);
	tokenAddProposal.set(header, 0); // 0 -> 40
	const address = hexToU8a(proposal.newTokenAddress).slice(0, 20);
	tokenAddProposal.set(address, 40); // 40 -> 60
	return tokenAddProposal;
}

export function decodeTokenAddProposal(data: Uint8Array): TokenAddProposal {
	const header = decodeProposalHeader(data.slice(0, 40)); // 0 -> 40
	const newTokenAddress = u8aToHex(data.slice(40, 60)); // 40 -> 60
	return {
		header,
		newTokenAddress
	};
}

export function encodeTokenRemoveProposal(proposal: TokenRemoveProposal): Uint8Array {
	const header = encodeProposalHeader(proposal.header);
	const tokenRemoveProposal = new Uint8Array(40 + 20);
	tokenRemoveProposal.set(header, 0); // 0 -> 40
	const address = hexToU8a(proposal.removeTokenAddress).slice(0, 20);
	tokenRemoveProposal.set(address, 40); // 40 -> 60
	return tokenRemoveProposal;
}

export function decodeTokenRemoveProposal(data: Uint8Array): TokenRemoveProposal {
	const header = decodeProposalHeader(data.slice(0, 40)); // 0 -> 40
	const removeTokenAddress = u8aToHex(data.slice(40, 60)); // 40 -> 60
	return {
		header,
		removeTokenAddress,
	};
}

export interface WrappingFeeUpdateProposal {
	/**
	 * The Wrapping Fee Update Proposal Header.
	 * This is the first 40 bytes of the proposal.
	 * See `encodeProposalHeader` for more details.
	 */
	readonly header: ProposalHeader;
	/**
	 * 1 byte Hex-encoded string.
	 */
	readonly newFee: string;
}

export interface WrappingFeeUpdateProposal {
	/**
	 * The Wrapping Fee Update Proposal Header.
	 * This is the first 40 bytes of the proposal.
	 * See `encodeProposalHeader` for more details.
	 */
	readonly header: ProposalHeader;
	/**
	 * 1 byte Hex-encoded string.
	 */
	readonly newFee: string;
}

export function encodeWrappingFeeUpdateProposal(proposal: WrappingFeeUpdateProposal): Uint8Array {
	const header = encodeProposalHeader(proposal.header);
	const wrappingFeeUpdateProposal = new Uint8Array(40 + 1);
	wrappingFeeUpdateProposal.set(header, 0); // 0 -> 40
	const newFee = hexToU8a(proposal.newFee).slice(0, 1);
	wrappingFeeUpdateProposal.set(newFee, 40); // 40 -> 41
	return wrappingFeeUpdateProposal;
}

export function decodeWrappingFeeUpdateProposal(data: Uint8Array): WrappingFeeUpdateProposal {
	const header = decodeProposalHeader(data.slice(0, 40)); // 0 -> 40
	const newFee = u8aToHex(data.slice(40, 41)); // 40 -> 41
	return {
		header,
		newFee
	};
}


export interface MinWithdrawalLimitProposal {
	/**
	 * The Wrapping Fee Update Proposal Header.
	 * This is the first 40 bytes of the proposal.
	 * See `encodeProposalHeader` for more details.
	 */
	readonly header: ProposalHeader;
	/**
	 * 32 bytes Hex-encoded string.
	 */
	readonly minWithdrawalLimitBytes: string;
}

export function encodeMinWithdrawalLimitProposal(proposal: MinWithdrawalLimitProposal): Uint8Array {
	const header = encodeProposalHeader(proposal.header);
	const minWithdrawalLimitProposal = new Uint8Array(40 + 32);
	minWithdrawalLimitProposal.set(header, 0); // 0 -> 40
	const newFee = hexToU8a(proposal.minWithdrawalLimitBytes, 256).slice(0, 32);
	minWithdrawalLimitProposal.set(newFee, 40); // 40 -> 72
	return minWithdrawalLimitProposal;
}

export function decodeMinWithDrawalLimitProposal(data: Uint8Array): MinWithdrawalLimitProposal {
	const header = decodeProposalHeader(data.slice(0, 40)); // 0 -> 40
	const minWithdrawalLimitBytes = u8aToHex(data.slice(40, 72)); // 40 -> 72
	return {
		header,
		minWithdrawalLimitBytes
	};
}

export interface MaxDepositLimitProposal {
	/**
	 * The Wrapping Fee Update Proposal Header.
	 * This is the first 40 bytes of the proposal.
	 * See `encodeProposalHeader` for more details.
	 */
	readonly header: ProposalHeader;
	/**
	 * 32 bytes Hex-encoded string.
	 */
	readonly maxDepositLimitBytes: string;
}

export function encodeMaxDepositLimitProposal(proposal: MaxDepositLimitProposal): Uint8Array {
	const header = encodeProposalHeader(proposal.header);
	const maxDepositLimitProposal = new Uint8Array(40 + 32);
	maxDepositLimitProposal.set(header, 0); // 0 -> 40
	const newFee = hexToU8a(proposal.maxDepositLimitBytes, 256).slice(0, 32);
	maxDepositLimitProposal.set(newFee, 40); // 40 -> 72
	return maxDepositLimitProposal;
}

export function decodeMaxDepositLimitProposal(data: Uint8Array): MaxDepositLimitProposal {
	const header = decodeProposalHeader(data.slice(0, 40)); // 0 -> 40
	const maxDepositLimitBytes = u8aToHex(data.slice(40, 72)); // 40 -> 72
	return {
		header,
		maxDepositLimitBytes
	};
}

export interface ResourceIdUpdateProposal {
	/**
	 * The ResourceIdUpdateProposal Header.
	* This is the first 40 bytes of the proposal.
	* See `encodeProposalHeader` for more details.
	*/
	readonly header: ProposalHeader;
	/** 
	 * 32 bytes Hex-encoded string.
	 */
	readonly newResourceId: string;
	/**
	 * 20 bytes Hex-encoded string.
	 */
	readonly handlerAddress: string;
	/**
	 * 20 bytes Hex-encoded string.
	 */
	readonly executionAddress: string;
}
export function encodeResourceIdUpdateProposal(proposal: ResourceIdUpdateProposal): Uint8Array {
	const header = encodeProposalHeader(proposal.header);
	const resourceIdUpdateProposal = new Uint8Array(40 + 32 + 20 + 20);
	resourceIdUpdateProposal.set(header, 0); // 0 -> 40
	const newResourceId = hexToU8a(proposal.newResourceId).slice(0, 32);
	const handlerAddress = hexToU8a(proposal.handlerAddress).slice(0, 20);
	const executionAddress = hexToU8a(proposal.executionAddress).slice(0, 20);
	resourceIdUpdateProposal.set(newResourceId, 40); // 40 -> 72
	resourceIdUpdateProposal.set(handlerAddress, 72); // 72 -> 92
	resourceIdUpdateProposal.set(executionAddress, 92); // 72 -> 112
	return resourceIdUpdateProposal;
}
export function decodeResourceIdUpdateProposal(data: Uint8Array): ResourceIdUpdateProposal {
	const header = decodeProposalHeader(data.slice(0, 40)); // 0 -> 40
	const newResourceId = u8aToHex(data.slice(40, 72)); // 40 -> 72
	const handlerAddress = u8aToHex(data.slice(72, 92)); // 72 -> 92
	const executionAddress = u8aToHex(data.slice(92, 112)); // 92 -> 112
	return {
		header,
		newResourceId,
		handlerAddress,
		executionAddress
	};
}

export interface SetTreasuryHandlerProposal {
	/**
	 * The Token Add Proposal Header.
	 * This is the first 40 bytes of the proposal.
	 * See `encodeProposalHeader` for more details.
	 */
	readonly header: ProposalHeader;
	/**
	 * 20 bytes Hex-encoded string.
	 */
	readonly newTreasuryHandler: string;
}

export function encodeSetTreasuryHandlerProposal(proposal: SetTreasuryHandlerProposal): Uint8Array {
	const header = encodeProposalHeader(proposal.header);
	const setTreasuryHandlerProposal = new Uint8Array(40 + 20);
	setTreasuryHandlerProposal.set(header, 0); // 0 -> 40
	const address = hexToU8a(proposal.newTreasuryHandler).slice(0, 20);
	setTreasuryHandlerProposal.set(address, 40); // 40 -> 60
	return setTreasuryHandlerProposal;
}

export function decodeSetTreasuryHandlerProposal(data: Uint8Array): SetTreasuryHandlerProposal {
	const header = decodeProposalHeader(data.slice(0, 40)); // 0 -> 40
	const newTreasuryHandler = u8aToHex(data.slice(40, 60)); // 40 -> 60
	return {
		header,
		newTreasuryHandler
	};
}

export interface SetVerifierProposal {
	/**
	 * The Token Add Proposal Header.
	 * This is the first 40 bytes of the proposal.
	 * See `encodeProposalHeader` for more details.
	 */
	readonly header: ProposalHeader;
	/**
	 * 20 bytes Hex-encoded string.
	 */
	readonly newVerifier: string;
}

export function encodeSetVerifierProposal(proposal: SetVerifierProposal): Uint8Array {
	const header = encodeProposalHeader(proposal.header);
	const setVerifierProposal = new Uint8Array(40 + 20);
	setVerifierProposal.set(header, 0); // 0 -> 40
	const address = hexToU8a(proposal.newVerifier).slice(0, 20);
	setVerifierProposal.set(address, 40); // 40 -> 60
	return setVerifierProposal;
}

export function decodeSetVerifierProposal(data: Uint8Array): SetVerifierProposal {
	const header = decodeProposalHeader(data.slice(0, 40)); // 0 -> 40
	const newVerifier = u8aToHex(data.slice(40, 60)); // 40 -> 60
	return {
		header,
		newVerifier
	};
}

export interface FeeRecipientUpdateProposal {
	/**
	 * The Token Add Proposal Header.
	 * This is the first 40 bytes of the proposal.
	 * See `encodeProposalHeader` for more details.
	 */
	readonly header: ProposalHeader;
	/**
	 * 20 bytes Hex-encoded string.
	 */
	readonly newFeeRecipient: string;
}

export function encodeFeeRecipientUpdateProposal(proposal: FeeRecipientUpdateProposal): Uint8Array {
	const header = encodeProposalHeader(proposal.header);
	const feeRecipientUpdateProposal = new Uint8Array(40 + 20);
	feeRecipientUpdateProposal.set(header, 0); // 0 -> 40
	const address = hexToU8a(proposal.newFeeRecipient).slice(0, 20);
	feeRecipientUpdateProposal.set(address, 40); // 40 -> 60
	return feeRecipientUpdateProposal;
}

export function decodeFeeRecipientUpdateProposal(data: Uint8Array): FeeRecipientUpdateProposal {
	const header = decodeProposalHeader(data.slice(0, 40)); // 0 -> 40
	const newFeeRecipient = u8aToHex(data.slice(40, 60)); // 40 -> 60
	return {
		header,
		newFeeRecipient
	};
}

export interface RescueTokensProposal {
	/**
	 * The Rescue Token Proposals Header.
	**/
	readonly header: ProposalHeader;

	/** 
	 * 20 bytes Hex-encoded string.
	 */
	readonly tokenAddress: string;
	/**
	 * 20 bytes Hex-encoded string.
	 */
	readonly toAddress: string;
	/**
	 * 32 bytes Hex-encoded string.
	 */
	readonly amount: string;
}

export function encodeRescueTokensProposal(proposal: RescueTokensProposal): Uint8Array {
	const header = encodeProposalHeader(proposal.header);
	const rescueTokensProposal = new Uint8Array(40 + 20 + 20 + 32);
	rescueTokensProposal.set(header, 0); // 0 -> 40
	const tokenAddress = hexToU8a(proposal.tokenAddress).slice(0, 20);
	const toAddress = hexToU8a(proposal.toAddress).slice(0, 20);
	const amount = hexToU8a(proposal.amount, 256).slice(0, 32);

	rescueTokensProposal.set(tokenAddress, 40); // 40 -> 60
	rescueTokensProposal.set(toAddress, 60); // 60 -> 80
	rescueTokensProposal.set(amount, 80); // 80 -> 112

	return rescueTokensProposal;
}

export function decodeRescueTokensProposal(data: Uint8Array): RescueTokensProposal {
	const header = decodeProposalHeader(data.slice(0, 40)); // 0 -> 40
	const tokenAddress = u8aToHex(data.slice(40, 60)); // 40 -> 60
	const toAddress = u8aToHex(data.slice(60, 80)); // 60 -> 80
	const amount = u8aToHex(data.slice(80, 112)); // 80 -> 112
	return {
		header,
		tokenAddress,
		toAddress,
		amount
	}
}

/**
 * A ResourceID is a 32 bytes hex-encoded string of the following format:
 * - 26 bytes of the `anchorHandlerContractAddress` which is usually is just 20 bytes, but we pad it with zeros
 * to make it 26 bytes.
 * - 2 bytes of the `chainIdType` encoded as the last 2 bytes just before the `chainId`.
 * - 4 bytes of the `chainId` which is the last 4 bytes.
 */
export function makeResourceId(addr: string, chainIdType: ChainIdType, chainId: number): string {
	const rId = new Uint8Array(32);
	const address = hexToU8a(addr).slice(0, 20);
	rId.set(address, 6); // 6 -> 26
	const view = new DataView(rId.buffer);
	view.setUint16(26, chainIdType, BE); // 26 -> 28
	view.setUint32(28, chainId, BE); // 28 -> 32
	return u8aToHex(rId);
}

function _testEncodeDecode() {
	const anchorHandlerAddress = '0xaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa';
	const chainId = 0xcafe;
	const chainIdType = ChainIdType.EVM;
	const resourceId = makeResourceId(anchorHandlerAddress, chainIdType, chainId);
	const functionSignature = '0xdeadbeef';
	const nonce = 0xdad;
	const header: ProposalHeader = {
		resourceId,
		functionSignature,
		nonce,
	};
	const srcChainId = 0xbabe;
	const lastLeafIndex = 0xfeed;
	const merkleRoot = '0xcccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc';
	const updateProposal: AnchorUpdateProposal = {
		header,
		chainIdType,
		srcChainId,
		lastLeafIndex,
		merkleRoot,
	};
	const headerEncoded = encodeProposalHeader(header);
	const headerDecoded = decodeProposalHeader(headerEncoded);
	assert(headerDecoded.resourceId === resourceId, 'resourceId');
	assert(headerDecoded.functionSignature === functionSignature, 'functionSignature');
	assert(headerDecoded.nonce === nonce, 'nonce');

	const updateProposalEncoded = encodeUpdateAnchorProposal(updateProposal);
	const updateProposalDecoded = decodeUpdateAnchorProposal(updateProposalEncoded);
	assert(updateProposalDecoded.header.resourceId === resourceId, 'resourceId');
	assert(updateProposalDecoded.header.functionSignature === functionSignature, 'functionSignature');
	assert(updateProposalDecoded.header.nonce === nonce, 'nonce');
	assert(updateProposalDecoded.srcChainId === srcChainId, 'srcChainId');
	assert(updateProposalDecoded.lastLeafIndex === lastLeafIndex, 'lastLeafIndex');
	assert(updateProposalDecoded.merkleRoot === merkleRoot, 'merkleRoot');
}

export const resourceId = makeResourceId(
	'0xe69a847cd5bc0c9480ada0b339d7f0a8cac2b667',
	ChainIdType.EVM,
	5002
);

export const newResourceId = makeResourceId(
	'0xe69a847cd5bc0c9480ada0b339d7f0a8cac2b660',
	ChainIdType.EVM,
	5002
);

export async function signAndSendUtil(api: ApiPromise, proposalCall: any, alice: KeyringPair) {
	const unsub = await api.tx.sudo.sudo(proposalCall).signAndSend(alice, ({events = [], status}) => {
		console.log(`Current status is: ${status.type}`);

		if (status.isFinalized) {
			console.log(`Transaction included at blockHash ${status.asFinalized}`);

			events.forEach(({phase, event: {data, method, section}}) => {
				console.log(`\t' ${phase}: ${section}.${method}:: ${data}`);
			});

			unsub();
		}
	});
}

export async function unsubSignedPropsUtil(api: ApiPromise, chainIdType: any, dkgPubKey: any, proposalType: any, propHash: any) {
	return await api.query.dKGProposalHandler.signedProposals(
		chainIdType,
		proposalType,
		(res: any) => {
			if (res) {
				const parsedResult = JSON.parse(JSON.stringify(res));
				console.log(`Signed ${JSON.stringify(proposalType)} prop: ${JSON.stringify(parsedResult)}`);

				if (parsedResult) {
					const sig = parsedResult.signed.signature;
					console.log(`Signature: ${sig}`);

					const recoveredPubKey = ethers.utils.recoverPublicKey(propHash, sig).substr(2);
					console.log(`Recovered public key: ${recoveredPubKey}`);
					console.log(`DKG public key: ${dkgPubKey}`);

					assert(recoveredPubKey == dkgPubKey, 'Public keys should match');
					if (recoveredPubKey == dkgPubKey) {
						console.log(`Public keys match`);
						process.exit(0);
					} else {
						console.error(`Public keys do not match`);
						process.exit(-1);
					}
				}
			}
		}
	);
}

export async function registerResourceId(api: ApiPromise) {
	// quick check if the resourceId is already registered
	const res = await api.query.dKGProposals.resources(resourceId);
	const val = new Option(api.registry, Bytes, res);
	if (val.isSome) {
		console.log(`Resource id ${resourceId} is already registered, skipping`);
		return;
	}
	const keyring = new Keyring({type: 'sr25519'});
	const alice = keyring.addFromUri('//Alice');

	const call = api.tx.dKGProposals.setResource(resourceId, '0x00');
	console.log('Registering resource id');
	const unsub = await api.tx.sudo.sudo(call).signAndSend(alice, ({events = [], status}) => {
		console.log(`Current status is: ${status.type}`);

		if (status.isFinalized) {
			console.log(`Transaction included at blockHash ${status.asFinalized}`);

			events.forEach(({phase, event: {data, method, section}}) => {
				console.log(`\t' ${phase}: ${section}.${method}:: ${data}`);
			});

			unsub();
		}
	});
}