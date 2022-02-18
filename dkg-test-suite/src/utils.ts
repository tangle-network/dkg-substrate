import { ApiPromise, Keyring, WsProvider } from '@polkadot/api';
import { u8aToHex, hexToU8a, assert } from '@polkadot/util';
import child from 'child_process';
import { ECPair } from 'ecpair';
import { ethers } from 'ethers';

export const ALICE = '5GrwvaEF5zXb26Fz9rcQpDWS57CtERHpNehXCPcNoHGKutQY';

export const provider = new WsProvider('ws://127.0.0.1:9944');

export async function sleep(ms: number) {
	return new Promise((resolve) => setTimeout(resolve, ms));
}

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

/**
 * @description: fast forward {n} blocks from the current block number.
 */
export async function fastForward(
	api: ApiPromise,
	n: number,
	{ delayBetweenBlocks }: { delayBetweenBlocks?: number } = { delayBetweenBlocks: 5 }
): Promise<void> {
	for (let i = 0; i < n; i++) {
		const createEmpty = true;
		const finalize = true;
		await api.rpc.engine.createBlock(createEmpty, finalize);
		// sleep for delayBetweenBlocks milliseconds
		await new Promise((resolve) => setTimeout(resolve, delayBetweenBlocks));
	}
}

export async function fastForwardTo(
	api: ApiPromise,
	blockNumber: number,
	{ delayBetweenBlocks }: { delayBetweenBlocks?: number } = { delayBetweenBlocks: 0 }
): Promise<void> {
	const currentBlockNumber = await api.rpc.chain.getHeader();
	const diff = blockNumber - currentBlockNumber.number.toNumber();
	if (diff > 0) {
		await fastForward(api, diff, { delayBetweenBlocks });
	}
}

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

export function startStandaloneNode(
	authority: 'alice' | 'bob' | 'charlie',
	options: { tmp: boolean; printLogs: boolean } = { tmp: true, printLogs: false }
): child.ChildProcess {
	const gitRoot = child.execSync('git rev-parse --show-toplevel').toString().trim();
	const nodePath = `${gitRoot}/target/release/dkg-standalone-node`;
	const ports = {
		alice: { ws: 9944, http: 9933, p2p: 30333 },
		bob: { ws: 9945, http: 9934, p2p: 30334 },
		charlie: { ws: 9946, http: 9935, p2p: 30335 },
	};
	const proc = child.spawn(
		nodePath,
		[
			options.printLogs ? '-linfo' : '-lerror',
			options.tmp ? '--tmp' : '',
			`--ws-port=${ports[authority].ws}`,
			`--rpc-port=${ports[authority].http}`,
			`--port=${ports[authority].p2p}`,
			...(authority == 'alice'
				? ['--node-key', '0000000000000000000000000000000000000000000000000000000000000001']
				: [
						'--bootnodes',
						`/ip4/127.0.0.1/tcp/${ports['alice'].p2p}/p2p/12D3KooWEyoppNCUx8Yx66oV9fJnriXwCcXwDDUA2kj6vnc6iDEp`,
				  ]),
			// only print logs from the alice node
			...(authority === 'alice'
				? [
						'-ldkg=debug',
						'-ldkg_metadata=debug',
						'-lruntime::offchain=debug',
						'-ldkg_proposal_handler=debug',
						'--rpc-cors',
						'all',
						'--ws-external',
				  ]
				: []),
			`--${authority}`,
		],
		{
			cwd: gitRoot,
		}
	);

	proc.stdout.on('data', (data) => {
		process.stdout.write(data);
	});

	proc.stderr.on('data', (data) => {
		process.stdout.write(data);
	});

	return proc;
}

/**
 * Waits until a new session is started.
 */
export async function waitForTheNextSession(api: ApiPromise): Promise<void> {
	return waitForEvent(api, 'session', 'NewSession');
}

export async function waitForTheNextDkgPublicKey(api: ApiPromise): Promise<void> {
	return waitForEvent(api, 'dkg', 'NextPublicKeySubmitted');
}

export async function waitForTheNextDkgPublicKeySignature(api: ApiPromise): Promise<void> {
	return waitForEvent(api, 'dkg', 'NextPublicKeySignatureSubmitted');
}

export async function waitForPublicKeyToChange(api: ApiPromise): Promise<void> {
	return waitForEvent(api, 'dkg', 'PublicKeyChanged');
}

export async function waitForPublicKeySignatureToChange(api: ApiPromise): Promise<void> {
	return waitForEvent(api, 'dkg', 'PublicKeySignatureChanged');
}

export async function waitForEvent(
	api: ApiPromise,
	pallet: string,
	eventVariant: string
): Promise<void> {
	return new Promise(async (reolve, _) => {
		// Subscribe to system events via storage
		const unsub = await api.query.system.events((events) => {
			// Loop through the Vec<EventRecord>
			events.forEach((record) => {
				const { event } = record;
				// dkg.NextPublicKeySubmitted
				if (event.section === pallet && event.method === eventVariant) {
					// Unsubscribe from the storage
					unsub();
					// Resolve the promise
					reolve(void 0);
				}
			});
		});
	});
}

