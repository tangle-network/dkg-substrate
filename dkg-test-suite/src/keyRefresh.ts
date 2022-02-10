import { ApiPromise, Keyring, WsProvider } from '@polkadot/api';
import { waitNfinalizedBlocks } from './utils';
import { assert } from '@polkadot/util';

const provider = new WsProvider('ws://127.0.0.1:9944');

async function dkg_refresh() {
	const api = await ApiPromise.create({ provider });
	const keyring = new Keyring({ type: 'sr25519' });
	const alice = keyring.addFromUri('//Alice');
	const charliesStash = keyring.addFromUri('//Charlie//stash');

	// Session 0
	console.log('Session 0');
	let call = api.tx.staking.chill();
	await call.signAndSend(charliesStash);
	let forceNewEra = api.tx.staking.forceNewEraAlways();
	const forceNewEraAlwayCall = api.tx.sudo.sudo({
		callIndex: forceNewEra.callIndex,
		args: forceNewEra.args,
	});
	await forceNewEraAlwayCall.signAndSend(alice);
	await waitNfinalizedBlocks(api, 10, 120);
	const nextKey = await api.query.dkg.nextDKGPublicKey();
	const dkgPublicKey = await api.query.dkg.dKGPublicKey();
	assert(JSON.parse(nextKey.toString() as any)[1], 'Next public key should be on chain');
	assert(JSON.parse(dkgPublicKey.toString() as any)[1], 'DKG public key should be on chain');
	await waitNfinalizedBlocks(api, 37, 37 * 12);
	const nextPublicKeySignature = await api.query.dkg.nextPublicKeySignature();
	assert(
		JSON.parse(nextPublicKeySignature.toString() as any)[1],
		'Next public key signature should be on chain'
	);
	await waitNfinalizedBlocks(api, 5, 5 * 5);

	// Session 1
	console.log('Session 1');
	call = api.tx.staking.validate({ commission: 0, blocked: false });
	await call.signAndSend(charliesStash);
	await waitNfinalizedBlocks(api, 15, 15 * 5);
	// Queued authority should have changed because we chilled Charlie and forced a new era
	const nextKey1 = await api.query.dkg.nextDKGPublicKey();
	assert(
		JSON.parse(nextKey.toString() as any)[1] !== JSON.parse(nextKey1.toString() as any)[1],
		'Next public key should be different'
	);

	await waitNfinalizedBlocks(api, 150, 150 * 5);

	// Three Key refreshes should have occured by now and the length of used signatures should be two
	let usedSignatures = await api.query.dkg.usedSignatures();
	let parsedSignatures: string[] = (usedSignatures.toHuman() as string[]).slice(1);

	assert(parsedSignatures.length == 2, 'There should be two used signatures on chain');
}

// Run
dkg_refresh()
	.catch(console.error)
	.finally(() => process.exit());
