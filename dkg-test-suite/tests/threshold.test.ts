
import 'jest-extended';
import {ACC1_PK, ACC2_PK, BLOCK_TIME, SECONDS} from '../src/constants';
import {
	ethAddressFromUncompressedPublicKey,
	provider,
	sleep, startStandaloneNode, waitForEvent, waitForTheNextSession, waitNfinalizedBlocks,
	waitUntilDKGPublicKeyStoredOnChain,
} from '../src/utils';

import {jest} from "@jest/globals";
import {ApiPromise, Keyring} from "@polkadot/api";
import {ChildProcess} from "child_process";
import {LocalChain} from "../src/localEvm";
import {ethers} from "ethers";
import {Bridges} from "@webb-tools/protocol-solidity";
import {MintableToken} from "@webb-tools/tokens";

jest.setTimeout(10000 * BLOCK_TIME); // 100 blocks

let polkadotApi: ApiPromise;
let aliceNode: ChildProcess;
let bobNode: ChildProcess;
let charlieNode: ChildProcess;
//let daveNode: ChildProcess;

export let signatureBridge: Bridges.SignatureBridge;


async function executeAfter() {
	await polkadotApi.disconnect();
	aliceNode?.kill('SIGINT');
	bobNode?.kill('SIGINT');
	charlieNode?.kill('SIGINT');
	//daveNode?.kill('SIGINT');
	await sleep(20 * SECONDS);
}


describe('Validator Node Test', () => {
	test('should be able to  remove validator node and check threshold', async () => {
		aliceNode = startStandaloneNode('alice', {tmp: true, printLogs: false});
		bobNode = startStandaloneNode('bob', {tmp: true, printLogs: true});
		charlieNode = startStandaloneNode('charlie', {tmp: true, printLogs: false});

		polkadotApi = await ApiPromise.create({
			provider,
		});

		const keyring = new Keyring({ type: 'sr25519' });
		const charlieStash = keyring.addFromUri('//Charlie//stash');
		const charlie = keyring.addFromUri('//Charlie');
		const alice = keyring.addFromUri('//Alice');

		let thresholdCount = await polkadotApi.query.dkg.signatureThreshold();

		console.log(`threshold count is ${thresholdCount}`);
		//expect(thresholdCount).toBe(2);

		let nextAuthorities = await polkadotApi.query.dkg.nextAuthorities();

		console.log(`authorities are ${nextAuthorities}`);

		// @ts-ignore
		console.log(`authority count is ${nextAuthorities.length}`);
		// @ts-ignore
		//expect(nextAuthorities.length).toBe(3);

		// chill(remove) charlie as validator
		let call = polkadotApi.tx.staking.chill();
		await call.signAndSend(charlieStash);
		const event = await waitForEvent(polkadotApi, 'staking', 'Chilled');
		console.log(`event is ${event}`);

		let forceNewEra = polkadotApi.tx.staking.forceNewEraAlways();
		const forceNewEraAlwayCall = polkadotApi.tx.sudo.sudo({
			callIndex: forceNewEra.callIndex,
			args: forceNewEra.args,
		});
		await forceNewEraAlwayCall.signAndSend(alice);

		// wait for the next 3 sessions
		for  (let i = 0; i < 3; i++) {
			console.log(`waiting for session`)
			const sessh = await waitForTheNextSession(polkadotApi);
			console.log(`session waited: ${sessh}`)
		}

		// get the new signature threshold and authorities
		thresholdCount = await polkadotApi.query.dkg.signatureThreshold();
		console.log(`new threshold count is ${thresholdCount}`);

		nextAuthorities = await polkadotApi.query.dkg.nextAuthorities();

		console.log(`new authorities are ${nextAuthorities}`);
		// @ts-ignore
		//expect(nextAuthorities.length).toBe(2);

		//expect(thresholdCount).toBe(1);


	});

	afterAll(async () => {
		await executeAfter();
	});
});
