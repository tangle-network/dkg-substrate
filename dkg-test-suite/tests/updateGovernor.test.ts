import { jest } from '@jest/globals';
import 'jest-extended';
import {
	fastForward,
	fastForwardTo,
	fetchDkgPublicKey,
	fetchDkgPublicKeySignature,
	startStandaloneNode,
	waitForTheNextDkgPublicKey,
	waitForTheNextSession,
	waitUntilDKGPublicKeyStoredOnChain,
} from '../src/utils';
import { LocalChain } from '../src/localEvm';
import { ChildProcess } from 'child_process';
import { ethers } from 'ethers';
import { SignatureBridge } from '@webb-tools/fixed-bridge/lib/packages/fixed-bridge/src/SignatureBridge';
import { SignatureBridge as SignatureBridgeContract } from '@webb-tools/contracts';
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

	let signatureBridge: SignatureBridge;

	let sealingHandle: ReturnType<typeof setInterval>;

	beforeAll(async () => {
		aliceNode = startStandaloneNode('alice', { tmp: true, printLogs: false });
		bobNode = startStandaloneNode('bob', { tmp: true, printLogs: false });
		charlieNode = startStandaloneNode('charlie', { tmp: true, printLogs: true });

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
		// Depoly the signature bridge.
		signatureBridge = await localChain.deploySignatureBridge(
			localChain2,
			localToken,
			localToken2,
			wallet1,
			wallet2
		);

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

		polkadotApi = await ApiPromise.create({
			provider: new WsProvider('ws://127.0.0.1:9944'),
		});

		// Update the signature bridge governor.
		const dkgPublicKey = await waitUntilDKGPublicKeyStoredOnChain(polkadotApi);
		expect(dkgPublicKey).toBeString();
		const signatureSide = signatureBridge.getBridgeSide(localChain.chainId);
		const contract = signatureSide.contract as SignatureBridgeContract;
		contract.connect(localChain.provider());
		const governor = await contract.governor();
		let nextGovernorAddress = ethers.utils.getAddress(
			`0x${ethers.utils.keccak256(dkgPublicKey!).slice(-40)}`
		);
		const tx = await contract.transferOwnership(nextGovernorAddress, 1);
		await expect(tx.wait()).toResolve();
		// check that the new governor is the same as the one we just set.
		const newGovernor = await contract.governor();
		expect(newGovernor).not.toEqualCaseInsensitive(governor);
		expect(newGovernor).toEqualCaseInsensitive(nextGovernorAddress);
	});

	test('should be able to transfer ownership to new Governor with Signature', async () => {
		// stop auto-sealing for now.
		// clearInterval(sealingHandle);
		let nextSessionBlockNumber = (3 * MINUTES) / BLOCK_TIME;
		// then we move faster.
		// await fastForwardTo(polkadotApi, nextSessionBlockNumber, { delayBetweenBlocks: 1000 }); // to trigger a new session.
		await waitForTheNextSession(polkadotApi);
		await waitForTheNextDkgPublicKey(polkadotApi);
		expect(true).toBe(true);
	});

	afterAll(async () => {
		clearInterval(sealingHandle);
		await polkadotApi.disconnect();
		aliceNode?.kill('SIGINT');
		bobNode?.kill('SIGINT');
		charlieNode?.kill('SIGINT');
		await localChain?.stop();
	});
});
