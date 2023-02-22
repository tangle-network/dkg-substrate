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
import '@webb-tools/types';
import { ApiPromise, Keyring } from '@polkadot/api';
import { SubmittableExtrinsic } from '@polkadot/api/types';
import child from 'child_process';
import { ECPair } from 'ecpair';
import { ethers } from 'ethers';

export const ALICE = '5GrwvaEF5zXb26Fz9rcQpDWS57CtERHpNehXCPcNoHGKutQY';

export const endpoint = 'ws://127.0.0.1:9944';

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

export const waitNfinalizedBlocks = async function (
	api: ApiPromise,
	n: number,
	timeout: number
) {
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
	{ delayBetweenBlocks }: { delayBetweenBlocks?: number } = {
		delayBetweenBlocks: 5,
	}
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
	{ delayBetweenBlocks }: { delayBetweenBlocks?: number } = {
		delayBetweenBlocks: 0,
	}
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

// a global variable to check if the node is already running or not.
// to avoid running multiple nodes with the same authority at the same time.
const __NODE_STATE: {
	[authorityId: string]: {
		process: child.ChildProcess | null;
		isRunning: boolean;
	};
} = {
	alice: { isRunning: false, process: null },
	bob: { isRunning: false, process: null },
	charlie: { isRunning: false, process: null },
	dave: { isRunning: false, process: null },
	eve: { isRunning: false, process: null },
	ferdie: { isRunning: false, process: null },
};
export function startStandaloneNode(
	authority: 'alice' | 'bob' | 'charlie' | 'dave' | 'eve' | 'ferdie',
	options: { tmp: boolean; printLogs: boolean, chain: string } = {
		tmp: true,
		printLogs: false,
		chain: "local"
	}
): child.ChildProcess {
	if (__NODE_STATE[authority].isRunning) {
		return __NODE_STATE[authority].process!;
	}
	const gitRoot = child
		.execSync('git rev-parse --show-toplevel')
		.toString()
		.trim();
	const nodePath = `${gitRoot}/target/release/dkg-standalone-node`;
	const ports = {
		alice: { ws: 9944, http: 9933, p2p: 30333 },
		bob: { ws: 9945, http: 9934, p2p: 30334 },
		charlie: { ws: 9946, http: 9935, p2p: 30335 },
		dave: { ws: 9947, http: 9936, p2p: 30336 },
		eve: { ws: 9948, http: 9937, p2p: 30337 },
		ferdie: { ws: 9949, http: 9938, p2p: 30338 },
	};
	const proc = child.spawn(
		nodePath,
		[
			`--${authority}`,
			options.printLogs ? '-linfo' : '-lerror',
			options.tmp ? '--tmp' : '',
			`--chain=${options.chain}`,
			`--ws-port=${ports[authority].ws}`,
			`--rpc-port=${ports[authority].http}`,
			`--port=${ports[authority].p2p}`,
			...(authority == 'alice'
				? [
						'--node-key',
						'0000000000000000000000000000000000000000000000000000000000000001',
				  ]
				: [
						'--bootnodes',
						`/ip4/127.0.0.1/tcp/${ports['alice'].p2p}/p2p/12D3KooWEyoppNCUx8Yx66oV9fJnriXwCcXwDDUA2kj6vnc6iDEp`,
				  ]),
			// only print logs from the alice node
			...(authority === 'alice' && options.printLogs
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
		],
		{
			cwd: gitRoot,
		}
	);

	__NODE_STATE[authority].isRunning = true;
	__NODE_STATE[authority].process = proc;

	if (options.printLogs) {
		proc.stdout.on('data', (data) => {
			console.log(`${authority}: ${data}`);
		});
		proc.stderr.on('data', (data) => {
			console.error(`${authority}: ${data}`);
		});
	}

	proc.on('close', (code) => {
		__NODE_STATE[authority].isRunning = false;
		__NODE_STATE[authority].process = null;
		console.log(`${authority} node exited with code ${code}`);
	});
	return proc;
}

/**
 * Waits until a new session is started.
 */
export async function waitForTheNextSession(api: ApiPromise): Promise<void> {
	return waitForEvent(api, 'session', 'NewSession');
}

export async function waitForTheNextDkgPublicKey(
	api: ApiPromise
): Promise<void> {
	return waitForEvent(api, 'dkg', 'NextPublicKeySubmitted');
}

export async function waitForTheNextDkgPublicKeySignature(
	api: ApiPromise
): Promise<void> {
	return waitForEvent(api, 'dkg', 'NextPublicKeySignatureSubmitted');
}

export async function waitForPublicKeyToChange(api: ApiPromise): Promise<void> {
	return waitForEvent(api, 'dkg', 'PublicKeyChanged');
}

export async function waitForPublicKeySignatureToChange(
	api: ApiPromise
): Promise<void> {
	return waitForEvent(api, 'dkg', 'PublicKeySignatureChanged');
}

export async function waitForEvent(
	api: ApiPromise,
	pallet: string,
	eventVariant: string,
	dataQuery?: { key: string }
): Promise<void> {
	return new Promise(async (resolve, _rej) => {
		// Subscribe to system events via storage
		const unsub = await api.query.system.events((events) => {
			const handleUnsub = () => {
				// Unsubscribe from the storage
				unsub();
				// Resolve the promise
				resolve(void 0);
			};

			// Loop through the Vec<EventRecord>
			events.forEach((record) => {
				const { event } = record;
				if (event.section === pallet && event.method === eventVariant) {
					console.log(
						`Event (${event.section}.${event.method}) =>`,
						event.data.toJSON()
					);
					if (dataQuery) {
						event.data.forEach((value, index) => {
							const jsonData = value.toJSON();
							if (jsonData instanceof Object) {
								Object.keys(jsonData).map((key) => {
									if (key === dataQuery.key) {
										handleUnsub();
									}
								});
							}
						});
					} else {
						handleUnsub();
					}
				}
			});
		});
	});
}

/**
 * Wait until the DKG Public Key is available and return it uncompressed without the `04` prefix byte.
 * @param api the current connected api.
 */
export async function waitUntilDKGPublicKeyStoredOnChain(
	api: ApiPromise
): Promise<`0x${string}`> {
	return new Promise(async (resolve, _reject) => {
		const unsubscribe = await api.rpc.chain.subscribeNewHeads(() => {
			fetchDkgPublicKey(api).then((value) => {
				if (value) {
					unsubscribe();
					resolve(value);
				}
			});
		});
	});
}

/**
 * Fetch DKG Public Key and return it **uncompressed** without the `04` prefix byte.
 * returns `null` if the key is not yet available.
 * @param api the current connected api.
 */
export async function fetchDkgPublicKey(
	api: ApiPromise
): Promise<`0x${string}` | null> {
	const res = await api.query.dkg.dkgPublicKey();
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
export async function fetchDkgPublicKeySignature(
	api: ApiPromise
): Promise<`0x${string}` | null> {
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
	const call = api.tx.dkg.manualRefresh();
	return sudoTx(api, call);
}

export async function sudoTx(
	api: ApiPromise,
	call: SubmittableExtrinsic<'promise'>
): Promise<void> {
	const keyring = new Keyring({ type: 'sr25519' });
	const alice = keyring.addFromUri('//Alice');
	return new Promise(async (resolve, _reject) => {
		const unsub = await api.tx.sudo
			.sudo(call)
			.signAndSend(alice, ({ status }) => {
				if (status.isFinalized) {
					unsub();
					resolve();
				}
			});
	});
}

export async function triggerDkgManuaIncrementNonce(
	api: ApiPromise
): Promise<void> {
	const keyring = new Keyring({ type: 'sr25519' });
	const alice = keyring.addFromUri('//Alice');
	const call = api.tx.dkg.manualIncrementNonce();
	const unsub = await api.tx.sudo
		.sudo(call)
		.signAndSend(alice, ({ status }) => {
			if (status.isFinalized) {
				unsub();
			}
		});
}

export function ethAddressFromUncompressedPublicKey(
	publicKey: `0x${string}`
): `0x${string}` {
	const pubKeyHash = ethers.utils.keccak256(publicKey); // we hash it.
	const address = ethers.utils.getAddress(`0x${pubKeyHash.slice(-40)}`); // take the last 20 bytes and convert it to an address.
	return address as `0x${string}`;
}
