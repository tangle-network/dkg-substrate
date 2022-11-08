// Auto-generated via `yarn polkadot-types-from-chain`, do not edit
/* eslint-disable */

// import type lookup before we augment - in some environments
// this is required to allow for ambient/previous definitions
import '@polkadot/api-base/types/events';

import type { ApiTypes, AugmentedEvent } from '@polkadot/api-base/types';
import type { Bytes, Null, Option, Result, Vec, bool, u128, u16, u32, u64 } from '@polkadot/types-codec';
import type { ITuple } from '@polkadot/types-codec/types';
import type { AccountId32, H256 } from '@polkadot/types/interfaces/runtime';
import type { DkgRuntimePrimitivesCryptoPublic, DkgRuntimePrimitivesMisbehaviourType, DkgRuntimePrimitivesProposalDkgPayloadKey, FrameSupportTokensMiscBalanceStatus, FrameSupportWeightsDispatchInfo, PalletElectionProviderMultiPhaseElectionCompute, PalletNominationPoolsPoolState, PalletStakingValidatorPrefs, SpFinalityGrandpaAppPublic, SpRuntimeDispatchError, WebbProposalsHeaderTypedChainId, WebbProposalsProposalProposalKind } from '@polkadot/types/lookup';

export type __AugmentedEvent<ApiType extends ApiTypes> = AugmentedEvent<ApiType>;

