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
import { ApiPromise, WsProvider } from '@polkadot/api';
import { waitNfinalizedBlocks, waitForTheNextSession } from './utils';
import ora from 'ora';

const provider = new WsProvider('ws://127.0.0.1:9944');
async function testDkgRefresh() {
	const ROUNDS = 3;
	const api = await ApiPromise.create({ provider });
	// Helps us to store the keys to test if we seen this key before or not.
	const keys = new Map<string, number>();
	for (let i = 0; i < ROUNDS; i++) {
		const roundSpinner = ora({
			text: 'Starting..',
			prefixText: `Round ${i + 1}/${ROUNDS}`,
		}).start();
		roundSpinner.text = `Waiting for the next Session...`;
		await waitForTheNextSession(api);
		roundSpinner.text = `Fetching the next DKG public key...`;
		const { authorityId, publicKey } = await fetchPublicKey(api);
		// check if the public key is already in the map
		// panic if we see the same key more than once
		roundSpinner.text = `Public key: ${publicKey}`;
		if (keys.has(publicKey)) {
			roundSpinner.fail(`Public key: ${publicKey} seen more than once`);
			break;
		}
		// add the key to the map
		keys.set(publicKey, authorityId);
		roundSpinner.succeed();
		// sleep for 2 block
		await waitNfinalizedBlocks(api, 2, 30);
	}
}

async function fetchPublicKey(
	api: ApiPromise
): Promise<{ authorityId: number; publicKey: string }> {
	const dkgPublicKey = await api.query.dkg.dKGPublicKey();
	const json = dkgPublicKey.toJSON() as [number, string];
	return {
		authorityId: json[0],
		publicKey: json[1],
	};
}

testDkgRefresh()
	.then(() => {
		console.log('Done');
		process.exit(0);
	})
	.catch((err) => {
		console.error(err);
		process.exit(1);
	});
