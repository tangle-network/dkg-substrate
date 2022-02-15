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
 * - target function signature (4 bytes). In the case of Substrate,
 * should be 4 bytes of zeroes.
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
 * A ResourceID is a 32 bytes hex-encoded string of the following format:
 * - 26 bytes of the `anchorHandlerContractAddress` which is usually is just 20 bytes, but we pad it with zeros
 * to make it 26 bytes.
 * - 2 bytes of the `chainIdType` encoded as the last 2 bytes just before the `chainId`.
 * - 4 bytes of the `chainId` which is the last 4 bytes.
 */
export function makeResourceId(chainIdType: ChainIdType, chainId: number): string {
	const rId = new Uint8Array(32);
	const view = new DataView(rId.buffer);
	view.setUint16(26, chainIdType, BE); // 26 -> 28
	view.setUint32(28, chainId, BE); // 28 -> 32
	return u8aToHex(rId);
}

export interface SubstrateProposal {
	/**
	 * The Wrapping Fee Update Proposal Header.
	 * This is the first 40 bytes of the proposal.
	 * See `encodeProposalHeader` for more details.
	 */
	readonly header: ProposalHeader;
	/**
	 * The encoded call
	 */
	readonly encodedCall: string;
}

export function encodeSubstrateProposal(proposal: SubstrateProposal, lengthOfCall: number): Uint8Array {
	const header = encodeProposalHeader(proposal.header);
	const substrateProposal = new Uint8Array(40 + lengthOfCall); 
	substrateProposal.set(header, 0); // 0 -> 40
	const encodedCall = hexToU8a(proposal.encodedCall).slice(0,);
	substrateProposal.set(encodedCall, 40); // 40 -> END
	return substrateProposal;
}

export function decodeSubstrateProposal(data: Uint8Array): SubstrateProposal {
	const header = decodeProposalHeader(data.slice(0, 40)); // 0 -> 40
	const encodedCall = u8aToHex(data.slice(40,)); // 40 -> 60
	return {
		header,
		encodedCall
	};
}


function _testEncodeDecode() {
	// const anchorHandlerAddress = '0xaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa';
	// const chainId = 0xcafe;
	// const chainIdType = ChainIdType.SUBSTRATE;
	// const resourceId = makeResourceId(chainIdType, chainId);
	// const functionSignature = '0x00000000';
	// const nonce = 0xdad;
	// const header: ProposalHeader = {
	// 	resourceId,
	// 	functionSignature,
	// 	nonce,
	// };
	// const srcChainId = 0xbabe;
	// const lastLeafIndex = 0xfeed;
	// const merkleRoot = '0xcccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc';
	// const updateProposal: AnchorUpdateProposal = {
	// 	header,
	// 	srcChainId,
	// 	lastLeafIndex,
	// 	merkleRoot,
	// };
	// const headerEncoded = encodeProposalHeader(header);
	// const headerDecoded = decodeProposalHeader(headerEncoded);
	// assert(headerDecoded.resourceId === resourceId, 'resourceId');
	// assert(headerDecoded.functionSignature === functionSignature, 'functionSignature');
	// assert(headerDecoded.nonce === nonce, 'nonce');

	// const updateProposalEncoded = encodeUpdateAnchorProposal(updateProposal);
	// const updateProposalDecoded = decodeUpdateAnchorProposal(updateProposalEncoded);
	// assert(updateProposalDecoded.header.resourceId === resourceId, 'resourceId');
	// assert(updateProposalDecoded.header.functionSignature === functionSignature, 'functionSignature');
	// assert(updateProposalDecoded.header.nonce === nonce, 'nonce');
	// assert(updateProposalDecoded.srcChainId === srcChainId, 'srcChainId');
	// assert(updateProposalDecoded.lastLeafIndex === lastLeafIndex, 'lastLeafIndex');
	// assert(updateProposalDecoded.merkleRoot === merkleRoot, 'merkleRoot');
}
