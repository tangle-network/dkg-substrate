import { ApiPromise } from '@polkadot/api';
import { provider, waitNfinalizedBlocks } from './utils';
import ora from 'ora';

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

function waitForTheNextSession(api: ApiPromise) {
	return new Promise(async (reolve, _) => {
		const currentBlock = await api.rpc.chain.getHeader();
		// Subscribe to system events via storage
		const unsub = await api.query.system.events((events) => {
			// Loop through the Vec<EventRecord>
			events.forEach((record) => {
				const { event } = record;
				if (event.section === 'session' && event.method === 'NewSession') {
					// Unsubscribe from the storage
					unsub();
					// Resolve the promise
					reolve(void 0);
				}
			});
		});
	});
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
