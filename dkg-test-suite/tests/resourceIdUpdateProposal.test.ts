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
		const bridgeSide = signatureBridge.getBridgeSide(localChain.chainId);
		const resourceId = await bridgeSide.createResourceId();

		// Let's create a new GovernedTokenWrapper and set the resourceId for it via
		// the ResourceIdUpdate Proposal
		const dummyAddress = '0x1111111111111111111111111111111111111111';
		const governedToken = await GovernedTokenWrapper.createGovernedTokenWrapper("token-e2e-test", 'te2e', dummyAddress, dummyAddress, '10000000000000000000000000', false, wallet1);

		const newResourceId = await governedToken.createResourceId();
		const handlerAddress = bridgeSide.tokenHandler.contract.address
		const executionContextAddress = governedToken.contract.address;

		const proposalNonce = Number(await bridgeSide.contract.proposalNonce()) + 1;
		const proposalPayload: ResourceIdUpdateProposal = {
			header: {
				resourceId,
				functionSignature: encodeFunctionSignature(
					bridgeSide.contract.interface.functions['adminSetResourceWithSignature(bytes32,bytes4,uint32,bytes32,address,address,bytes)'].format()
				),
				nonce: proposalNonce,
				chainIdType: ChainIdType.EVM,
				chainId: localChain.chainId,
			},
			newResourceId: newResourceId,
			handlerAddress: handlerAddress,
			executionAddress: executionContextAddress,
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
		const resourceIdUpdateProposal = polkadotApi.createType('DkgRuntimePrimitivesProposal', {
			Unsigned: {
				kind: kind,
				data: prop
			}
		});
		const proposalCall = polkadotApi.tx.dKGProposalHandler.forceSubmitUnsignedProposal(resourceIdUpdateProposal);

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
		const contract = bridgeSide.contract;
		const isSignedByGovernor = await contract.isSignatureFromGovernor(
			dkgProposal.signed.data,
			dkgProposal.signed.signature
		);
		expect(isSignedByGovernor).toBeTrue();
		// check that we have the resouceId mapping.
		const tx2 = await contract.adminSetResourceWithSignature(
			resourceId,
			encodeFunctionSignature('adminSetResourceWithSignature(bytes32,bytes4,uint32,bytes32,address,address,bytes)'),
			proposalNonce,
			newResourceId,
			handlerAddress,
			executionContextAddress,
			dkgProposal.signed.signature,
		);
		await expect(tx2.wait()).toResolve();

		expect(await bridgeSide.contract._resourceIDToHandlerAddress(newResourceId)).toEqual(handlerAddress);
	});

	afterAll(async () => {
		await executeAfter();
	});
});