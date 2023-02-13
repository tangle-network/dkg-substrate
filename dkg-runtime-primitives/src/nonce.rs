use core::ops::{Add, Mul};
use num_traits::{Bounded, CheckedMul, One, Saturating, Zero};

#[cfg(feature = "std")]
use serde::{Deserialize, Serialize};

/// Proposal ProposalNonce (4 bytes).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Ord, PartialOrd)]
#[cfg_attr(
    feature = "scale",
    derive(scale_info::TypeInfo, scale_codec::Encode, scale_codec::Decode)
)]
#[cfg_attr(feature = "std", derive(Serialize, Deserialize))]
pub struct ProposalNonce(pub u32);

impl Add for ProposalNonce {
    type Output = Self;

    fn add(self, rhs: Self) -> Self::Output {
        ProposalNonce(self.0 + rhs.0)
    }
}
impl Mul for ProposalNonce {
    type Output = Self;

    fn mul(self, rhs: Self) -> Self::Output {
        ProposalNonce(self.0 * rhs.0)
    }
}

impl CheckedMul for ProposalNonce {
    fn checked_mul(&self, v: &Self) -> Option<Self> {
        self.0.checked_mul(v.0).map(ProposalNonce)
    }
}

impl Zero for ProposalNonce {
    fn zero() -> Self {
        ProposalNonce(0)
    }

    fn is_zero(&self) -> bool {
        self.0 == 0
    }

    fn set_zero(&mut self) {
        *self = Zero::zero();
    }
}

impl One for ProposalNonce {
    fn one() -> Self {
        ProposalNonce(1)
    }
}

impl Saturating for ProposalNonce {
    fn saturating_add(self, v: Self) -> Self {
        self.0.saturating_add(v.0).into()
    }

    fn saturating_sub(self, v: Self) -> Self {
        self.0.saturating_sub(v.0).into()
    }
}

impl Bounded for ProposalNonce {
    fn min_value() -> Self {
        ProposalNonce(u32::min_value())
    }

    fn max_value() -> Self {
        ProposalNonce(u32::max_value())
    }
}
