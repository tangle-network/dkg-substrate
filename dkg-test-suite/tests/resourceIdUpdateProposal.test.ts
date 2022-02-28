import 'jest-extended';
import {
	ResourceIdUpdateProposal,
	ChainIdType,
	encodeFunctionSignature,
	encodeResourceIdUpdateProposal,
	registerResourceId,
	waitForEvent,
} from '../src/utils';
import {ethers} from 'ethers';
import {MintableToken, GovernedTokenWrapper} from '@webb-tools/tokens';
import {Keyring} from '@polkadot/api';
import {u8aToHex} from '@polkadot/util';
import {Option} from '@polkadot/types';
import {HexString} from '@polkadot/util/types';
import {signAndSendUtil} from '../src/evm/util/utils';
import {
	localChain,
	polkadotApi,
	signatureBridge,
	wallet1,
	executeAfter, wallet2
} from './utils/util';

describe('Resource Id Update Proposal', () => {
	test('should be able to sign resource id update proposal', async () => {
		const anchor = signatureBridge.getAnchor(localChain.chainId, ethers.utils.parseEther('1'))!;
		const governedTokenAddress = anchor.token!;
		let governedToken = GovernedTokenWrapper.connect(governedTokenAddress, wallet1);
		const resourceId = await governedToken.createResourceId();
		// Create Mintable Token to add to GovernedTokenWrapper
		//Create an ERC20 Token
		const tokenToAdd = await MintableToken.createToken('testToken', 'TEST', wallet1);
		const nonce = Number(await governedToken.contract.proposalNonce()) + 1;
		const functionSignature = encodeFunctionSignature(
			'adminSetResourceWithSignature(bytes32,bytes4,uint32,bytes32,address,address,bytes'
		);
		const proposalPayload: ResourceIdUpdateProposal = {
			header: {
				resourceId,
				functionSignature: functionSignature,
				nonce: nonce,
				chainIdType: ChainIdType.EVM,
				chainId: localChain.chainId,
			},
			newResourceId: tokenToAdd.contract.address,
			handlerAddress: wallet1.address,
			executionAddress: wallet2.address,
		};
		// register proposal resourceId.
		await expect(registerResourceId(polkadotApi, proposalPayload.header.resourceId)).toResolve();
		const proposalBytes = encodeResourceIdUpdateProposal(proposalPayload);
		// get alice account to send the transaction to the dkg node.
		const keyring = new Keyring({type: 'sr25519'});
		const alice = keyring.addFromUri('//Alice');
		const prop = u8aToHex(proposalBytes);
		const chainIdType = polkadotApi.createType('DkgRuntimePrimitivesChainIdType', {
			EVM: localChain.chainId,
		});
		const kind = polkadotApi.createType('DkgRuntimePrimitivesProposalProposalKind', 'ResourceIdUpdate');
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
			ResourceIdUpdateProposal: proposalPayload.header.nonce,
		};
		const proposal = await polkadotApi.query.dKGProposalHandler.signedProposals(chainIdType, key);
		const value = new Option(polkadotApi.registry, 'DkgRuntimePrimitivesProposal', proposal);
		expect(value.isSome).toBeTrue();
		const dkgProposal = value.unwrap().toJSON() as {
			signed: {
				kind: 'ResourceIdUpdate';
				data: HexString;
				signature: HexString;
			};
		};
		// sanity check.
		expect(dkgProposal.signed.data).toEqual(prop);
		// perfect! now we need to send it to the signature bridge.
		const bridgeSide = await signatureBridge.getBridgeSide(localChain.chainId);
		const contract = bridgeSide.contract;
		const isSignedByGovernor = await contract.isSignatureFromGovernor(
			dkgProposal.signed.data,
			dkgProposal.signed.signature
		);
		expect(isSignedByGovernor).toBeTrue();
		// check that we have the resouceId mapping.
		const tx2 = await contract.adminSetResourceWithSignature(
			resourceId,
			functionSignature,
			nonce,
			resourceId,
			wallet1.address,
			wallet2.address,
			functionSignature
		);
		await expect(tx2.wait()).toResolve();
		// Want to check that token was actually added
		//expect((await governedToken.contract.getTokens()).includes(tokenToAdd.contract.address)).toBeTrue();
	});

	afterAll(async () => {
		await executeAfter();
	});
});
