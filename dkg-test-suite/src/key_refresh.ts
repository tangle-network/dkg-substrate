import { ApiPromise, Keyring } from "@polkadot/api";
import { provider, wait_n_finalized_blocks } from "./utils";
import { assert } from "@polkadot/util";

async function dkg_refresh() {
	const api = await ApiPromise.create({ provider });

	const keyring = new Keyring({ type: "sr25519" });
	const alice = keyring.addFromUri("//Alice");
	const charliesStash = keyring.addFromUri("//Charlie//stash");

	// Session 0
	console.log("Session 0");

	let forceNewEra = api.tx.staking.forceNewEraAlways();

	const forceNewEraAlwayCall = api.tx.sudo.sudo({
		callIndex: forceNewEra.callIndex,
		args: forceNewEra.args,
	});

	await forceNewEraAlwayCall.signAndSend(alice);

	let call = api.tx.staking.chill();

	await call.signAndSend(charliesStash);

	await wait_n_finalized_blocks(api, 10, 120);

	const nextKey = await api.query.dkg.nextDKGPublicKey();

	const dkgPublicKey = await api.query.dkg.dKGPublicKey();

	assert(JSON.parse(nextKey.toString() as any)[1], "Next public key should be on chain");

	assert(JSON.parse(dkgPublicKey.toString() as any)[1], "DKG public key should be on chain");

	await wait_n_finalized_blocks(api, 37, 37 * 12);

	const nextPublicKeySignature = await api.query.dkg.nextPublicKeySignature();

	assert(
		JSON.parse(nextPublicKeySignature.toString() as any)[1],
		"Next public key signature should be on chain"
	);

	// remove invulnerables
	const invulnerableCall = api.tx.staking.setInvulnerables([]);
	const setInvulnerables = api.tx.sudo.sudo({
		callIndex: invulnerableCall.callIndex,
		args: invulnerableCall.args,
	});

	await setInvulnerables.signAndSend(alice);

	await wait_n_finalized_blocks(api, 5, 72);

	// Session 1

	console.log("Session 1");

	await wait_n_finalized_blocks(api, 15, 180);

	// New session has started but authority set or queued authority set has not changed

	const nextKey1 = await api.query.dkg.nextDKGPublicKey();

	assert(
		JSON.parse(nextKey.toString() as any)[1] == JSON.parse(nextKey1.toString() as any)[1],
		"Next public key should be same"
	);

	await wait_n_finalized_blocks(api, 35, 540);

	const nextPublicKeySignature1 = await api.query.dkg.nextPublicKeySignature();

	assert(
		JSON.parse(nextPublicKeySignature.toString() as any)[1] ==
			JSON.parse(nextPublicKeySignature1.toString() as any)[1],
		"Next public key signature should be same"
	);

	await wait_n_finalized_blocks(api, 35, 35 * 12);

	// Session 2

	console.log("Session 2");

	const nextPublicKeySignature2 = await api.query.dkg.nextPublicKeySignature();

	// Queued authority set changed so the key signatures should change

	assert(
		JSON.parse(nextPublicKeySignature1.toString() as any)[1] !=
			JSON.parse(nextPublicKeySignature2.toString() as any)[1],
		"Next public key signature should be different"
	);

	call = api.tx.staking.validate({ commission: 0, blocked: false });

	await call.signAndSend(charliesStash);

	await wait_n_finalized_blocks(api, 50, 600);

	// Session 3

	console.log("Session 3");

	// A key refresh should have occured

	let dkgPubKeySignature = await api.query.dkg.dkgPublicKeySignature();

	assert(
		JSON.parse(dkgPubKeySignature.toString() as any)[1],
		"DKG public key signature should be set"
	);

	await wait_n_finalized_blocks(api, 100, 600);

	let usedSignatures = await api.query.dkg.usedSignatures();

	console.log(usedSignatures.toString());
}

// Run
dkg_refresh()
	.catch(console.error)
	.finally(() => process.exit());
