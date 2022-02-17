import { jest } from '@jest/globals';
import 'jest-extended';
import {
	fetchDkgPublicKey,
	fetchDkgPublicKeySignature,
	fetchDkgRefreshNonce,
	sleep,
	startStandaloneNode,
	triggerDkgManualRefresh,
	triggerDkgManualRenonce,
	waitForPublicKeySignatureToChange,
	waitForPublicKeyToChange,
	waitUntilDKGPublicKeyStoredOnChain,
} from '../src/utils';
import { LocalChain } from '../src/localEvm';
import { ChildProcess } from 'child_process';
import { ethers } from 'ethers';
import { Bridges } from '@webb-tools/protocol-solidity';
import { MintableToken } from '@webb-tools/tokens';
import { ApiPromise, Keyring, WsProvider } from '@polkadot/api';

describe('Update SignatureBridge Governor', () => {
	const SECONDS = 1000;
	const MINUTES = 60 * SECONDS;
	const BLOCK_TIME = 3 * SECONDS;
	const ACC1_PK = '0x0000000000000000000000000000000000000000000000000000000000000001';
	const ACC2_PK = '0x0000000000000000000000000000000000000000000000000000000000000002';
	jest.setTimeout(100 * BLOCK_TIME); // 100 blocks

	let polkadotApi: ApiPromise;
	let aliceNode: ChildProcess;
	let bobNode: ChildProcess;
	let charlieNode: ChildProcess;

	let localChain: LocalChain;
	let localChain2: LocalChain;
	let wallet1: ethers.Wallet;
	let wallet2: ethers.Wallet;

	let signatureBridge: Bridges.SignatureBridge;

	beforeAll(async () => {
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
			provider: new WsProvider('ws://127.0.0.1:9944'),
		});

		// Update the signature bridge governor.
		let dkgPublicKey = await waitUntilDKGPublicKeyStoredOnChain(polkadotApi);
		expect(dkgPublicKey).toBeString();
		dkgPublicKey = `0x${dkgPublicKey.slice(4)}`;
		let governorAddress = ethers.utils.getAddress(
			`0x${ethers.utils.keccak256(dkgPublicKey).slice(-40)}`
		);

		let intialGovernors = {
			[localChain.chainId]: wallet1,
			[localChain2.chainId]: wallet2,
		};

		// Depoly the signature bridge.
		signatureBridge = await localChain.deploySignatureBridge(
			localChain2,
			localToken,
			localToken2,
			wallet1,
			wallet2,
			intialGovernors
		);
		const signatureSide = signatureBridge.getBridgeSide(localChain.chainId);
		const contract = signatureSide.contract;
		contract.connect(localChain.provider());
		// now we transferOwnership, forcefully.
		const tx = await contract.transferOwnership(governorAddress, 1);
		expect(tx.wait()).toResolve();
		// check that the new governor is the same as the one we just set.
		const currentGovernor = await contract.governor();
		expect(currentGovernor).toEqualCaseInsensitive(governorAddress);

		// get the anhor on localchain1
		const anchor = signatureBridge.getAnchor(localChain.chainId, ethers.utils.parseEther('1'))!;
		await anchor.setSigner(wallet1);
		// approve token spending
		const tokenAddress = signatureBridge.getWebbTokenAddress(localChain.chainId)!;
		const token = await MintableToken.tokenFromAddress(tokenAddress, wallet1);
		await token.approveSpending(anchor.contract.address);
		await token.mintTokens(wallet1.address, ethers.utils.parseEther('1000'));

		// do the same but on localchain2
		const anchor2 = signatureBridge.getAnchor(localChain2.chainId, ethers.utils.parseEther('1'))!;
		await anchor2.setSigner(wallet2);
		const tokenAddress2 = signatureBridge.getWebbTokenAddress(localChain2.chainId)!;
		const token2 = await MintableToken.tokenFromAddress(tokenAddress2, wallet2);
		await token2.approveSpending(anchor2.contract.address);
		await token2.mintTokens(wallet2.address, ethers.utils.parseEther('1000'));
	});

	test('should be able to transfer ownership to new Governor with Signature', async () => {
		// we trigger a manual renonce since we already transfered the ownership before.
		await triggerDkgManualRenonce(polkadotApi);
		// for some reason, we have to wait for a bit ¯\_(ツ)_/¯.
		await sleep(2 * BLOCK_TIME);
		// we trigger a manual DKG Refresh.
		await triggerDkgManualRefresh(polkadotApi);
		// then we wait until the dkg public key and its signature to get changed.
		await Promise.all([
			waitForPublicKeyToChange(polkadotApi),
			waitForPublicKeySignatureToChange(polkadotApi),
		]);
		// then we fetch them.
		let dkgPublicKey = await fetchDkgPublicKey(polkadotApi);
		const dkgPublicKeySignature = await fetchDkgPublicKeySignature(polkadotApi);
		const refreshNonce = await fetchDkgRefreshNonce(polkadotApi);
		expect(dkgPublicKey).toBeString();
		expect(dkgPublicKeySignature).toBeString();
		expect(refreshNonce).toBeGreaterThan(0);
		// remove the 0x04 prefix.
		dkgPublicKey = `0x${dkgPublicKey!.slice(4)}`;
		// now we can transfer ownership.
		const signatureSide = signatureBridge.getBridgeSide(localChain.chainId);
		const contract = signatureSide.contract;
		contract.connect(localChain.provider());
		const governor = await contract.governor();
		console.log(`governor: ${governor}`);
		let nextGovernorAddress = ethers.utils.getAddress(
			`0x${ethers.utils.keccak256(dkgPublicKey!).slice(-40)}`
		);
		console.log(`nextGovernor: ${nextGovernorAddress}`);
		console.log(`publicKey: ${dkgPublicKey}`);
		console.log(`refreshNonce: ${refreshNonce}`);
		console.log(`signature: ${dkgPublicKeySignature}`);
		expect(nextGovernorAddress).not.toEqualCaseInsensitive(governor);
		const tx = await contract.transferOwnershipWithSignaturePubKey(
			dkgPublicKey!,
			refreshNonce,
			dkgPublicKeySignature!
		);
		await expect(tx.wait()).toResolve();
		// check that the new governor is the same as the one we just set.
		const newGovernor = await contract.governor();
		expect(newGovernor).not.toEqualCaseInsensitive(governor);
		expect(newGovernor).toEqualCaseInsensitive(nextGovernorAddress);
	});

	afterAll(async () => {
		await polkadotApi.disconnect();
		aliceNode?.kill('SIGINT');
		bobNode?.kill('SIGINT');
		charlieNode?.kill('SIGINT');
		await localChain?.stop();
	});
});