declare module '@polkadot/api-base/types/events' {
  interface AugmentedEvents<ApiType extends ApiTypes> {
    bagsList: {
      /**
       * Moved an account from one bag to another.
       **/
      Rebagged: AugmentedEvent<ApiType, [who: AccountId32, from: u64, to: u64], { who: AccountId32, from: u64, to: u64 }>;
      /**
       * Updated the score of some account to the given amount.
       **/
      ScoreUpdated: AugmentedEvent<ApiType, [who: AccountId32, newScore: u64], { who: AccountId32, newScore: u64 }>;
      /**
       * Generic RuntimeEvent
       **/
      [key: string]: AugmentedEvent<ApiType>;
    };
    balances: {
      /**
       * A balance was set by root.
       **/
      BalanceSet: AugmentedEvent<ApiType, [who: AccountId32, free: u128, reserved: u128], { who: AccountId32, free: u128, reserved: u128 }>;
      /**
       * Some amount was deposited (e.g. for transaction fees).
       **/
      Deposit: AugmentedEvent<ApiType, [who: AccountId32, amount: u128], { who: AccountId32, amount: u128 }>;
      /**
       * An account was removed whose balance was non-zero but below ExistentialDeposit,
       * resulting in an outright loss.
       **/
      DustLost: AugmentedEvent<ApiType, [account: AccountId32, amount: u128], { account: AccountId32, amount: u128 }>;
      /**
       * An account was created with some free balance.
       **/
      Endowed: AugmentedEvent<ApiType, [account: AccountId32, freeBalance: u128], { account: AccountId32, freeBalance: u128 }>;
      /**
       * Some balance was reserved (moved from free to reserved).
       **/
      Reserved: AugmentedEvent<ApiType, [who: AccountId32, amount: u128], { who: AccountId32, amount: u128 }>;
      /**
       * Some balance was moved from the reserve of the first account to the second account.
       * Final argument indicates the destination balance type.
       **/
      ReserveRepatriated: AugmentedEvent<ApiType, [from: AccountId32, to: AccountId32, amount: u128, destinationStatus: FrameSupportTokensMiscBalanceStatus], { from: AccountId32, to: AccountId32, amount: u128, destinationStatus: FrameSupportTokensMiscBalanceStatus }>;
      /**
       * Some amount was removed from the account (e.g. for misbehavior).
       **/
      Slashed: AugmentedEvent<ApiType, [who: AccountId32, amount: u128], { who: AccountId32, amount: u128 }>;
      /**
       * Transfer succeeded.
       **/
      Transfer: AugmentedEvent<ApiType, [from: AccountId32, to: AccountId32, amount: u128], { from: AccountId32, to: AccountId32, amount: u128 }>;
      /**
       * Some balance was unreserved (moved from reserved to free).
       **/
      Unreserved: AugmentedEvent<ApiType, [who: AccountId32, amount: u128], { who: AccountId32, amount: u128 }>;
      /**
       * Some amount was withdrawn from the account (e.g. for transaction fees).
       **/
      Withdraw: AugmentedEvent<ApiType, [who: AccountId32, amount: u128], { who: AccountId32, amount: u128 }>;
      /**
       * Generic RuntimeEvent
       **/
      [key: string]: AugmentedEvent<ApiType>;
    };
    bridgeRegistry: {
      /**
       * Generic RuntimeEvent
       **/
      [key: string]: AugmentedEvent<ApiType>;
    };
    dkg: {
      /**
       * Misbehaviour reports submitted
       **/
      MisbehaviourReportsSubmitted: AugmentedEvent<ApiType, [misbehaviourType: DkgRuntimePrimitivesMisbehaviourType, reporters: Vec<DkgRuntimePrimitivesCryptoPublic>], { misbehaviourType: DkgRuntimePrimitivesMisbehaviourType, reporters: Vec<DkgRuntimePrimitivesCryptoPublic> }>;
      /**
       * NextKeygenThreshold updated
       **/
      NextKeygenThresholdUpdated: AugmentedEvent<ApiType, [nextKeygenThreshold: u16], { nextKeygenThreshold: u16 }>;
      /**
       * Next public key signature submitted
       **/
      NextPublicKeySignatureSubmitted: AugmentedEvent<ApiType, [pubKeySig: Bytes, compressedPubKey: Bytes, uncompressedPubKey: Bytes], { pubKeySig: Bytes, compressedPubKey: Bytes, uncompressedPubKey: Bytes }>;
      /**
       * Next public key submitted
       **/
      NextPublicKeySubmitted: AugmentedEvent<ApiType, [compressedPubKey: Bytes, uncompressedPubKey: Bytes], { compressedPubKey: Bytes, uncompressedPubKey: Bytes }>;
      /**
       * NextSignatureThreshold updated
       **/
      NextSignatureThresholdUpdated: AugmentedEvent<ApiType, [nextSignatureThreshold: u16], { nextSignatureThreshold: u16 }>;
      /**
       * Current Public Key Changed.
       **/
      PublicKeyChanged: AugmentedEvent<ApiType, [compressedPubKey: Bytes, uncompressedPubKey: Bytes], { compressedPubKey: Bytes, uncompressedPubKey: Bytes }>;
      /**
       * Current Public Key Signature Changed.
       **/
      PublicKeySignatureChanged: AugmentedEvent<ApiType, [pubKeySig: Bytes, compressedPubKey: Bytes, uncompressedPubKey: Bytes], { pubKeySig: Bytes, compressedPubKey: Bytes, uncompressedPubKey: Bytes }>;
      /**
       * Current public key submitted
       **/
      PublicKeySubmitted: AugmentedEvent<ApiType, [compressedPubKey: Bytes, uncompressedPubKey: Bytes], { compressedPubKey: Bytes, uncompressedPubKey: Bytes }>;
      /**
       * Refresh DKG Keys Finished (forcefully).
       **/
      RefreshKeysFinished: AugmentedEvent<ApiType, [nextAuthoritySetId: u64], { nextAuthoritySetId: u64 }>;
      /**
       * Generic RuntimeEvent
       **/
      [key: string]: AugmentedEvent<ApiType>;
    };
    dkgProposalHandler: {
      /**
       * RuntimeEvent Emitted when we encounter a Proposal with invalid Signature.
       **/
      InvalidProposalSignature: AugmentedEvent<ApiType, [kind: WebbProposalsProposalProposalKind, data: Bytes, invalidSignature: Bytes, expectedPublicKey: Option<Bytes>, actualPublicKey: Option<Bytes>], { kind: WebbProposalsProposalProposalKind, data: Bytes, invalidSignature: Bytes, expectedPublicKey: Option<Bytes>, actualPublicKey: Option<Bytes> }>;
      /**
       * RuntimeEvent When a Proposal is added to UnsignedProposalQueue.
       **/
      ProposalAdded: AugmentedEvent<ApiType, [key: DkgRuntimePrimitivesProposalDkgPayloadKey, targetChain: WebbProposalsHeaderTypedChainId, data: Bytes], { key: DkgRuntimePrimitivesProposalDkgPayloadKey, targetChain: WebbProposalsHeaderTypedChainId, data: Bytes }>;
      /**
       * RuntimeEvent When a Proposal is removed to UnsignedProposalQueue.
       **/
      ProposalRemoved: AugmentedEvent<ApiType, [key: DkgRuntimePrimitivesProposalDkgPayloadKey, targetChain: WebbProposalsHeaderTypedChainId], { key: DkgRuntimePrimitivesProposalDkgPayloadKey, targetChain: WebbProposalsHeaderTypedChainId }>;
      /**
       * RuntimeEvent When a Proposal Gets Signed by DKG.
       **/
      ProposalSigned: AugmentedEvent<ApiType, [key: DkgRuntimePrimitivesProposalDkgPayloadKey, targetChain: WebbProposalsHeaderTypedChainId, data: Bytes, signature: Bytes], { key: DkgRuntimePrimitivesProposalDkgPayloadKey, targetChain: WebbProposalsHeaderTypedChainId, data: Bytes, signature: Bytes }>;
      /**
       * Generic RuntimeEvent
       **/
      [key: string]: AugmentedEvent<ApiType>;
    };
    dkgProposals: {
      /**
       * Proposers have been reset
       **/
      AuthorityProposersReset: AugmentedEvent<ApiType, [proposers: Vec<AccountId32>], { proposers: Vec<AccountId32> }>;
      /**
       * Chain now available for transfers (chain_id)
       **/
      ChainWhitelisted: AugmentedEvent<ApiType, [chainId: WebbProposalsHeaderTypedChainId], { chainId: WebbProposalsHeaderTypedChainId }>;
      /**
       * Voting successful for a proposal
       **/
      ProposalApproved: AugmentedEvent<ApiType, [chainId: WebbProposalsHeaderTypedChainId, proposalNonce: u32], { chainId: WebbProposalsHeaderTypedChainId, proposalNonce: u32 }>;
      /**
       * Execution of call failed
       **/
      ProposalFailed: AugmentedEvent<ApiType, [chainId: WebbProposalsHeaderTypedChainId, proposalNonce: u32], { chainId: WebbProposalsHeaderTypedChainId, proposalNonce: u32 }>;
      /**
       * Voting rejected a proposal
       **/
      ProposalRejected: AugmentedEvent<ApiType, [chainId: WebbProposalsHeaderTypedChainId, proposalNonce: u32], { chainId: WebbProposalsHeaderTypedChainId, proposalNonce: u32 }>;
      /**
       * Execution of call succeeded
       **/
      ProposalSucceeded: AugmentedEvent<ApiType, [chainId: WebbProposalsHeaderTypedChainId, proposalNonce: u32], { chainId: WebbProposalsHeaderTypedChainId, proposalNonce: u32 }>;
      /**
       * Proposer added to set
       **/
      ProposerAdded: AugmentedEvent<ApiType, [proposerId: AccountId32], { proposerId: AccountId32 }>;
      /**
       * Proposer removed from set
       **/
      ProposerRemoved: AugmentedEvent<ApiType, [proposerId: AccountId32], { proposerId: AccountId32 }>;
      /**
       * Vote threshold has changed (new_threshold)
       **/
      ProposerThresholdChanged: AugmentedEvent<ApiType, [newThreshold: u32], { newThreshold: u32 }>;
      /**
       * Vot submitted against proposal
       **/
      VoteAgainst: AugmentedEvent<ApiType, [chainId: WebbProposalsHeaderTypedChainId, proposalNonce: u32, who: AccountId32], { chainId: WebbProposalsHeaderTypedChainId, proposalNonce: u32, who: AccountId32 }>;
      /**
       * Vote submitted in favour of proposal
       **/
      VoteFor: AugmentedEvent<ApiType, [chainId: WebbProposalsHeaderTypedChainId, proposalNonce: u32, who: AccountId32], { chainId: WebbProposalsHeaderTypedChainId, proposalNonce: u32, who: AccountId32 }>;
      /**
       * Generic RuntimeEvent
       **/
      [key: string]: AugmentedEvent<ApiType>;
    };
    electionProviderMultiPhase: {
      /**
       * The election has been finalized, with `Some` of the given computation, or else if the
       * election failed, `None`.
       **/
      ElectionFinalized: AugmentedEvent<ApiType, [electionCompute: Option<PalletElectionProviderMultiPhaseElectionCompute>], { electionCompute: Option<PalletElectionProviderMultiPhaseElectionCompute> }>;
      /**
       * An account has been rewarded for their signed submission being finalized.
       **/
      Rewarded: AugmentedEvent<ApiType, [account: AccountId32, value: u128], { account: AccountId32, value: u128 }>;
      /**
       * The signed phase of the given round has started.
       **/
      SignedPhaseStarted: AugmentedEvent<ApiType, [round: u32], { round: u32 }>;
      /**
       * An account has been slashed for submitting an invalid signed submission.
       **/
      Slashed: AugmentedEvent<ApiType, [account: AccountId32, value: u128], { account: AccountId32, value: u128 }>;
      /**
       * A solution was stored with the given compute.
       * 
       * If the solution is signed, this means that it hasn't yet been processed. If the
       * solution is unsigned, this means that it has also been processed.
       * 
       * The `bool` is `true` when a previous solution was ejected to make room for this one.
       **/
      SolutionStored: AugmentedEvent<ApiType, [electionCompute: PalletElectionProviderMultiPhaseElectionCompute, prevEjected: bool], { electionCompute: PalletElectionProviderMultiPhaseElectionCompute, prevEjected: bool }>;
      /**
       * The unsigned phase of the given round has started.
       **/
      UnsignedPhaseStarted: AugmentedEvent<ApiType, [round: u32], { round: u32 }>;
      /**
       * Generic RuntimeEvent
       **/
      [key: string]: AugmentedEvent<ApiType>;
    };
    grandpa: {
      /**
       * New authority set has been applied.
       **/
      NewAuthorities: AugmentedEvent<ApiType, [authoritySet: Vec<ITuple<[SpFinalityGrandpaAppPublic, u64]>>], { authoritySet: Vec<ITuple<[SpFinalityGrandpaAppPublic, u64]>> }>;
      /**
       * Current authority set has been paused.
       **/
      Paused: AugmentedEvent<ApiType, []>;
      /**
       * Current authority set has been resumed.
       **/
      Resumed: AugmentedEvent<ApiType, []>;
      /**
       * Generic RuntimeEvent
       **/
      [key: string]: AugmentedEvent<ApiType>;
    };
    indices: {
      /**
       * A account index was assigned.
       **/
      IndexAssigned: AugmentedEvent<ApiType, [who: AccountId32, index: u32], { who: AccountId32, index: u32 }>;
      /**
       * A account index has been freed up (unassigned).
       **/
      IndexFreed: AugmentedEvent<ApiType, [index: u32], { index: u32 }>;
      /**
       * A account index has been frozen to its current account ID.
       **/
      IndexFrozen: AugmentedEvent<ApiType, [index: u32, who: AccountId32], { index: u32, who: AccountId32 }>;
      /**
       * Generic RuntimeEvent
       **/
      [key: string]: AugmentedEvent<ApiType>;
    };
    nominationPools: {
      /**
       * A member has became bonded in a pool.
       **/
      Bonded: AugmentedEvent<ApiType, [member: AccountId32, poolId: u32, bonded: u128, joined: bool], { member: AccountId32, poolId: u32, bonded: u128, joined: bool }>;
      /**
       * A pool has been created.
       **/
      Created: AugmentedEvent<ApiType, [depositor: AccountId32, poolId: u32], { depositor: AccountId32, poolId: u32 }>;
      /**
       * A pool has been destroyed.
       **/
      Destroyed: AugmentedEvent<ApiType, [poolId: u32], { poolId: u32 }>;
      /**
       * A member has been removed from a pool.
       * 
       * The removal can be voluntary (withdrawn all unbonded funds) or involuntary (kicked).
       **/
      MemberRemoved: AugmentedEvent<ApiType, [poolId: u32, member: AccountId32], { poolId: u32, member: AccountId32 }>;
      /**
       * A payout has been made to a member.
       **/
      PaidOut: AugmentedEvent<ApiType, [member: AccountId32, poolId: u32, payout: u128], { member: AccountId32, poolId: u32, payout: u128 }>;
      /**
       * The active balance of pool `pool_id` has been slashed to `balance`.
       **/
      PoolSlashed: AugmentedEvent<ApiType, [poolId: u32, balance: u128], { poolId: u32, balance: u128 }>;
      /**
       * The roles of a pool have been updated to the given new roles. Note that the depositor
       * can never change.
       **/
      RolesUpdated: AugmentedEvent<ApiType, [root: Option<AccountId32>, stateToggler: Option<AccountId32>, nominator: Option<AccountId32>], { root: Option<AccountId32>, stateToggler: Option<AccountId32>, nominator: Option<AccountId32> }>;
      /**
       * The state of a pool has changed
       **/
      StateChanged: AugmentedEvent<ApiType, [poolId: u32, newState: PalletNominationPoolsPoolState], { poolId: u32, newState: PalletNominationPoolsPoolState }>;
      /**
       * A member has unbonded from their pool.
       * 
       * - `balance` is the corresponding balance of the number of points that has been
       * requested to be unbonded (the argument of the `unbond` transaction) from the bonded
       * pool.
       * - `points` is the number of points that are issued as a result of `balance` being
       * dissolved into the corresponding unbonding pool.
       * - `era` is the era in which the balance will be unbonded.
       * In the absence of slashing, these values will match. In the presence of slashing, the
       * number of points that are issued in the unbonding pool will be less than the amount
       * requested to be unbonded.
       **/
      Unbonded: AugmentedEvent<ApiType, [member: AccountId32, poolId: u32, balance: u128, points: u128, era: u32], { member: AccountId32, poolId: u32, balance: u128, points: u128, era: u32 }>;
      /**
       * The unbond pool at `era` of pool `pool_id` has been slashed to `balance`.
       **/
      UnbondingPoolSlashed: AugmentedEvent<ApiType, [poolId: u32, era: u32, balance: u128], { poolId: u32, era: u32, balance: u128 }>;
      /**
       * A member has withdrawn from their pool.
       * 
       * The given number of `points` have been dissolved in return of `balance`.
       * 
       * Similar to `Unbonded` RuntimeEvent, in the absence of slashing, the ratio of point to balance
       * will be 1.
       **/
      Withdrawn: AugmentedEvent<ApiType, [member: AccountId32, poolId: u32, balance: u128, points: u128], { member: AccountId32, poolId: u32, balance: u128, points: u128 }>;
      /**
       * Generic RuntimeEvent
       **/
      [key: string]: AugmentedEvent<ApiType>;
    };
    session: {
      /**
       * New session has happened. Note that the argument is the session index, not the
       * block number as the type might suggest.
       **/
      NewSession: AugmentedEvent<ApiType, [sessionIndex: u32], { sessionIndex: u32 }>;
      /**
       * Generic RuntimeEvent
       **/
      [key: string]: AugmentedEvent<ApiType>;
    };
    staking: {
      /**
       * An account has bonded this amount. \[stash, amount\]
       * 
       * NOTE: This RuntimeEvent is only emitted when funds are bonded via a dispatchable. Notably,
       * it will not be emitted for staking rewards when they are added to stake.
       **/
      Bonded: AugmentedEvent<ApiType, [AccountId32, u128]>;
      /**
       * An account has stopped participating as either a validator or nominator.
       * \[stash\]
       **/
      Chilled: AugmentedEvent<ApiType, [AccountId32]>;
      /**
       * The era payout has been set; the first balance is the validator-payout; the second is
       * the remainder from the maximum amount of reward.
       * \[era_index, validator_payout, remainder\]
       **/
      EraPaid: AugmentedEvent<ApiType, [u32, u128, u128]>;
      /**
       * A nominator has been kicked from a validator. \[nominator, stash\]
       **/
      Kicked: AugmentedEvent<ApiType, [AccountId32, AccountId32]>;
      /**
       * An old slashing report from a prior era was discarded because it could
       * not be processed. \[session_index\]
       **/
      OldSlashingReportDiscarded: AugmentedEvent<ApiType, [u32]>;
      /**
       * The stakers' rewards are getting paid. \[era_index, validator_stash\]
       **/
      PayoutStarted: AugmentedEvent<ApiType, [u32, AccountId32]>;
      /**
       * The nominator has been rewarded by this amount. \[stash, amount\]
       **/
      Rewarded: AugmentedEvent<ApiType, [AccountId32, u128]>;
      /**
       * One validator (and its nominators) has been slashed by the given amount.
       * \[validator, amount\]
       **/
      Slashed: AugmentedEvent<ApiType, [AccountId32, u128]>;
      /**
       * A new set of stakers was elected.
       **/
      StakersElected: AugmentedEvent<ApiType, []>;
      /**
       * The election failed. No new era is planned.
       **/
      StakingElectionFailed: AugmentedEvent<ApiType, []>;
      /**
       * An account has unbonded this amount. \[stash, amount\]
       **/
      Unbonded: AugmentedEvent<ApiType, [AccountId32, u128]>;
      /**
       * A validator has set their preferences.
       **/
      ValidatorPrefsSet: AugmentedEvent<ApiType, [AccountId32, PalletStakingValidatorPrefs]>;
      /**
       * An account has called `withdraw_unbonded` and removed unbonding chunks worth `Balance`
       * from the unlocking queue. \[stash, amount\]
       **/
      Withdrawn: AugmentedEvent<ApiType, [AccountId32, u128]>;
      /**
       * Generic RuntimeEvent
       **/
      [key: string]: AugmentedEvent<ApiType>;
    };
    sudo: {
      /**
       * The \[sudoer\] just switched identity; the old key is supplied if one existed.
       **/
      KeyChanged: AugmentedEvent<ApiType, [oldSudoer: Option<AccountId32>], { oldSudoer: Option<AccountId32> }>;
      /**
       * A sudo just took place. \[result\]
       **/
      Sudid: AugmentedEvent<ApiType, [sudoResult: Result<Null, SpRuntimeDispatchError>], { sudoResult: Result<Null, SpRuntimeDispatchError> }>;
      /**
       * A sudo just took place. \[result\]
       **/
      SudoAsDone: AugmentedEvent<ApiType, [sudoResult: Result<Null, SpRuntimeDispatchError>], { sudoResult: Result<Null, SpRuntimeDispatchError> }>;
      /**
       * Generic RuntimeEvent
       **/
      [key: string]: AugmentedEvent<ApiType>;
    };
    system: {
      /**
       * `:code` was updated.
       **/
      CodeUpdated: AugmentedEvent<ApiType, []>;
      /**
       * An extrinsic failed.
       **/
      ExtrinsicFailed: AugmentedEvent<ApiType, [dispatchError: SpRuntimeDispatchError, dispatchInfo: FrameSupportWeightsDispatchInfo], { dispatchError: SpRuntimeDispatchError, dispatchInfo: FrameSupportWeightsDispatchInfo }>;
      /**
       * An extrinsic completed successfully.
       **/
      ExtrinsicSuccess: AugmentedEvent<ApiType, [dispatchInfo: FrameSupportWeightsDispatchInfo], { dispatchInfo: FrameSupportWeightsDispatchInfo }>;
      /**
       * An account was reaped.
       **/
      KilledAccount: AugmentedEvent<ApiType, [account: AccountId32], { account: AccountId32 }>;
      /**
       * A new account was created.
       **/
      NewAccount: AugmentedEvent<ApiType, [account: AccountId32], { account: AccountId32 }>;
      /**
       * On on-chain remark happened.
       **/
      Remarked: AugmentedEvent<ApiType, [sender: AccountId32, hash_: H256], { sender: AccountId32, hash_: H256 }>;
      /**
       * Generic RuntimeEvent
       **/
      [key: string]: AugmentedEvent<ApiType>;
    };
    transactionPayment: {
      /**
       * A transaction fee `actual_fee`, of which `tip` was added to the minimum inclusion fee,
       * has been paid by `who`.
       **/
      TransactionFeePaid: AugmentedEvent<ApiType, [who: AccountId32, actualFee: u128, tip: u128], { who: AccountId32, actualFee: u128, tip: u128 }>;
      /**
       * Generic RuntimeEvent
       **/
      [key: string]: AugmentedEvent<ApiType>;
    };
  } // AugmentedEvents
} // declare module
