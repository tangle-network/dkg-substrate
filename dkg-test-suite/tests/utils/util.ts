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
import { ACC1_PK, ACC2_PK, BLOCK_TIME, SECONDS } from '../../src/constants';
import { ApiPromise, WsProvider } from '@polkadot/api';
import { ChildProcess, execSync } from 'child_process';
import fs from 'fs';
import { LocalChain } from '../../src/localEvm';
import { ethers } from 'ethers';
import { Bridges, VBridge } from '@webb-tools/protocol-solidity';
import { MintableToken } from '@webb-tools/tokens';
import {
	endpoint,
	ethAddressFromUncompressedPublicKey,
	fetchDkgPublicKey,
	fetchDkgPublicKeySignature,
	fetchDkgRefreshNonce,
	sleep,
	startStandaloneNode,
	waitForPublicKeySignatureToChange,
	waitForPublicKeyToChange,
	waitUntilDKGPublicKeyStoredOnChain,
} from '../../src/utils';
import { expect } from 'chai';

export let polkadotApi: ApiPromise;
export let aliceNode: ChildProcess;
export let bobNode: ChildProcess;
export let charlieNode: ChildProcess;

export let localChain: LocalChain;
export let localChain2: LocalChain;
export let wallet1: ethers.Wallet;
export let wallet2: ethers.Wallet;

export let signatureBridge: Bridges.SignatureBridge;
export let signatureVBridge: VBridge.VBridge;

export const executeBefore = async ({
	isVariable,
	both,
}: {
	isVariable?: boolean;
	both?: boolean;
}) => {
	// delete the tmp directory if it exists.
	const gitRoot = execSync('git rev-parse --show-toplevel').toString().trim();
	const tmpDir = `${gitRoot}/tmp`;
	if (fs.existsSync(tmpDir)) {
		fs.rmSync(tmpDir, { recursive: true });
	}
	aliceNode = startStandaloneNode('alice', { tmp: true, printLogs: false });
	bobNode = startStandaloneNode('bob', { tmp: true, printLogs: false });
	charlieNode = startStandaloneNode('charlie', { tmp: true, printLogs: false });
	localChain = new LocalChain('local', 5001, [
		{
			balance: ethers.utils.parseEther('1000').toHexString(),
			secretKey: ACC1_PK,
		},
		{
			balance: ethers.utils.parseEther('1000').toHexString(),
			secretKey: ACC2_PK,
		},
		{
			balance: ethers.utils.parseEther('1000').toHexString(),
			secretKey:
				'0x79c3b7fc0b7697b9414cb87adcb37317d1cab32818ae18c0e97ad76395d1fdcf',
		},
		{
			balance: ethers.utils.parseEther('1000').toHexString(),
			secretKey:
				'0xf8d74108dbe199c4a6e4ef457046db37c325ba3f709b14cabfa1885663e4c589',
		},
		{
			balance: ethers.utils.parseEther('1000').toHexString(),
			secretKey:
				'0xcb6df9de1efca7a3998a8ead4e02159d5fa99c3e0d4fd6432667390bb4726854',
		},
	]);
	localChain2 = new LocalChain('local2', 5002, [
		{
			balance: ethers.utils.parseEther('1000').toHexString(),
			secretKey: ACC1_PK,
		},
		{
			balance: ethers.utils.parseEther('1000').toHexString(),
			secretKey: ACC2_PK,
		},
	]);
	wallet1 = new ethers.Wallet(ACC1_PK, localChain.provider());
	wallet2 = new ethers.Wallet(ACC2_PK, localChain2.provider());
	// Deploy the token.
	const localToken = await localChain.deployToken(
		'Webb Token',
		'WEBB',
		wallet1
	);
	const localToken2 = await localChain2.deployToken(
		'Webb Token',
		'WEBB',
		wallet2
	);

	polkadotApi = await ApiPromise.create({
		provider: new WsProvider(endpoint),
	});

	// Update the signature bridge governor.
	const dkgPublicKey = await waitUntilDKGPublicKeyStoredOnChain(polkadotApi);
	expect(dkgPublicKey).to.have.length.greaterThan(0);
	const governorAddress = ethAddressFromUncompressedPublicKey(dkgPublicKey);
	console.log('Initial Governor:', governorAddress);
	let initialGovernors = {
		[localChain.chainId]: wallet1,
		[localChain2.chainId]: wallet2,
	};

	if (both) {
		signatureVBridge = await localChain.deploySignatureVBridge(
			localChain2,
			localToken,
			localToken2,
			wallet1,
			wallet2,
			initialGovernors
		);
		signatureBridge = await localChain.deploySignatureBridge(
			localChain2,
			localToken,
			localToken2,
			wallet1,
			wallet2,
			initialGovernors
		);

		await handleSetup(!!isVariable, governorAddress);
		await handleSetup(!isVariable, governorAddress);
	} else {
		// Deploy the signature bridge.
		if (isVariable) {
			signatureVBridge = await localChain.deploySignatureVBridge(
				localChain2,
				localToken,
				localToken2,
				wallet1,
				wallet2,
				initialGovernors
			);
		} else {
			signatureBridge = await localChain.deploySignatureBridge(
				localChain2,
				localToken,
				localToken2,
				wallet1,
				wallet2,
				initialGovernors
			);
		}

		await handleSetup(!!isVariable, governorAddress);
	}
};

