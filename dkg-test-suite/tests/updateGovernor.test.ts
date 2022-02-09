import { jest } from '@jest/globals';
import { startStandaloneNode } from '../src/utils';
import { LocalChain } from '../src/localEvm';
import { ChildProcess } from 'child_process';
import { ethers } from 'ethers';
import { SignatureBridge } from '@webb-tools/fixed-bridge/lib/packages/fixed-bridge/src/SignatureBridge';
import { MintableToken } from '@webb-tools/tokens';

describe('Update SignatureBridge Governor', () => {
	const SECONDS = 1000;
	const ACC1_PK = '0x0000000000000000000000000000000000000000000000000000000000000001';
	const ACC2_PK = '0x0000000000000000000000000000000000000000000000000000000000000002';
	jest.setTimeout(60 * SECONDS);

	let aliceNode: ChildProcess;
	let bobNode: ChildProcess;
	let charlieNode: ChildProcess;

	let localChain: LocalChain;
	let wallet1: ethers.Wallet;
	let wallet2: ethers.Wallet;

	let signatureBridge: SignatureBridge;

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
		wallet1 = new ethers.Wallet(ACC1_PK, localChain.provider());
		wallet2 = new ethers.Wallet(ACC2_PK, localChain.provider());
		// Deploy the token.
		const localToken = await localChain.deployToken('LocalChain', 'WEBB', wallet1);
		// Depoly the signature bridge.
		signatureBridge = await localChain.deploySignatureBridge(
			localChain,
			localToken,
			localToken,
			wallet1,
			wallet1
		);

		// get the anhor on chainA
		const anchor = signatureBridge.getAnchor(localChain.chainId, ethers.utils.parseEther('1'))!;
		await anchor.setSigner(wallet1);
		// approve token spending
		const tokenAddress = signatureBridge.getWebbTokenAddress(localChain.chainId)!;
		const token = await MintableToken.tokenFromAddress(tokenAddress, wallet1);
		await token.approveSpending(anchor.contract.address);
		await token.mintTokens(wallet1.address, ethers.utils.parseEther('1000'));
	});

	test('it should pass', () => {
		expect(true).toBe(true);
	});

	afterAll(async () => {
		aliceNode?.kill();
		bobNode?.kill();
		charlieNode?.kill();
		await localChain?.stop();
	});
});
