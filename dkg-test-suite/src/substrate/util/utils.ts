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
	readonly call: string;
}

export function encodeSubstrateProposal(proposal: SubstrateProposal, lengthOfCall: number): Uint8Array {
	const header = encodeProposalHeader(proposal.header);
	const substrateProposal = new Uint8Array(40 + lengthOfCall); 
	substrateProposal.set(header, 0); // 0 -> 40
	const hexifiedCall = new Buffer(proposal.call).toString('hex');
	const encodedCall = hexToU8a(hexifiedCall).slice(0,);
	substrateProposal.set(encodedCall, 40); // 40 -> END
	return substrateProposal;
}

export function decodeSubstrateProposal(data: Uint8Array): SubstrateProposal {
	const header = decodeProposalHeader(data.slice(0, 40)); // 0 -> 40
	const hexifiedCall = u8aToHex(data.slice(40,)); // 40 -> 60
	const decodedCall = new Buffer(hexifiedCall, 'hex').toString(); 
	return {
		header,
		call: decodedCall
	};
}

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
export const substratePalletResourceId = makeResourceId(
	ChainIdType.SUBSTRATE,
	5002,
);

export async function registerResourceId(api: ApiPromise) {
	// quick check if the resourceId is already registered
	const res = await api.query.dKGProposals.resources(substratePalletResourceId);
	const val = new Option(api.registry, Bytes, res);
	if (val.isSome) {
		console.log(`Resource id ${substratePalletResourceId} is already registered, skipping`);
		return;
	}
	const keyring = new Keyring({type: 'sr25519'});
	const alice = keyring.addFromUri('//Alice');

	const call = api.tx.dKGProposals.setResource(substratePalletResourceId, '0x00');
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
