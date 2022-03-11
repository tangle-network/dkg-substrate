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
import {
	AnchorUpdateProposal,
	ResourceIdUpdateProposal,
	TokenAddProposal,
	TokenRemoveProposal,
	MinWithdrawalLimitProposal,
	MaxDepositLimitProposal,
	WrappingFeeUpdateProposal,
	resourceId,
	newResourceId,
	SetTreasuryHandlerProposal,
	SetVerifierProposal,
	FeeRecipientUpdateProposal,
} from "./utils";

let nonce = Math.floor(Math.random() * 100); // Returns a random integer from 0 to 99;

export const anchorUpdateProposal: AnchorUpdateProposal = {
	header: {
		resourceId,
		functionSignature: '0xdeadbeef',
		nonce,
	},
	srcChainId: 5001,
	lastLeafIndex: 0,
	merkleRoot: '0x0000000000000000000000000000000000000000000000000000000000000000',
};

export const tokenAddProposal: TokenAddProposal = {
	header: {
		resourceId,
		functionSignature: '0xdeadbeef',
		nonce,
	},
	newTokenAddress: '0xe69a847cd5bc0c9480ada0b339d7f0a8cac2b667',
};

export const tokenRemoveProposal: TokenRemoveProposal = {
	header: {
		resourceId,
		functionSignature: '0xdeadbeef',
		nonce,
	},
	removeTokenAddress: '0xe69a847cd5bc0c9480ada0b339d7f0a8cac2b667',
};

export const wrappingFeeUpdateProposal: WrappingFeeUpdateProposal = {
	header: {
		resourceId,
		functionSignature: '0xdeadbeef',
		nonce,
	},
	newFee: '0xe69a847cd5bc0c9480ada0b339d7f0a8cac2b667',
};


export const minWithdrawalLimitProposal: MinWithdrawalLimitProposal = {
	header: {
		resourceId,
		functionSignature: '0xdeadbeef',
		nonce,
	},
	minWithdrawalLimitBytes: '0xe69a847cd5bc0c9480ada0b339d7f0a8cac2b667',
};

export const maxDepositLimitProposal: MaxDepositLimitProposal = {
	header: {
		resourceId,
		functionSignature: '0xdeadbeef',
		nonce,
	},
	maxDepositLimitBytes: '0xe69a847cd5bc0c9480ada0b339d7f0a8cac2b667',
};

export const setTreasuryHandlerProposal: SetTreasuryHandlerProposal = {
	header: {
		resourceId,
		functionSignature: '0xdeadbeef',
		nonce,
	},
	newTreasuryHandler: '0xe69a847cd5bc0c9480ada0b339d7f0a8cac2b667',
};

export const resourceIdUpdateProposal: ResourceIdUpdateProposal = {
	header: {
		resourceId,
		functionSignature: '0xdeadbeef',
		nonce,
	},
	// Dummy Values
	newResourceId: newResourceId,
	handlerAddress: '0xe69a847cd5bc0c9480ada0b339d7f0a8cac2b668',
	executionContextAddress: '0xe69a847cd5bc0c9480ada0b339d7f0a8cac2b667',
};

export const setVerifierProposal: SetVerifierProposal = {
	header: {
		resourceId,
		functionSignature: '0xdeadbeef',
		nonce,
	},
	newVerifier: '0xe69a847cd5bc0c9480ada0b339d7f0a8cac2b667',
};

export const feeRecipientUpdateProposal: FeeRecipientUpdateProposal = {
	header: {
		resourceId,
		functionSignature: '0xdeadbeef',
		nonce,
	},
	newFeeRecipient: '0xe69a847cd5bc0c9480ada0b339d7f0a8cac2b667',
};