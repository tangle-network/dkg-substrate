import { ApiPromise } from '@polkadot/api';
import { WsProvider } from '@polkadot/api';
import { u8aToHex, hexToU8a, assert } from '@polkadot/util';

export const ALICE = '5GrwvaEF5zXb26Fz9rcQpDWS57CtERHpNehXCPcNoHGKutQY';

export const provider = new WsProvider('ws://127.0.0.1:9944');

export const hexToBytes = function (hex: any) {
	for (var bytes = [], c = 0; c < hex.length; c += 2) {
		bytes.push(parseInt(hex.substr(c, 2), 16));
	}
	return bytes;
};

export const listenOneBlock = async function (api: ApiPromise) {
	const unsubscribe = await api.rpc.chain.subscribeNewHeads((header) => {
		console.log(`Chain is at block: #${header.hash}`);
		unsubscribe();
	});
};

export const waitNfinalizedBlocks = async function (api: ApiPromise, n: number, timeout: number) {
	return new Promise<void>(async (resolve, _reject) => {
		let count = 0;
		const unsubscribe = await api.rpc.chain.subscribeNewHeads((header) => {
			count++;
			if (count == n) {
				unsubscribe();
				resolve();
			}
			setTimeout(() => {
				unsubscribe();
				resolve();
			}, timeout * 1000);
		});
	});
};

export const printValidators = async function (api: ApiPromise) {
	const [{ nonce: accountNonce }, now, validators] = await Promise.all([
		api.query.system.account(ALICE),
		api.query.timestamp.now(),
		api.query.session.validators(),
	]);

	console.log(`accountNonce(${ALICE}) ${accountNonce}`);
	console.log(`last block timestamp ${now.toNumber()}`);

	if (validators && validators.length > 0) {
		const validatorBalances = await Promise.all(
			validators.map((authorityId) => api.query.system.account(authorityId))
		);

		console.log(
			'validators',
			validators.map((authorityId, index) => ({
				address: authorityId.toString(),
				balance: validatorBalances[index].data.free.toHuman(),
				nonce: validatorBalances[index].nonce.toHuman(),
			}))
		);
	}
};

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
 * Anchor Update Proposal is the next 40 bytes (after the header) and it contains the following information:
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
	const updateProposal = new Uint8Array(40 + 40);
	updateProposal.set(header, 0); // 0 -> 40
	const view = new DataView(updateProposal.buffer);
	view.setUint32(40, proposal.srcChainId, false); // 40 -> 44
	view.setUint32(44, proposal.lastLeafIndex, false); // 44 -> 48
	const merkleRoot = hexToU8a(proposal.merkleRoot).slice(0, 32);
	updateProposal.set(merkleRoot, 48); // 48 -> 80
	return updateProposal;
}

export function decodeUpdateAnchorProposal(data: Uint8Array): AnchorUpdateProposal {
	const header = decodeProposalHeader(data.slice(0, 40)); // 0 -> 40
	const srcChainId = new DataView(data.buffer).getUint32(40, false); // 40 -> 44
	const lastLeafIndex = new DataView(data.buffer).getUint32(44, false); // 44 -> 48
	const merkleRoot = u8aToHex(data.slice(48, 80)); // 48 -> 80
	return {
		header,
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
	const tokenAddProposal = new Uint8Array(40 + 20);
	tokenAddProposal.set(header, 0); // 0 -> 40
	const address = hexToU8a(proposal.removeTokenAddress).slice(0, 20);
	tokenAddProposal.set(address, 40); // 40 -> 60
	return tokenAddProposal;
}

export function decodeTokenRemoveProposal(data: Uint8Array): TokenRemoveProposal {
	const header = decodeProposalHeader(data.slice(0, 40)); // 0 -> 40
	const removeTokenAddress = u8aToHex(data.slice(40, 60)); // 40 -> 60
	return {
		header,
		removeTokenAddress
	};
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
