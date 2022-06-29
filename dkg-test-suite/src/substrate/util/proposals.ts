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
	SubstrateProposal,
	substratePalletResourceId,
	ResourceIdUpdateProposal,
} from './utils';
import { ApiPromise } from '@polkadot/api';

let nonce = Math.floor(Math.random() * 100); // Returns a random integer from 0 to 99;

export const getWrappingFeeUpdateProposal = (api: ApiPromise) => {
	const wrappingFeeUpdateProposal: SubstrateProposal = {
		header: {
			resourceId: substratePalletResourceId,
			functionSignature: '0x00000000',
			nonce,
		},
		// TODO: this is a dummy call, should replace with actual call
		call: api.tx.system
			.remark('execute wrapping fee update proposal')
			.toString(),
	};
	return wrappingFeeUpdateProposal;
};

export const getTokenAddProposal = (api: ApiPromise) => {
	const tokenAddProposal: SubstrateProposal = {
		header: {
			resourceId: substratePalletResourceId,
			functionSignature: '0x00000000',
			nonce,
		},
		// TODO: this is a dummy call, should replace with actual call
		call: api.tx.system.remark('execute token add proposal').toString(),
	};
	return tokenAddProposal;
};

export const getTokenRemoveProposal = (api: ApiPromise) => {
	const tokenRemoveProposal: SubstrateProposal = {
		header: {
			resourceId: substratePalletResourceId,
			functionSignature: '0x00000000',
			nonce,
		},
		// TODO: this is a dummy call, should replace with actual call
		call: api.tx.system.remark('execute token remove proposal').toString(),
	};
	return tokenRemoveProposal;
};

export const getAnchorUpdateProposal = (api: ApiPromise) => {
	const anchorUpdateProposal: SubstrateProposal = {
		header: {
			resourceId: substratePalletResourceId,
			functionSignature: '0x00000000',
			nonce,
		},
		// TODO: this is a dummy call, should replace with actual call
		call: api.tx.system.remark('execute anchor update proposal').toString(),
	};
	return anchorUpdateProposal;
};

export const getAnchorCreateProposal = (api: ApiPromise) => {
	const anchorCreateProposal: SubstrateProposal = {
		header: {
			resourceId: substratePalletResourceId,
			functionSignature: '0x00000000',
			nonce,
		},
		// TODO: this is a dummy call, should replace with actual call
		call: api.tx.system.remark('execute anchor update proposal').toString(),
	};
	return anchorCreateProposal;
};

export const getResourceIdUpdateProposal = (api: ApiPromise) => {
	const resourceIdUpdateProposal: ResourceIdUpdateProposal = {
		header: {
			resourceId: substratePalletResourceId,
			functionSignature: '0x00000000',
			nonce,
		},
		resourceIdToRegister: substratePalletResourceId,
		methodName: 'execute_wrapping_fee_proposal',
	};
	return resourceIdUpdateProposal;
};
