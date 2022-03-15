
import 'jest-extended';
import {ACC1_PK, ACC2_PK, BLOCK_TIME, SECONDS} from '../src/constants';
import {
	ethAddressFromUncompressedPublicKey,
	provider,
	sleep, startStandaloneNode, waitNfinalizedBlocks,
	waitUntilDKGPublicKeyStoredOnChain,
} from '../src/utils';

import {jest} from "@jest/globals";
import {ApiPromise, Keyring} from "@polkadot/api";
import {ChildProcess} from "child_process";
import {LocalChain} from "../src/localEvm";
import {ethers} from "ethers";
import {Bridges} from "@webb-tools/protocol-solidity";
import {MintableToken} from "@webb-tools/tokens";

jest.setTimeout(100 * BLOCK_TIME); // 100 blocks

let polkadotApi: ApiPromise;
let aliceNode: ChildProcess;
let bobNode: ChildProcess;
let charlieNode: ChildProcess;
let daveNode: ChildProcess;

export let signatureBridge: Bridges.SignatureBridge;


async function executeAfter() {
	await polkadotApi.disconnect();
	aliceNode?.kill('SIGINT');
	bobNode?.kill('SIGINT');
	charlieNode?.kill('SIGINT');
	daveNode?.kill('SIGINT');
	await sleep(20 * SECONDS);
}


describe('Validator Node Test', () => {
	test('should be able to add or remove validator nodes', async () => {
		aliceNode = startStandaloneNode('alice', {tmp: true, printLogs: false});
		bobNode = startStandaloneNode('bob', {tmp: true, printLogs: true});
		charlieNode = startStandaloneNode('charlie', {tmp: true, printLogs: false});
		daveNode = startStandaloneNode('dave', {tmp: true, printLogs: true});

		polkadotApi = await ApiPromise.create({
			provider,
		});

		const nodeCount = polkadotApi.rpc.system.peers();

		//console.log(`peered nodes are ${nodeCount}`);
		//console.log(`peered nodes length is are ${nodeCount.length}`);
		//expect(nodeCount.length).toBe(2);
		const nextAuthorities = await polkadotApi.query.dkg.nextAuthorities();
		//const updatedNodeCount = await polkadotApi.rpc.system.peers();

		console.log(`authorities are ${nextAuthorities}`);

		// @ts-ignore
		console.log(`authority count is ${nextAuthorities.length}`);
		// @ts-ignore
		expect(nextAuthorities.length).toBe(3);

		const keyring = new Keyring({ type: 'sr25519' });
		const daveStash = keyring.addFromUri('//Dave//stash');
		const dave = keyring.addFromUri('//Dave');
		const alice = keyring.addFromUri('//Alice');

		//console.log(`Dave stash ${daveStash}`);
		console.log(`Forcing new Era`);

		let forceNewEra = polkadotApi.tx.staking.forceNewEraAlways();
		const forceNewEraAlwaysCall = polkadotApi.tx.sudo.sudo({
			callIndex: forceNewEra.callIndex,
			args: forceNewEra.args,
		});
		await forceNewEraAlwaysCall.signAndSend(alice);

		console.log(`Bond(stake) for Dave so as to become a validator`);

		// stake at first for dave
		let callStake = polkadotApi.tx.staking.bond(
			dave.publicKey,
			1000000,
			'Stash'
		);
		await callStake.signAndSend(daveStash, {
			tip: 100000
		});

		console.log(`Make Dave a validator`);
		// become validator
		let call = polkadotApi.tx.staking.validate({
			commission: 0,
			blocked: false
		});
		await call.signAndSend(daveStash, {
			tip: 20000000
		});

		await waitNfinalizedBlocks(polkadotApi, 10, 120);

		console.log(`Get validators count`);
		const nextAuthoritiesAfterValidatingDave = await polkadotApi.query.dkg.nextAuthorities();
		// @ts-ignore
		console.log(`authority count is now ${nextAuthoritiesAfterValidatingDave.length}`);
		// @ts-ignore
		expect(nextAuthoritiesAfterValidatingDave.length).toBe(4);

		let callChill = polkadotApi.tx.staking.chill();
		await callChill.signAndSend(daveStash);
		/*let forceNewEraAgain = polkadotApi.tx.staking.forceNewEraAlways();
		const forceNewEraAlwaysAgainCall = polkadotApi.tx.sudo.sudo({
			callIndex: forceNewEraAgain.callIndex,
			args: forceNewEraAgain.args,
		});
		await forceNewEraAlwaysAgainCall.signAndSend(alice);*/
		await waitNfinalizedBlocks(polkadotApi, 10, 120);

		const nextAuthoritiesAfterRemovingDave = await polkadotApi.query.dkg.nextAuthorities();
		// @ts-ignore
		console.log(`authority count is now ${nextAuthoritiesAfterValidatingDave.length}`);
		// @ts-ignore
		expect(nextAuthoritiesAfterRemovingDave.length).toBe(3);
	});

	afterAll(async () => {
		await executeAfter();
	});
});
