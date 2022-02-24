import { jest } from '@jest/globals';
import 'jest-extended';
import {
	TokenAddProposal,
	ChainIdType,
	encodeFunctionSignature,
	encodeTokenAddProposal,
	encodeTokenRemoveProposal,
	ethAddressFromUncompressedPublicKey,
	fetchDkgPublicKey,
	registerResourceId,
	sleep,
	startStandaloneNode,
	waitForEvent,
	waitUntilDKGPublicKeyStoredOnChain,
	AnchorUpdateProposal,
	encodeUpdateAnchorProposal, TokenRemoveProposal,
} from '../src/utils';
import { LocalChain } from '../src/localEvm';
import { ChildProcess } from 'child_process';
import { ethers } from 'ethers';
import { Anchors, Bridges } from '@webb-tools/protocol-solidity';
import { MintableToken, GovernedTokenWrapper, TokenWrapperHandler } from '@webb-tools/tokens';
import { ApiPromise, Keyring } from '@polkadot/api';
import { provider } from '../src/utils';
import { ACC1_PK, ACC2_PK, BLOCK_TIME, SECONDS } from '../src/constants';
import { u8aToHex } from '@polkadot/util';
import { Option } from '@polkadot/types';
import { HexString } from '@polkadot/util/types';
import { signAndSendUtil } from '../src/evm/util/utils';


