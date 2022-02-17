import { ApiPromise, WsProvider } from '@polkadot/api';
const provider = new WsProvider('ws://127.0.0.1:9944');
export const apiProposalTypes = function () {
	return ApiPromise.create({
		provider,
		types: {
			ProposalType: {
				_enum: [
					'EVMUnsigned',
					'EVMSigned',
					'AnchorUpdate',
					'AnchorUpdateSigned',
					'TokenUpdate',
					'TokenUpdateSigned',
					'WrappingFeeUpdate',
					'WrappingFeeUpdateSigned',
					'MaxDepositLimitUpdate',
					'MaxDepositLimitUpdateSigned',
					'MinWithdrawalLimitUpdate',
					'MinWithdrawalLimitUpdateSigned',
					'MaxExtLimitUpdate',
					'MaxExtLimitUpdateSigned',
					'MaxFeeLimitUpdate',
					'MaxFeeLimitUpdateSigned',
				],
			},
			EVMUnsigned: {
				data: 'Vec<u8>',
			},
			EVMSigned: {
				data: 'Vec<u8>',
				signature: 'Vec<u8>',
			},
			AnchorUpdate: {
				data: 'Vec<u8>',
			},
			AnchorUpdateSigned: {
				data: 'Vec<u8>',
				signature: 'Vec<u8>',
			},
			TokenUpdate: {
				data: 'Vec<u8>',
			},
			TokenUpdateSigned: {
				data: 'Vec<u8>',
				signature: 'Vec<u8>',
			},
			WrappingFeeUpdate: {
				data: 'Vec<u8>',
			},
			WrappingFeeUpdateSigned: {
				data: 'Vec<u8>',
				signature: 'Vec<u8>',
			},
			MaxDepositLimitUpdate: {
				data: 'Vec<u8>',
			},
			MaxDepositLimitUpdateSigned: {
				data: 'Vec<u8>',
				signature: 'Vec<u8>',
			},
			MinWithdrawalLimitUpdate: {
				data: 'Vec<u8>',
			},
			MinWithdrawalLimitUpdateSigned: {
				data: 'Vec<u8>',
				signature: 'Vec<u8>',
			},
			MaxExtLimitUpdate: {
				data: 'Vec<u8>',
			},
			MaxExtLimitUpdateSigned: {
				data: 'Vec<u8>',
				signature: 'Vec<u8>',
			},
			MaxFeeLimitUpdate: {
				data: 'Vec<u8>',
			},
			MaxFeeLimitUpdateSigned: {
				data: 'Vec<u8>',
				signature: 'Vec<u8>',
			},
		},
	});
};
