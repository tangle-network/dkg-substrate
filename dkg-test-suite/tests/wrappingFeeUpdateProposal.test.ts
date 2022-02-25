import { jest } from '@jest/globals';
import 'jest-extended';
import {
	WrappingFeeUpdateProposal,
	ChainIdType,
	encodeFunctionSignature,
	encodeWrappingFeeUpdateProposal,
	ethAddressFromUncompressedPublicKey,
	fetchDkgPublicKey,
	registerResourceId,
	sleep,
	startStandaloneNode,
	waitForEvent,
	waitUntilDKGPublicKeyStoredOnChain,
	AnchorUpdateProposal,
	encodeUpdateAnchorProposal,
} from '../src/utils';
import { LocalChain } from '../src/localEvm';
import { ChildProcess } from 'child_process';
import { ethers } from 'ethers';
import { Anchors, Bridges } from '@webb-tools/protocol-solidity';
import { MintableToken, GovernedTokenWrapper, TokenWrapperHandler } from '@webb-tools/tokens';
import { ApiPromise, Keyring } from '@polkadot/api';
import { provider } from '../src/utils';
import { ACC1_PK, ACC2_PK, BLOCK_TIME, SECONDS } from '../src/constants';
import {hexToNumber, numberToHex, u8aToHex} from '@polkadot/util';
import { Option } from '@polkadot/types';
import { HexString } from '@polkadot/util/types';
import { signAndSendUtil } from '../src/evm/util/utils';
import {
	aliceNode,
	bobNode,
	localChain,
	polkadotApi,
	signatureBridge,
	wallet1,
	wallet2,
	charlieNode,
	localChain2
} from './utils/util';

describe('Wrapping Fee Update Proposal', () => {

	test('should be able to sign wrapping fee update proposal', async () => {
		const anchor = signatureBridge.getAnchor(localChain.chainId, ethers.utils.parseEther('1'))!;
		const governedTokenAddress = anchor.token!;
		let governedToken = GovernedTokenWrapper.connect(governedTokenAddress , wallet1);
		const resourceId = await governedToken.createResourceId();
		// Create Mintable Token to add to GovernedTokenWrapper
		//Create an ERC20 Token
		const tokenToAdd = await MintableToken.createToken('testToken', 'TEST', wallet1);
		const proposalPayload: WrappingFeeUpdateProposal = {
			header: {
				resourceId,
				functionSignature: encodeFunctionSignature(
					governedToken.contract.interface.functions['setFee(uint8,uint256)'].format()
				),
				nonce: Number(await governedToken.contract.proposalNonce()) + 1,
				chainIdType: ChainIdType.EVM,
				chainId: localChain.chainId,
			},
			newFee: "0x50",
		};
		// register proposal resourceId.
		await expect(registerResourceId(polkadotApi, proposalPayload.header.resourceId)).toResolve();
		const proposalBytes = encodeWrappingFeeUpdateProposal(proposalPayload);
		// get alice account to send the transaction to the dkg node.
		const keyring = new Keyring({ type: 'sr25519' });
		const alice = keyring.addFromUri('//Alice');
		const prop = u8aToHex(proposalBytes);
		const chainIdType = polkadotApi.createType('DkgRuntimePrimitivesChainIdType', {
			EVM: localChain.chainId,
		});
		const kind = polkadotApi.createType('DkgRuntimePrimitivesProposalProposalKind', 'WrappingFeeUpdate');
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
			WrappingFeeUpdateProposal: proposalPayload.header.nonce,
		};
		const proposal = await polkadotApi.query.dKGProposalHandler.signedProposals(chainIdType, key);
		const value = new Option(polkadotApi.registry, 'DkgRuntimePrimitivesProposal', proposal);
		expect(value.isSome).toBeTrue();
		const dkgProposal = value.unwrap().toJSON() as {
			signed: {
				kind: 'WrappingFeeUpdate';
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
		const fee = await governedToken.contract.getFee();
		expect(hexToNumber("0x50")).toEqual(fee);
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