export async function executeAfter() {
	await polkadotApi.disconnect();
	aliceNode?.kill('SIGINT');
	bobNode?.kill('SIGINT');
	charlieNode?.kill('SIGINT');
	await localChain?.stop();
	await localChain2?.stop();
	await sleep(5 * SECONDS);
}

export const handleSetup = async (isVariable: boolean, governor: string) => {
	// get the anchor on localchain1
	const anchor = isVariable
		? signatureVBridge.getVAnchor(localChain.chainId)!
		: signatureBridge.getAnchor(
				localChain.chainId,
				ethers.utils.parseEther('1')
		  )!;

	await anchor.setSigner(wallet1);

	// approve token spending
	const tokenAddress = signatureBridge.getWebbTokenAddress(localChain.chainId)!;
	const token = await MintableToken.tokenFromAddress(tokenAddress, wallet1);
	await token.approveSpending(anchor.contract.address);
	await token.mintTokens(wallet1.address, ethers.utils.parseEther('1000'));

	// do the same but on localchain2
	const anchor2 = isVariable
		? signatureVBridge.getVAnchor(localChain2.chainId)!
		: signatureBridge.getAnchor(
				localChain2.chainId,
				ethers.utils.parseEther('1')
		  )!;
	await anchor2.setSigner(wallet2);
	const tokenAddress2 = signatureBridge.getWebbTokenAddress(
		localChain2.chainId
	)!;
	const token2 = await MintableToken.tokenFromAddress(tokenAddress2, wallet2);
	await token2.approveSpending(anchor2.contract.address);
	await token2.mintTokens(wallet2.address, ethers.utils.parseEther('1000'));

	// update the signature bridge governor on both chains.
	const sides = isVariable
		? signatureVBridge.vBridgeSides.values()
		: signatureBridge.bridgeSides.values();
	for (const signatureSide of sides) {
		const contract = signatureSide.contract;
		// now we transferOwnership, forcefully.
		const tx = await contract.transferOwnership(governor, 1);
		await tx.wait();
		// check that the new governor is the same as the one we just set.
		const currentGovernor = await contract.governor();
		expect(currentGovernor).to.eq(governor);
	}
};

export const waitForAndExecuteNthRotation = async (n: number) => {
	for (let i = 0; i < n; i++) {
		await Promise.all([
			waitForPublicKeyToChange(polkadotApi),
			waitForPublicKeySignatureToChange(polkadotApi),
		]);
		// then we fetch them.
		const dkgPublicKey = await fetchDkgPublicKey(polkadotApi);
		const dkgPublicKeySignature = await fetchDkgPublicKeySignature(polkadotApi);
		const refreshNonce = await fetchDkgRefreshNonce(polkadotApi);
		expect(dkgPublicKey).to.be.length.greaterThan(0);
		expect(dkgPublicKeySignature).to.be.length.greaterThan(0);
		expect(refreshNonce).to.be.greaterThan(1);
		// now we can transfer ownership.
		const bridgeSide = signatureBridge.getBridgeSide(localChain.chainId);
		const contract = bridgeSide.contract;
		contract.connect(localChain.provider());
		const governor = await contract.governor();
		let nextGovernorAddress = ethAddressFromUncompressedPublicKey(
			dkgPublicKey!
		);
		// sanity check
		expect(nextGovernorAddress).not.to.eq(governor);
		let tx = await contract.transferOwnershipWithSignaturePubKey(
			dkgPublicKey!,
			refreshNonce,
			dkgPublicKeySignature!
		);
		try {
			await tx.wait();
		} catch (e) {
			console.log(e);
		}
	}
};
