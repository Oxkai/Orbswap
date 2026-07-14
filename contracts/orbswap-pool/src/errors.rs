//! Contract error type. Math-library errors are mapped in via the `From` impls.

use orbswap_math::ccmm::CcmmError;
use orbswap_math::circle_liq::CircleLiqError;
use orbswap_math::csemm::CsemmError;
use soroban_sdk::contracterror;

#[contracterror]
#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
#[repr(u32)]
pub enum OrbswapError {
    NotInitialized = 1,
    AlreadyInitialized = 2,
    Paused = 3,
    Unauthorized = 4,
    SlippageExceeded = 5,
    Expired = 6,
    InsufficientLiquidity = 7,
    MinimumLiquidity = 8,
    TokenNotAllowed = 9,
    /// Bad initialize params (token count, duplicate, shape, mode mismatch, fee).
    InvalidConfig = 10,
    /// A swap/deposit amount was non-positive, or slice length mismatched.
    InvalidAmount = 11,
    /// Token address is not a member of the pool.
    UnknownToken = 12,
    /// Deposit ratios were not proportional to current reserves.
    ImbalancedDeposit = 13,
    /// Trade would push a reserve past its extent (negative-price fold).
    PriceOutOfRange = 14,
    /// Math domain error (e.g. off-arc, shape < 2).
    MathDomain = 15,
    /// Fixed-point overflow.
    Overflow = 16,
    /// Post-swap invariant drifted off-curve (csemm near-asymptote guard).
    InvariantViolation = 17,
    /// Trade smaller than the configured minimum.
    BelowMinTrade = 18,
    /// Operation requires concentrated-liquidity tick mode (Circular + enabled).
    TickModeOnly = 19,
    /// Share-based deposit/withdraw are disabled once tick mode is enabled.
    TickModeActive = 20,
    /// Tick range invalid (`lower`/`upper` outside `[0,90]` or `lower >= upper`).
    InvalidTickRange = 21,
    /// No position exists for this (owner, range).
    PositionNotFound = 22,
}

impl From<CircleLiqError> for OrbswapError {
    fn from(e: CircleLiqError) -> Self {
        match e {
            CircleLiqError::InvalidRange => OrbswapError::InvalidTickRange,
            CircleLiqError::InvalidAmount => OrbswapError::InvalidAmount,
            CircleLiqError::Overflow => OrbswapError::Overflow,
        }
    }
}

impl From<CcmmError> for OrbswapError {
    fn from(e: CcmmError) -> Self {
        match e {
            CcmmError::InvalidAmount => OrbswapError::InvalidAmount,
            CcmmError::OutOfRange => OrbswapError::MathDomain,
            CcmmError::PriceOutOfRange => OrbswapError::PriceOutOfRange,
            CcmmError::Overflow => OrbswapError::Overflow,
        }
    }
}

impl From<CsemmError> for OrbswapError {
    fn from(e: CsemmError) -> Self {
        match e {
            CsemmError::InvalidAmount => OrbswapError::InvalidAmount,
            CsemmError::OutOfRange => OrbswapError::MathDomain,
            CsemmError::PriceOutOfRange => OrbswapError::PriceOutOfRange,
            CsemmError::DomainError => OrbswapError::MathDomain,
            CsemmError::Overflow => OrbswapError::Overflow,
        }
    }
}
