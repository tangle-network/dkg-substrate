use super::*;
use sp_runtime::{Perbill, Saturating};

/// The type of DKG misbehaviour offences
#[derive(RuntimeDebug, TypeInfo)]
#[cfg_attr(feature = "std", derive(Clone, PartialEq, Eq))]
pub enum DKGMisbehaviorOffenceType {
	SignedProposalNotInQueue,
}

/// An offence that is filed if a DKG authority misbehaves
#[derive(RuntimeDebug, TypeInfo)]
#[cfg_attr(feature = "std", derive(Clone, PartialEq, Eq))]
pub struct DKGMisbehaviourOffence<Offender> {
	/// The current session index in which we report the unresponsive validators.
	///
	/// It acts as a time measure for unresponsiveness reports and effectively will always point
	/// at the end of the session.
	pub session_index: SessionIndex,
	/// The size of the validator set in current session/era.
	pub validator_set_count: u32,
	/// The type of offence
	pub offence: DKGMisbehaviorOffenceType,
	/// Authorities that were unresponsive during the current era.
	pub offenders: Vec<Offender>,
}

impl<Offender: Clone> Offence<Offender> for DKGMisbehaviourOffence<Offender> {
	const ID: Kind = *b"im-online:offlin";
	type TimeSlot = SessionIndex;

	fn offenders(&self) -> Vec<Offender> {
		self.offenders.clone()
	}

	fn session_index(&self) -> SessionIndex {
		self.session_index
	}

	fn validator_set_count(&self) -> u32 {
		self.validator_set_count
	}

	fn time_slot(&self) -> Self::TimeSlot {
		self.session_index
	}

	fn disable_strategy(&self) -> DisableStrategy {
		DisableStrategy::Never
	}

	fn slash_fraction(&self, offenders: u32) -> Perbill {
		// the formula is min((3 * (k - (n / 10 + 1))) / n, 1) * 0.07
		// basically, 10% can be offline with no slash, but after that, it linearly climbs up to 7%
		// when 13/30 are offline (around 5% when 1/3 are offline).
		if let Some(threshold) = offenders.checked_sub(self.validator_set_count / 10 + 1) {
			let x = Perbill::from_rational(3 * threshold, self.validator_set_count);
			x.saturating_mul(Perbill::from_percent(7))
		} else {
			Perbill::default()
		}
	}
}