describe('Token Add Proposal', () => {
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

	let governedToken: GovernedTokenWrapper;

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
		signatureBridge = await localChain.deploySignatureBridge(
			localChain2,
			localToken,
			localToken2,
			wallet1,
			wallet2,
			initialGovernors
		);
		// get the anchor on localchain1
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

		// update the signature bridge governor on both chains.
		const sides = signatureBridge.bridgeSides.values();
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

	test('should be able to sign token add proposal', async () => {
		const anchor = signatureBridge.getAnchor(localChain.chainId, ethers.utils.parseEther('1'))!;
		const governedTokenAddress = anchor.token!;
		governedToken = GovernedTokenWrapper.connect(governedTokenAddress , wallet1);
		const resourceId = await governedToken.createResourceId();
		// Create Mintable Token to add to GovernedTokenWrapper
		//Create an ERC20 Token
		const tokenToAdd = await MintableToken.createToken('testToken', 'TEST', wallet1);
		{

			const proposalPayload: TokenAddProposal = {
				header: {
					resourceId,
					functionSignature: encodeFunctionSignature(
						governedToken.contract.interface.functions['add(address,uint256)'].format()
					),
					nonce: Number(await governedToken.contract.proposalNonce()) + 1,
					chainIdType: ChainIdType.EVM,
					chainId: localChain.chainId,
				},
				newTokenAddress: tokenToAdd.contract.address,
			};
			// register proposal resourceId.
			await expect(registerResourceId(polkadotApi, proposalPayload.header.resourceId)).toResolve();
			const proposalBytes = encodeTokenAddProposal(proposalPayload);
			// get alice account to send the transaction to the dkg node.
			const keyring = new Keyring({ type: 'sr25519' });
			const alice = keyring.addFromUri('//Alice');
			const prop = u8aToHex(proposalBytes);
			const chainIdType = polkadotApi.createType('DkgRuntimePrimitivesChainIdType', {
				EVM: localChain.chainId,
			});
			const kind = polkadotApi.createType('DkgRuntimePrimitivesProposalProposalKind', 'TokenAdd');
			const tokenAddProposal = polkadotApi.createType('DkgRuntimePrimitivesProposal', {
				Unsigned: {
					kind: kind,
					data: prop
				}
			});
			const proposalCall = polkadotApi.tx.dKGProposalHandler.forceSubmitUnsignedProposal(tokenAddProposal);

			await signAndSendUtil(polkadotApi, proposalCall, alice);

			// now we need to wait until the proposal to be signed on chain.
			await waitForEvent(polkadotApi, 'dKGProposalHandler', 'ProposalSigned');
			// now we need to query the proposal and its signature.
			const key = {
				TokenAddProposal: proposalPayload.header.nonce,
			};
			const proposal = await polkadotApi.query.dKGProposalHandler.signedProposals(chainIdType, key);
			const value = new Option(polkadotApi.registry, 'DkgRuntimePrimitivesProposal', proposal);
			expect(value.isSome).toBeTrue();
			const dkgProposal = value.unwrap().toJSON() as {
				signed: {
					kind: 'TokenAdd';
					data: HexString;
					signature: HexString;
				};
			};
			// sanity check.
			expect(dkgProposal.signed.data).toEqual(prop);
			// perfect! now we need to send it to the signature bridge.
			// but first, we need to log few things to help us to debug.'
			const bridgeSide = await signatureBridge.getBridgeSide(localChain.chainId);
			const contract = bridgeSide.contract;
			const isSignedByGovernor = await contract.isSignatureFromGovernor(
				dkgProposal.signed.data,
				dkgProposal.signed.signature
			);
			expect(isSignedByGovernor).toBeTrue();
			// check that we have the resouceId mapping.
			const tx2 = await contract.executeProposalWithSignature(
				dkgProposal.signed.data,
				dkgProposal.signed.signature
			);
			await expect(tx2.wait()).toResolve();
			// Want to check that token was actually added
			expect((await governedToken.contract.getTokens()).includes(tokenToAdd.contract.address)).toBeTrue();
		}
		await sleep(5 * BLOCK_TIME);
		//const tokenToRemove = await MintableToken.createToken('testToken', 'TEST', wallet1);
		const proposalPayload: TokenRemoveProposal = {
			header: {
				resourceId,
				functionSignature: encodeFunctionSignature(
					governedToken.contract.interface.functions['remove(address,uint256)'].format()
				),
				nonce: Number(await governedToken.contract.proposalNonce()) + 1,
				chainIdType: ChainIdType.EVM,
				chainId: localChain.chainId,
			},
			removeTokenAddress: tokenToAdd.contract.address,
		};
		// register proposal resourceId.
		await expect(registerResourceId(polkadotApi, proposalPayload.header.resourceId)).toResolve();
		const proposalBytes = encodeTokenRemoveProposal(proposalPayload);
		// get alice account to send the transaction to the dkg node.
		const keyring = new Keyring({ type: 'sr25519' });
		const alice = keyring.addFromUri('//Alice');
		const prop = u8aToHex(proposalBytes);
		const chainIdType = polkadotApi.createType('DkgRuntimePrimitivesChainIdType', {
			EVM: localChain.chainId,
		});
		const kind = polkadotApi.createType('DkgRuntimePrimitivesProposalProposalKind', 'TokenRemove');
		const tokenRemoveProposal = polkadotApi.createType('DkgRuntimePrimitivesProposal', {
			Unsigned: {
				kind: kind,
				data: prop
			}
		});
		const proposalCall = polkadotApi.tx.dKGProposalHandler.forceSubmitUnsignedProposal(tokenRemoveProposal);

		await signAndSendUtil(polkadotApi, proposalCall, alice);

		// now we need to wait until the proposal to be signed on chain.
		await waitForEvent(polkadotApi, 'dKGProposalHandler', 'ProposalSigned');
		// now we need to query the proposal and its signature.
		const key = {
			TokenRemoveProposal: proposalPayload.header.nonce,
		};
		const proposal = await polkadotApi.query.dKGProposalHandler.signedProposals(chainIdType, key);
		const value = new Option(polkadotApi.registry, 'DkgRuntimePrimitivesProposal', proposal);

		expect(value.isSome).toBeTrue();
		const dkgProposal = value.unwrap().toJSON() as {
			signed: {
				kind: 'TokenRemove';
				data: HexString;
				signature: HexString;
			};
		};
		// sanity check.
		expect(dkgProposal.signed.data).toEqual(prop);
		// perfect! now we need to send it to the signature bridge.
		// but first, we need to log few things to help us to debug.'
		const bridgeSide = await signatureBridge.getBridgeSide(localChain.chainId);
		const contract = bridgeSide.contract;
		const isSignedByGovernor = await contract.isSignatureFromGovernor(
			dkgProposal.signed.data,
			dkgProposal.signed.signature
		);
		expect(isSignedByGovernor).toBeTrue();
		// check that we have the resouceId mapping.
		const tx2 = await contract.executeProposalWithSignature(
			dkgProposal.signed.data,
			dkgProposal.signed.signature
		);
		await expect(tx2.wait()).toResolve();
		// Want to check that token was actually added
		expect((await governedToken.contract.getTokens()).includes(tokenToAdd.contract.address)).toBeFalse();
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