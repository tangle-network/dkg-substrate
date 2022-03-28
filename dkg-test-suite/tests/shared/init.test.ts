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
import 'jest-extended';
import {jest} from "@jest/globals";
import {
	startStandaloneNode,
	provider,
	waitUntilDKGPublicKeyStoredOnChain,
	ethAddressFromUncompressedPublicKey,
	sleep,
} from '../../src/utils';
import {ethers} from 'ethers';
import {MintableToken} from '@webb-tools/tokens';
import {ApiPromise} from '@polkadot/api';
import { ChildProcess } from 'child_process';
import { LocalChain } from '../../src/localEvm';
import { VBridge } from '@webb-tools/protocol-solidity';
import { ACC1_PK, ACC2_PK, BLOCK_TIME, SECONDS } from '../../src/constants';
import {maxDepositTest} from '../shared/e2e/maxDeposit.test'
import {minWithdrawalTest} from '../shared/e2e/minWithdrawal.test'
import {thresholdTest} from '../shared/e2e/threshold.test'
import {setUpdateProposalTest} from '../shared/e2e/proposerSetUpdateProposal.test'
import {resourceIdUpdateTest} from "./e2e/resourceIdUpdateProposal.test";

function importTest(name: string | number | jest.FunctionLike, path: string) {
	describe(name, function () {
		require(path);
	});
}

describe('All e2e Tests sharing the same start up', () => {
	jest.setTimeout(100000 * BLOCK_TIME); // 100 blocks

	let polkadotApi: ApiPromise;
	let aliceNode: ChildProcess;
	let bobNode: ChildProcess;
	let charlieNode: ChildProcess;
	let localChain: LocalChain;
	let localChain2: LocalChain;
	let wallet1: ethers.Wallet;
	let wallet2: ethers.Wallet;
	let signatureVBridge: VBridge.VBridge;

	beforeAll(async () => {
		aliceNode = startStandaloneNode('alice', {tmp: true, printLogs: false});
		bobNode = startStandaloneNode('bob', {tmp: true, printLogs: false});
		charlieNode = startStandaloneNode('charlie', {tmp: true, printLogs: false});

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
				secretKey: '0x79c3b7fc0b7697b9414cb87adcb37317d1cab32818ae18c0e97ad76395d1fdcf',
			},
			{
				balance: ethers.utils.parseEther('1000').toHexString(),
				secretKey: '0xf8d74108dbe199c4a6e4ef457046db37c325ba3f709b14cabfa1885663e4c589',
			},
			{
				balance: ethers.utils.parseEther('1000').toHexString(),
				secretKey: '0xcb6df9de1efca7a3998a8ead4e02159d5fa99c3e0d4fd6432667390bb4726854',
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
		const localToken = await localChain.deployToken('Webb Token', 'WEBB', wallet1);
		const localToken2 = await localChain2.deployToken('Webb Token', 'WEBB', wallet2);

		polkadotApi = await ApiPromise.create({
			provider,
		});

		// Update the signature bridge governor.
		const dkgPublicKey = await waitUntilDKGPublicKeyStoredOnChain(polkadotApi);
		expect(dkgPublicKey).toBeString();
		const governorAddress = ethAddressFromUncompressedPublicKey(dkgPublicKey);

		let initialGovernors = {
			[localChain.chainId]: wallet1,
			[localChain2.chainId]: wallet2,
		};

		// Depoly the signature bridge.
		signatureVBridge = await localChain.deploySignatureVBridge(
			localChain2,
			localToken,
			localToken2,
			wallet1,
			wallet2,
			initialGovernors
		);
		// get the anchor on localchain1
		const vAnchor = signatureVBridge.getVAnchor(localChain.chainId)!;
		await vAnchor.setSigner(wallet1);
		// approve token spending
		const tokenAddress = signatureVBridge.getWebbTokenAddress(localChain.chainId)!;
		const token = await MintableToken.tokenFromAddress(tokenAddress, wallet1);
		await token.approveSpending(vAnchor.contract.address);
		await token.mintTokens(wallet1.address, ethers.utils.parseEther('1000'));

		// do the same but on localchain2
		const anchor2 = signatureVBridge.getVAnchor(localChain2.chainId)!;
		await anchor2.setSigner(wallet2);
		const tokenAddress2 = signatureVBridge.getWebbTokenAddress(localChain2.chainId)!;
		const token2 = await MintableToken.tokenFromAddress(tokenAddress2, wallet2);
		await token2.approveSpending(anchor2.contract.address);
		await token2.mintTokens(wallet2.address, ethers.utils.parseEther('1000'));

		// update the signature bridge governor on both chains.
		const sides = signatureVBridge.vBridgeSides.values();
		for (const signatureSide of sides) {
			const contract = signatureSide.contract;
			// now we transferOwnership, forcefully.
			const tx = await contract.transferOwnership(governorAddress, 1);
			expect(tx.wait()).toResolve();
			// check that the new governor is the same as the one we just set.
			const currentGovernor = await contract.governor();
			expect(currentGovernor).toEqualCaseInsensitive(governorAddress);
		}
	});

	test('should be able to  remove validator node and check threshold', async () => {
		await thresholdTest(polkadotApi);
	});

	test('should be able to update max deposit limit', async () => {
		await maxDepositTest(signatureVBridge, localChain, polkadotApi);
	});

	test('should be able to update minimum withdrawal limit', async () => {
		await minWithdrawalTest(signatureVBridge, localChain, polkadotApi);
	});

	/*test('proposer set update test', async () => {
		await setUpdateProposalTest(signatureVBridge, localChain, polkadotApi);
	});*/

	test('resource id update test', async () => {
		await resourceIdUpdateTest(localChain, polkadotApi);
	});

	afterAll(async () => {
		await polkadotApi.disconnect();
		aliceNode?.kill('SIGINT');
		bobNode?.kill('SIGINT');
		charlieNode?.kill('SIGINT');
		await localChain?.stop();
		await localChain2?.stop();
		await sleep(5 * SECONDS);
	});

});
