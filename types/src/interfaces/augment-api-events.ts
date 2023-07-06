// Auto-generated via `yarn polkadot-types-from-chain`, do not edit
/* eslint-disable */

// import type lookup before we augment - in some environments
// this is required to allow for ambient/previous definitions
import '@polkadot/api-base/types/events';

import type { ApiTypes, AugmentedEvent } from '@polkadot/api-base/types';
import type { Bytes, Null, Option, Result, Vec, bool, u128, u16, u32, u64 } from '@polkadot/types-codec';
import type { ITuple } from '@polkadot/types-codec/types';
import type { AccountId32, H256, Perbill } from '@polkadot/types/interfaces/runtime';
import type {FrameSupportTokensMiscBalanceStatus,SpVersionRuntimeVersion,SpWeightsRuntimeDbWeight,FrameSystemLimitsBlockWeights,FrameSystemLimitsBlockLength, SpRuntimeDispatchError,SpFinalityGrandpaAppPublic,PalletElectionProviderMultiPhaseElectionCompute,WebbProposalsProposalProposalKind,FrameSupportDispatchDispatchInfo,PalletBagsListListNode, PalletBalancesReserveData,PalletBalancesAccountData, DkgRuntimePrimitivesMisbehaviourType, PalletDkgProposalsProposalVotes, PalletElectionProviderMultiPhasePhase, PalletElectionProviderMultiPhaseReadySolution, PalletGrandpaStoredState, PalletIdentityRegistration, PalletGrandpaStoredPendingChange, PalletElectionProviderMultiPhaseRoundSnapshot, PalletImOnlineSr25519AppSr25519Public, PalletNominationPoolsPoolMember, PalletNominationPoolsBondedPoolInner, PalletNominationPoolsRewardPool, SpCoreCryptoKeyTypeId, PalletStakingActiveEraInfo, PalletStakingEraRewardPoints, PalletStakingExposure, PalletStakingForcing, PalletStakingStakingLedger, PalletStakingNominations, PalletStakingSlashingSlashingSpans, FrameSystemAccountInfo, FrameSystemPhase, PalletTransactionPaymentReleases,PalletStakingSlashingSpanRecord, SpConsensusAuraSr25519AppSr25519Public, FrameSystemLastRuntimeUpgradeInfo,PalletStakingUnappliedSlash,  FrameSystemEventRecord, SpRuntimeDigest, FrameSupportDispatchPerDispatchClassWeight, DkgRuntimePrimitivesCryptoPublic,DkgRuntimePrimitivesAggregatedMisbehaviourReports,DkgRuntimePrimitivesProposalDkgPayloadKey, WebbProposalsProposal,WebbProposalsHeaderResourceId, WebbProposalsHeaderTypedChainId,SpNposElectionsSupport,SpNposElectionsElectionScore,PalletElectionProviderMultiPhaseRawSolution,SpWeightsWeightV2Weight,PalletNominationPoolsBondExtra,PalletIdentityJudgement,SpCoreVoid,SpFinalityGrandpaEquivocationProof,PalletElectionProviderMultiPhaseSolutionOrSnapshotSize,PalletImOnlineHeartbeat,PalletIdentityIdentityInfo,PalletIdentityBitFlags,PalletImOnlineSr25519AppSr25519Signature, PalletStakingValidatorPrefs,PalletNominationPoolsClaimPermission,PalletNominationPoolsConfigOpAccountId32,PalletNominationPoolsPoolState, PalletStakingPalletConfigOpPerbill,PalletStakingRewardDestination, DkgStandaloneRuntimeOpaqueSessionKeys, PalletStakingPalletConfigOpU32, PalletStakingPalletConfigOpU128, PalletStakingPalletConfigOpPercent, PalletNominationPoolsConfigOpU32, PalletNominationPoolsConfigOpU128} from "./types-lookup";


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
       * Generic event
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
       * Generic event
       **/
      [key: string]: AugmentedEvent<ApiType>;
    };
    bridgeRegistry: {
      /**
       * Generic event
       **/
      [key: string]: AugmentedEvent<ApiType>;
    };
    dkg: {
      /**
       * An authority has been jailed for misbehaviour
       **/
      AuthorityJailed: AugmentedEvent<ApiType, [misbehaviourType: DkgRuntimePrimitivesMisbehaviourType, authority: DkgRuntimePrimitivesCryptoPublic], { misbehaviourType: DkgRuntimePrimitivesMisbehaviourType, authority: DkgRuntimePrimitivesCryptoPublic }>;
      /**
       * An authority has been unjailed
       **/
      AuthorityUnJailed: AugmentedEvent<ApiType, [authority: DkgRuntimePrimitivesCryptoPublic], { authority: DkgRuntimePrimitivesCryptoPublic }>;
      /**
       * An Emergency Keygen Protocol was triggered.
       **/
      EmergencyKeygenTriggered: AugmentedEvent<ApiType, []>;
      /**
       * Misbehaviour reports submitted
       **/
      MisbehaviourReportsSubmitted: AugmentedEvent<ApiType, [misbehaviourType: DkgRuntimePrimitivesMisbehaviourType, reporters: Vec<DkgRuntimePrimitivesCryptoPublic>, offender: DkgRuntimePrimitivesCryptoPublic], { misbehaviourType: DkgRuntimePrimitivesMisbehaviourType, reporters: Vec<DkgRuntimePrimitivesCryptoPublic>, offender: DkgRuntimePrimitivesCryptoPublic }>;
      /**
       * NextKeygenThreshold updated
       **/
      NextKeygenThresholdUpdated: AugmentedEvent<ApiType, [nextKeygenThreshold: u16], { nextKeygenThreshold: u16 }>;
      /**
       * Next public key signature submitted
       **/
      NextPublicKeySignatureSubmitted: AugmentedEvent<ApiType, [pubKeySig: Bytes, compressedPubKey: Bytes, uncompressedPubKey: Bytes, nonce: u32], { pubKeySig: Bytes, compressedPubKey: Bytes, uncompressedPubKey: Bytes, nonce: u32 }>;
      /**
       * Next public key submitted
       **/
      NextPublicKeySubmitted: AugmentedEvent<ApiType, [compressedPubKey: Bytes, uncompressedPubKey: Bytes], { compressedPubKey: Bytes, uncompressedPubKey: Bytes }>;
      /**
       * NextSignatureThreshold updated
       **/
      NextSignatureThresholdUpdated: AugmentedEvent<ApiType, [nextSignatureThreshold: u16], { nextSignatureThreshold: u16 }>;
      /**
       * PendingKeygenThreshold updated
       **/
      PendingKeygenThresholdUpdated: AugmentedEvent<ApiType, [pendingKeygenThreshold: u16], { pendingKeygenThreshold: u16 }>;
      /**
       * PendingSignatureThreshold updated
       **/
      PendingSignatureThresholdUpdated: AugmentedEvent<ApiType, [pendingSignatureThreshold: u16], { pendingSignatureThreshold: u16 }>;
      /**
       * Current Public Key Changed.
       **/
      PublicKeyChanged: AugmentedEvent<ApiType, [compressedPubKey: Bytes, uncompressedPubKey: Bytes], { compressedPubKey: Bytes, uncompressedPubKey: Bytes }>;
      /**
       * Current Public Key Signature Changed.
       **/
      PublicKeySignatureChanged: AugmentedEvent<ApiType, [pubKeySig: Bytes, compressedPubKey: Bytes, uncompressedPubKey: Bytes, nonce: u32], { pubKeySig: Bytes, compressedPubKey: Bytes, uncompressedPubKey: Bytes, nonce: u32 }>;
      /**
       * Current public key submitted
       **/
      PublicKeySubmitted: AugmentedEvent<ApiType, [compressedPubKey: Bytes, uncompressedPubKey: Bytes], { compressedPubKey: Bytes, uncompressedPubKey: Bytes }>;
      /**
       * Refresh DKG Keys Finished (forcefully).
       **/
      RefreshKeysFinished: AugmentedEvent<ApiType, [nextAuthoritySetId: u64], { nextAuthoritySetId: u64 }>;
      /**
       * Generic event
       **/
      [key: string]: AugmentedEvent<ApiType>;
    };
    dkgProposalHandler: {
      /**
       * RuntimeEvent Emitted when we encounter a Proposal with invalid Signature.
       **/
      InvalidProposalBatchSignature: AugmentedEvent<ApiType, [proposals: DkgRuntimePrimitivesProposalSignedProposalBatch, data: Bytes, invalidSignature: Bytes, expectedPublicKey: Option<Bytes>, actualPublicKey: Option<Bytes>], { proposals: DkgRuntimePrimitivesProposalSignedProposalBatch, data: Bytes, invalidSignature: Bytes, expectedPublicKey: Option<Bytes>, actualPublicKey: Option<Bytes> }>;
      /**
       * RuntimeEvent When a Proposal is added to UnsignedProposalQueue.
       **/
      ProposalAdded: AugmentedEvent<ApiType, [key: DkgRuntimePrimitivesProposalDkgPayloadKey, targetChain: WebbProposalsHeaderTypedChainId, data: Bytes], { key: DkgRuntimePrimitivesProposalDkgPayloadKey, targetChain: WebbProposalsHeaderTypedChainId, data: Bytes }>;
      /**
       * RuntimeEvent When a Proposal Gets Signed by DKG.
       **/
      ProposalBatchSigned: AugmentedEvent<ApiType, [targetChain: WebbProposalsHeaderTypedChainId, proposals: DkgRuntimePrimitivesProposalSignedProposalBatch, data: Bytes, signature: Bytes], { targetChain: WebbProposalsHeaderTypedChainId, proposals: DkgRuntimePrimitivesProposalSignedProposalBatch, data: Bytes, signature: Bytes }>;
      /**
       * RuntimeEvent When a Proposal is removed from UnsignedProposalQueue.
       **/
      ProposalRemoved: AugmentedEvent<ApiType, [key: DkgRuntimePrimitivesProposalDkgPayloadKey, targetChain: WebbProposalsHeaderTypedChainId, expired: bool], { key: DkgRuntimePrimitivesProposalDkgPayloadKey, targetChain: WebbProposalsHeaderTypedChainId, expired: bool }>;
      /**
       * Generic event
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
      ProposalApproved: AugmentedEvent<ApiType, [kind: WebbProposalsProposalProposalKind, srcChainId: WebbProposalsHeaderTypedChainId, proposalNonce: u32], { kind: WebbProposalsProposalProposalKind, srcChainId: WebbProposalsHeaderTypedChainId, proposalNonce: u32 }>;
      /**
       * Execution of call failed
       **/
      ProposalFailed: AugmentedEvent<ApiType, [kind: WebbProposalsProposalProposalKind, srcChainId: WebbProposalsHeaderTypedChainId, proposalNonce: u32], { kind: WebbProposalsProposalProposalKind, srcChainId: WebbProposalsHeaderTypedChainId, proposalNonce: u32 }>;
      /**
       * Voting rejected a proposal
       **/
      ProposalRejected: AugmentedEvent<ApiType, [kind: WebbProposalsProposalProposalKind, srcChainId: WebbProposalsHeaderTypedChainId, proposalNonce: u32], { kind: WebbProposalsProposalProposalKind, srcChainId: WebbProposalsHeaderTypedChainId, proposalNonce: u32 }>;
      /**
       * Execution of call succeeded
       **/
      ProposalSucceeded: AugmentedEvent<ApiType, [kind: WebbProposalsProposalProposalKind, srcChainId: WebbProposalsHeaderTypedChainId, proposalNonce: u32], { kind: WebbProposalsProposalProposalKind, srcChainId: WebbProposalsHeaderTypedChainId, proposalNonce: u32 }>;
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
      VoteAgainst: AugmentedEvent<ApiType, [kind: WebbProposalsProposalProposalKind, srcChainId: WebbProposalsHeaderTypedChainId, proposalNonce: u32, who: AccountId32], { kind: WebbProposalsProposalProposalKind, srcChainId: WebbProposalsHeaderTypedChainId, proposalNonce: u32, who: AccountId32 }>;
      /**
       * Vote submitted in favour of proposal
       **/
      VoteFor: AugmentedEvent<ApiType, [kind: WebbProposalsProposalProposalKind, srcChainId: WebbProposalsHeaderTypedChainId, proposalNonce: u32, who: AccountId32], { kind: WebbProposalsProposalProposalKind, srcChainId: WebbProposalsHeaderTypedChainId, proposalNonce: u32, who: AccountId32 }>;
      /**
       * Generic event
       **/
      [key: string]: AugmentedEvent<ApiType>;
    };
    electionProviderMultiPhase: {
      /**
       * An election failed.
       * 
       * Not much can be said about which computes failed in the process.
       **/
      ElectionFailed: AugmentedEvent<ApiType, []>;
      /**
       * The election has been finalized, with the given computation and score.
       **/
      ElectionFinalized: AugmentedEvent<ApiType, [compute: PalletElectionProviderMultiPhaseElectionCompute, score: SpNposElectionsElectionScore], { compute: PalletElectionProviderMultiPhaseElectionCompute, score: SpNposElectionsElectionScore }>;
      /**
       * There was a phase transition in a given round.
       **/
      PhaseTransitioned: AugmentedEvent<ApiType, [from: PalletElectionProviderMultiPhasePhase, to: PalletElectionProviderMultiPhasePhase, round: u32], { from: PalletElectionProviderMultiPhasePhase, to: PalletElectionProviderMultiPhasePhase, round: u32 }>;
      /**
       * An account has been rewarded for their signed submission being finalized.
       **/
      Rewarded: AugmentedEvent<ApiType, [account: AccountId32, value: u128], { account: AccountId32, value: u128 }>;
      /**
       * An account has been slashed for submitting an invalid signed submission.
       **/
      Slashed: AugmentedEvent<ApiType, [account: AccountId32, value: u128], { account: AccountId32, value: u128 }>;
      /**
       * A solution was stored with the given compute.
       * 
       * The `origin` indicates the origin of the solution. If `origin` is `Some(AccountId)`,
       * the stored solution was submited in the signed phase by a miner with the `AccountId`.
       * Otherwise, the solution was stored either during the unsigned phase or by
       * `T::ForceOrigin`. The `bool` is `true` when a previous solution was ejected to make
       * room for this one.
       **/
      SolutionStored: AugmentedEvent<ApiType, [compute: PalletElectionProviderMultiPhaseElectionCompute, origin: Option<AccountId32>, prevEjected: bool], { compute: PalletElectionProviderMultiPhaseElectionCompute, origin: Option<AccountId32>, prevEjected: bool }>;
      /**
       * Generic event
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
       * Generic event
       **/
      [key: string]: AugmentedEvent<ApiType>;
    };
    identity: {
      /**
       * A name was cleared, and the given balance returned.
       **/
      IdentityCleared: AugmentedEvent<ApiType, [who: AccountId32, deposit: u128], { who: AccountId32, deposit: u128 }>;
      /**
       * A name was removed and the given balance slashed.
       **/
      IdentityKilled: AugmentedEvent<ApiType, [who: AccountId32, deposit: u128], { who: AccountId32, deposit: u128 }>;
      /**
       * A name was set or reset (which will remove all judgements).
       **/
      IdentitySet: AugmentedEvent<ApiType, [who: AccountId32], { who: AccountId32 }>;
      /**
       * A judgement was given by a registrar.
       **/
      JudgementGiven: AugmentedEvent<ApiType, [target: AccountId32, registrarIndex: u32], { target: AccountId32, registrarIndex: u32 }>;
      /**
       * A judgement was asked from a registrar.
       **/
      JudgementRequested: AugmentedEvent<ApiType, [who: AccountId32, registrarIndex: u32], { who: AccountId32, registrarIndex: u32 }>;
      /**
       * A judgement request was retracted.
       **/
      JudgementUnrequested: AugmentedEvent<ApiType, [who: AccountId32, registrarIndex: u32], { who: AccountId32, registrarIndex: u32 }>;
      /**
       * A registrar was added.
       **/
      RegistrarAdded: AugmentedEvent<ApiType, [registrarIndex: u32], { registrarIndex: u32 }>;
      /**
       * A sub-identity was added to an identity and the deposit paid.
       **/
      SubIdentityAdded: AugmentedEvent<ApiType, [sub: AccountId32, main: AccountId32, deposit: u128], { sub: AccountId32, main: AccountId32, deposit: u128 }>;
      /**
       * A sub-identity was removed from an identity and the deposit freed.
       **/
      SubIdentityRemoved: AugmentedEvent<ApiType, [sub: AccountId32, main: AccountId32, deposit: u128], { sub: AccountId32, main: AccountId32, deposit: u128 }>;
      /**
       * A sub-identity was cleared, and the given deposit repatriated from the
       * main identity account to the sub-identity account.
       **/
      SubIdentityRevoked: AugmentedEvent<ApiType, [sub: AccountId32, main: AccountId32, deposit: u128], { sub: AccountId32, main: AccountId32, deposit: u128 }>;
      /**
       * Generic event
       **/
      [key: string]: AugmentedEvent<ApiType>;
    };
    imOnline: {
      /**
       * At the end of the session, no offence was committed.
       **/
      AllGood: AugmentedEvent<ApiType, []>;
      /**
       * A new heartbeat was received from `AuthorityId`.
       **/
      HeartbeatReceived: AugmentedEvent<ApiType, [authorityId: PalletImOnlineSr25519AppSr25519Public], { authorityId: PalletImOnlineSr25519AppSr25519Public }>;
      /**
       * At the end of the session, at least one validator was found to be offline.
       **/
      SomeOffline: AugmentedEvent<ApiType, [offline: Vec<ITuple<[AccountId32, PalletStakingExposure]>>], { offline: Vec<ITuple<[AccountId32, PalletStakingExposure]>> }>;
      /**
       * Generic event
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
       * Generic event
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
      RolesUpdated: AugmentedEvent<ApiType, [root: Option<AccountId32>, bouncer: Option<AccountId32>, nominator: Option<AccountId32>], { root: Option<AccountId32>, bouncer: Option<AccountId32>, nominator: Option<AccountId32> }>;
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
       * Similar to `Unbonded` event, in the absence of slashing, the ratio of point to balance
       * will be 1.
       **/
      Withdrawn: AugmentedEvent<ApiType, [member: AccountId32, poolId: u32, balance: u128, points: u128], { member: AccountId32, poolId: u32, balance: u128, points: u128 }>;
      /**
       * Generic event
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
       * Generic event
       **/
      [key: string]: AugmentedEvent<ApiType>;
    };
    staking: {
      /**
       * An account has bonded this amount. \[stash, amount\]
       * 
       * NOTE: This event is only emitted when funds are bonded via a dispatchable. Notably,
       * it will not be emitted for staking rewards when they are added to stake.
       **/
      Bonded: AugmentedEvent<ApiType, [stash: AccountId32, amount: u128], { stash: AccountId32, amount: u128 }>;
      /**
       * An account has stopped participating as either a validator or nominator.
       **/
      Chilled: AugmentedEvent<ApiType, [stash: AccountId32], { stash: AccountId32 }>;
      /**
       * The era payout has been set; the first balance is the validator-payout; the second is
       * the remainder from the maximum amount of reward.
       **/
      EraPaid: AugmentedEvent<ApiType, [eraIndex: u32, validatorPayout: u128, remainder: u128], { eraIndex: u32, validatorPayout: u128, remainder: u128 }>;
      /**
       * A new force era mode was set.
       **/
      ForceEra: AugmentedEvent<ApiType, [mode: PalletStakingForcing], { mode: PalletStakingForcing }>;
      /**
       * A nominator has been kicked from a validator.
       **/
      Kicked: AugmentedEvent<ApiType, [nominator: AccountId32, stash: AccountId32], { nominator: AccountId32, stash: AccountId32 }>;
      /**
       * An old slashing report from a prior era was discarded because it could
       * not be processed.
       **/
      OldSlashingReportDiscarded: AugmentedEvent<ApiType, [sessionIndex: u32], { sessionIndex: u32 }>;
      /**
       * The stakers' rewards are getting paid.
       **/
      PayoutStarted: AugmentedEvent<ApiType, [eraIndex: u32, validatorStash: AccountId32], { eraIndex: u32, validatorStash: AccountId32 }>;
      /**
       * The nominator has been rewarded by this amount.
       **/
      Rewarded: AugmentedEvent<ApiType, [stash: AccountId32, amount: u128], { stash: AccountId32, amount: u128 }>;
      /**
       * A staker (validator or nominator) has been slashed by the given amount.
       **/
      Slashed: AugmentedEvent<ApiType, [staker: AccountId32, amount: u128], { staker: AccountId32, amount: u128 }>;
      /**
       * A slash for the given validator, for the given percentage of their stake, at the given
       * era as been reported.
       **/
      SlashReported: AugmentedEvent<ApiType, [validator: AccountId32, fraction: Perbill, slashEra: u32], { validator: AccountId32, fraction: Perbill, slashEra: u32 }>;
      /**
       * A new set of stakers was elected.
       **/
      StakersElected: AugmentedEvent<ApiType, []>;
      /**
       * The election failed. No new era is planned.
       **/
      StakingElectionFailed: AugmentedEvent<ApiType, []>;
      /**
       * An account has unbonded this amount.
       **/
      Unbonded: AugmentedEvent<ApiType, [stash: AccountId32, amount: u128], { stash: AccountId32, amount: u128 }>;
      /**
       * A validator has set their preferences.
       **/
      ValidatorPrefsSet: AugmentedEvent<ApiType, [stash: AccountId32, prefs: PalletStakingValidatorPrefs], { stash: AccountId32, prefs: PalletStakingValidatorPrefs }>;
      /**
       * An account has called `withdraw_unbonded` and removed unbonding chunks worth `Balance`
       * from the unlocking queue.
       **/
      Withdrawn: AugmentedEvent<ApiType, [stash: AccountId32, amount: u128], { stash: AccountId32, amount: u128 }>;
      /**
       * Generic event
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
       * Generic event
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
      ExtrinsicFailed: AugmentedEvent<ApiType, [dispatchError: SpRuntimeDispatchError, dispatchInfo: FrameSupportDispatchDispatchInfo], { dispatchError: SpRuntimeDispatchError, dispatchInfo: FrameSupportDispatchDispatchInfo }>;
      /**
       * An extrinsic completed successfully.
       **/
      ExtrinsicSuccess: AugmentedEvent<ApiType, [dispatchInfo: FrameSupportDispatchDispatchInfo], { dispatchInfo: FrameSupportDispatchDispatchInfo }>;
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
       * Generic event
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
       * Generic event
       **/
      [key: string]: AugmentedEvent<ApiType>;
    };
    utility: {
      /**
       * Batch of dispatches completed fully with no error.
       **/
      BatchCompleted: AugmentedEvent<ApiType, []>;
      /**
       * Batch of dispatches completed but has errors.
       **/
      BatchCompletedWithErrors: AugmentedEvent<ApiType, []>;
      /**
       * Batch of dispatches did not complete fully. Index of first failing dispatch given, as
       * well as the error.
       **/
      BatchInterrupted: AugmentedEvent<ApiType, [index: u32, error: SpRuntimeDispatchError], { index: u32, error: SpRuntimeDispatchError }>;
      /**
       * A call was dispatched.
       **/
      DispatchedAs: AugmentedEvent<ApiType, [result: Result<Null, SpRuntimeDispatchError>], { result: Result<Null, SpRuntimeDispatchError> }>;
      /**
       * A single item within a Batch of dispatches has completed with no error.
       **/
      ItemCompleted: AugmentedEvent<ApiType, []>;
      /**
       * A single item within a Batch of dispatches has completed with error.
       **/
      ItemFailed: AugmentedEvent<ApiType, [error: SpRuntimeDispatchError], { error: SpRuntimeDispatchError }>;
      /**
       * Generic event
       **/
      [key: string]: AugmentedEvent<ApiType>;
    };
  } // AugmentedEvents
} // declare module