/**
 * Wait until the DKG Public Key is available and return it uncompressed without the `04` prefix byte.
 * @param api the current connected api.
 */
export async function waitUntilDKGPublicKeyStoredOnChain(api: ApiPromise): Promise<`0x${string}`> {
	return new Promise(async (resolve, _reject) => {
		const unsubscribe = await api.rpc.chain.subscribeNewHeads(async () => {
			const dkgKey = await fetchDkgPublicKey(api);
			if (dkgKey) {
				unsubscribe();
				resolve(dkgKey);
			}
		});
	});
}

/**
 * Fetch DKG Public Key and return it **uncompressed** without the `04` prefix byte.
 * returns `null` if the key is not yet available.
 * @param api the current connected api.
 */
export async function fetchDkgPublicKey(api: ApiPromise): Promise<`0x${string}` | null> {
	const res = await api.query.dkg.dKGPublicKey();
	const json = res.toJSON() as [number, string];
	if (json && json[1] !== '0x') {
		const key = json[1];
		const dkgPubKey = ECPair.fromPublicKey(Buffer.from(key.slice(2), 'hex'), {
			compressed: false,
		}).publicKey.toString('hex');
		// now we remove the `04` prefix byte and return it.
		return `0x${dkgPubKey.slice(2)}`;
	} else {
		return null;
	}
}

/**
 * Fetch DKG Public Key signature and return it.
 * returns `null` if the key is not yet available.
 * @param api the current connected api.
 */
export async function fetchDkgPublicKeySignature(api: ApiPromise): Promise<`0x${string}` | null> {
	const sig = await api.query.dkg.dKGPublicKeySignature();
	if (!sig.isEmpty) {
		return sig.toHex();
	} else {
		return null;
	}
}

export async function fetchDkgRefreshNonce(api: ApiPromise): Promise<number> {
	const nonce = await api.query.dkg.refreshNonce();
	return nonce.toJSON() as number;
}

export async function triggerDkgManualRefresh(api: ApiPromise): Promise<void> {
	const keyring = new Keyring({ type: 'sr25519' });
	const alice = keyring.addFromUri('//Alice');
	const call = api.tx.dkg.manualRefresh();
	const unsub = await api.tx.sudo.sudo(call).signAndSend(alice, ({ status }) => {
		if (status.isFinalized) {
			unsub();
		}
	});
}

export async function triggerDkgManuaIncrementNonce(api: ApiPromise): Promise<void> {
	const keyring = new Keyring({ type: 'sr25519' });
	const alice = keyring.addFromUri('//Alice');
	const call = api.tx.dkg.manualIncrementNonce();
	const unsub = await api.tx.sudo.sudo(call).signAndSend(alice, ({ status }) => {
		if (status.isFinalized) {
			unsub();
		}
	});
}

export function ethAddressFromUncompressedPublicKey(publicKey: `0x${string}`): `0x${string}` {
	const pubKeyHash = ethers.utils.keccak256(publicKey); // we hash it.
	const address = ethers.utils.getAddress(`0x${pubKeyHash.slice(-40)}`); // take the last 20 bytes and convert it to an address.
	return address as `0x${string}`;
}

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
		newTokenAddress,
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
		newFee,
	};
}

export interface VAnchorConfigurableLimitProposal {
	/**
	 * The Wrapping Fee Update Proposal Header.
	 * This is the first 40 bytes of the proposal.
	 * See `encodeProposalHeader` for more details.
	 */
	readonly header: ProposalHeader;
	/**
	 * 32 bytes Hex-encoded string.
	 */
	readonly min_withdrawal_limit_bytes: string;
}

export function encodeVAnchorConfigurableLimitProposal(
	proposal: VAnchorConfigurableLimitProposal
): Uint8Array {
	const header = encodeProposalHeader(proposal.header);
	const vAnchorConfigurableLimitProposal = new Uint8Array(40 + 32);
	vAnchorConfigurableLimitProposal.set(header, 0); // 0 -> 40
	const newFee = hexToU8a(proposal.min_withdrawal_limit_bytes).slice(0, 1);
	vAnchorConfigurableLimitProposal.set(newFee, 40); // 40 -> 41
	return vAnchorConfigurableLimitProposal;
}

export function decodeVAnchorConfigurableLimitProposal(
	data: Uint8Array
): VAnchorConfigurableLimitProposal {
	const header = decodeProposalHeader(data.slice(0, 40)); // 0 -> 40
	const min_withdrawal_limit_bytes = u8aToHex(data.slice(40, 72)); // 40 -> 72
	return {
		header,
		min_withdrawal_limit_bytes,
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
