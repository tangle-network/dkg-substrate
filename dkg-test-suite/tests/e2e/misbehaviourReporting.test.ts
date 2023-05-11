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

// A test suite for testing Misbehaviour in the DKG System
// and to observe the behavior of the system.
//
// When we start, we expect the Keygen to equal to 2, then we wait for the first keygen to finish,
// then we update the Keygen to 3, expecting the system to now work with 3 Authorities.
// Next, we shutdown one of the authorities, we expect the system to report a misbehaviour,
// then, we expect the system to adjust the Keygen to 2, expecting the system to now work with 2 Authorities.
//
// Next, we start the misbehaving authority again, and unjail it, then adjust the Keygen to 3,
// expecting the system to now work with 3 Authorities again.

import fs from 'fs';
import { expect } from 'chai';
import { ChildProcess, execSync } from 'child_process';
import {
	startStandaloneNode,
	sudoTx,
	waitForTheNextSession,
	sleep,
	waitForTheNextDkgPublicKey,
	waitForEvent,
	fetchDkgPublicKey,
	endpoint,
} from '../utils/setup';
import { ApiPromise, Keyring, WsProvider } from '@polkadot/api';
import { Vec } from '@polkadot/types';
import { BLOCK_TIME } from '../utils/constants';

// Requires total of 8 sessions to complete.
describe.skip('Misbehavior Flow', function () {
	// 30 sessions should be more than enough for the test to complete
	this.timeout(300 * BLOCK_TIME);
	// 5 session.
	this.slow(50 * BLOCK_TIME);
	// fail fast, since tests are dependent on each other.
	this.bail(true);

	let aliceNode: ChildProcess;
	let bobNode: ChildProcess;
	let charlieNode: ChildProcess;
	let api: ApiPromise;

	before(async () => {
		// delete the tmp directory if it exists.
		const gitRoot = execSync('git rev-parse --show-toplevel').toString().trim();
		const tmpDir = `${gitRoot}/tmp`;
		if (fs.existsSync(tmpDir)) {
			fs.rmSync(tmpDir, { recursive: true });
		}
		aliceNode = startStandaloneNode('alice', { tmp: true, printLogs: false, output_dir: tmpDir });
		bobNode = startStandaloneNode('bob', { tmp: true, printLogs: false, output_dir: tmpDir });
		charlieNode = startStandaloneNode('charlie', {
			tmp: true,
			printLogs: false,
			output_dir: tmpDir,
		});

		api = await ApiPromise.create({
			provider: new WsProvider(endpoint),
		});
	});

	// This test requires at least 4 sessions.
	// 4 sessions = 4 * 10 blocks = 40 blocks = 40 * 6 seconds = 240 seconds.
	it('should report misbehaviour and update the keygen threshold', async () => {
		// first query the current keygen threshold value.
		const currentKeygenThreshold = await api.query.dkg.keygenThreshold();
		expect(currentKeygenThreshold.toHex()).to.equal(
			'0x0002',
			'Keygen threshold at the start should be 2'
		);
		// then, we shall query the current best authorities.
		const currentBestAuthoritiesValue = await api.query.dkg.bestAuthorities();
		const currentBestAuthorities = new Vec(
			api.registry,
			'(u16,DkgRuntimePrimitivesCryptoPublic)',
			currentBestAuthoritiesValue.toU8a()
		);
		expect(currentBestAuthorities.length).to.equal(
			2,
			'Current best authorities should be 2'
		);
		// and we wait for the first keygen to be completed.
		await waitForTheNextDkgPublicKey(api);
		// next, is to increase the keygen threshold.
		const increaseKeygenThreshold = api.tx.dkg.setKeygenThreshold(3);
		await sudoTx(api, increaseKeygenThreshold);
		// and then, we query for the pending keygen threshold.
		// we expect the pending keygen threshold to be 3.
		const pendingKeygenThreshold = await api.query.dkg.pendingKeygenThreshold();
		expect(pendingKeygenThreshold.toHex()).to.equal(
			'0x0003',
			'Pending keygen threshold should be 3'
		);
		// now we wait for the next session, we expect the next keygen threshold to be 3.
		// we also expect the next best authorities to be 3.
		await waitForTheNextSession(api);
		await sleep(BLOCK_TIME * 2);
		const nextKeygenThreshold = await api.query.dkg.nextKeygenThreshold();
		expect(nextKeygenThreshold.toHex()).to.equal(
			'0x0003',
			'Next keygen threshold should be 3'
		);
		const nextBestAuthoritiesValue = await api.query.dkg.nextBestAuthorities();
		const nextBestAuthorities = new Vec(
			api.registry,
			'(u16,DkgRuntimePrimitivesCryptoPublic)',
			nextBestAuthoritiesValue.toU8a()
		);
		expect(nextBestAuthorities.length).to.equal(
			3,
			'Next best authorities should be 3'
		);

		// now, wait for the next session, we expect the keygen threshold to be 3.
		// and we expect the best authorities to be 3.
		// Also, the DKG rotation should still working as expected.
		await waitForTheNextSession(api);
		await sleep(BLOCK_TIME * 2);
		const keygenThreshold = await api.query.dkg.keygenThreshold();
		expect(keygenThreshold.toHex()).to.equal(
			'0x0003',
			'Keygen threshold should be now equal to 3'
		);
		const bestAuthoritiesValue = await api.query.dkg.bestAuthorities();
		const bestAuthorities = new Vec(
			api.registry,
			'(u16,DkgRuntimePrimitivesCryptoPublic)',
			bestAuthoritiesValue.toU8a()
		);
		expect(bestAuthorities.length).to.equal(3, 'Best authorities should be 3');
		// Now, we shall shutdown one of the authorities.
		//
		// We will kill charlie's node.
		charlieNode.kill('SIGINT');
		// Now, we should expect the system to report misbehaviour.
		// and we should expect the keygen threshold to be 2.
		// and we should expect the best authorities to be 2.
		// Then we also expect a new Keygen to happen.
		await waitForEvent(api, 'dkg', 'MisbehaviourReportsSubmitted');
		await sleep(BLOCK_TIME * 1);
		// then wait for 2 sessions.
		await waitForTheNextSession(api);
		await sleep(BLOCK_TIME * 2);
		await waitForTheNextSession(api);
		await sleep(BLOCK_TIME * 2);
		// now we observe the changes.
		const keygenThresholdAfterMisbehaviour =
			await api.query.dkg.keygenThreshold();
		expect(keygenThresholdAfterMisbehaviour.toHex()).to.equal(
			'0x0002',
			'Keygen threshold after misbehaviour should be now equal to 2'
		);
		const bestAuthoritiesValueAfterMisbehaviour =
			await api.query.dkg.bestAuthorities();
		const bestAuthoritiesAfterMisbehaviour = new Vec(
			api.registry,
			'(u16,DkgRuntimePrimitivesCryptoPublic)',
			bestAuthoritiesValueAfterMisbehaviour.toU8a()
		);
		expect(bestAuthoritiesAfterMisbehaviour.length).to.equal(
			2,
			'Best authorities after misbehaviour should be now equal to 2'
		);
	});

	// This test requires at least 3 sessions.
	// 3 sessions = 3 * 10 blocks = 30 blocks = 30 * 6 seconds = 180 seconds.
	it('should include the unjailed authority after keygen threshold change', async () => {
		// first query the current keygen threshold value.
		const currentKeygenThreshold = await api.query.dkg.keygenThreshold();
		expect(currentKeygenThreshold.toHex()).to.equal(
			'0x0002',
			'Keygen threshold at the start should be 2'
		);
		// then, we shall query the current best authorities.
		const currentBestAuthoritiesValue = await api.query.dkg.bestAuthorities();
		const currentBestAuthorities = new Vec(
			api.registry,
			'(u16,DkgRuntimePrimitivesCryptoPublic)',
			currentBestAuthoritiesValue.toU8a()
		);
		expect(currentBestAuthorities.length).to.equal(
			2,
			'Current best authorities should be 2'
		);
		// then start the killed node, it was charlie.
		charlieNode = startStandaloneNode('charlie', {
			tmp: true,
			printLogs: false,
		});
		// then we want to unjail charlie.
		const unjailCharlie = api.tx.dkg.unjail();
		const keyring = new Keyring({ type: 'sr25519' });
		const charlie = keyring.addFromUri('//Charlie');
		await unjailCharlie.signAndSend(charlie);
		// next, is to increase the keygen threshold.
		const increaseKeygenThreshold = api.tx.dkg.setKeygenThreshold(3);
		await sudoTx(api, increaseKeygenThreshold);
		// and then, we query for the pending keygen threshold.
		// we expect the pending keygen threshold to be 3.
		const pendingKeygenThreshold = await api.query.dkg.pendingKeygenThreshold();
		expect(pendingKeygenThreshold.toHex()).to.equal(
			'0x0003',
			'Pending keygen threshold should be 3'
		);
		// now we wait for the next session, we expect the next keygen threshold to be 3.
		// we also expect the next best authorities to be 3.
		await waitForTheNextSession(api);
		await sleep(BLOCK_TIME * 2);
		const nextKeygenThreshold = await api.query.dkg.nextKeygenThreshold();
		expect(nextKeygenThreshold.toHex()).to.equal(
			'0x0003',
			'Next keygen threshold should be 3'
		);
		const nextBestAuthoritiesValue = await api.query.dkg.nextBestAuthorities();
		const nextBestAuthorities = new Vec(
			api.registry,
			'(u16,DkgRuntimePrimitivesCryptoPublic)',
			nextBestAuthoritiesValue.toU8a()
		);
		expect(nextBestAuthorities.length).to.equal(
			3,
			'Next best authorities should be 3'
		);

		// now, wait for the next session, we expect the keygen threshold to be 3.
		// and we expect the best authorities to be 3.
		// Also, the DKG rotation should still working as expected.
		await waitForTheNextSession(api);
		await sleep(BLOCK_TIME * 2);
		const keygenThreshold = await api.query.dkg.keygenThreshold();
		expect(keygenThreshold.toHex()).to.equal(
			'0x0003',
			'Keygen threshold should be now equal to 3'
		);
		const bestAuthoritiesValue = await api.query.dkg.bestAuthorities();
		const bestAuthorities = new Vec(
			api.registry,
			'(u16,DkgRuntimePrimitivesCryptoPublic)',
			bestAuthoritiesValue.toU8a()
		);
		expect(bestAuthorities.length).to.equal(3, 'Best authorities should be 3');
		// query the current DKG public key.
		const currentDkgPublicKey = await fetchDkgPublicKey(api);
		// now wait for the next session, we expect the dkg to rotate.
		await waitForTheNextSession(api);
		await sleep(BLOCK_TIME * 2);
		// query the next DKG public key.
		const newDkgPublicKey = await fetchDkgPublicKey(api);
		// we expect the new dkg public key to be different from the current dkg public key.
		expect(newDkgPublicKey).to.not.equal(
			currentDkgPublicKey,
			'DKG public key should be different after a rotation'
		);
	});

	after(async () => {
		await api?.disconnect();
		aliceNode?.kill('SIGINT');
		bobNode?.kill('SIGINT');
		charlieNode?.kill('SIGINT');
		await sleep(BLOCK_TIME * 2);
	});
});
