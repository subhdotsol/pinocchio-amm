use constant_product_curve::CurveError;
use pinocchio::error::ProgramError;
use thiserror::Error;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Error)]
pub enum AmmError {
    #[error("default error")]
    DefaultError,
    #[error("offer expired")]
    OfferExpired,
    #[error("this pool is locked")]
    PoolLocked,
    #[error("slippage exceeded")]
    SlippageExceeded,
    #[error("overflow detected")]
    Overflow,
    #[error("underflow detected")]
    Underflow,
    #[error("invalid token")]
    InvalidToken,
    #[error("actual liquidity is less than minimum")]
    LiquidityLessThanMinimum,
    #[error("no liquidity in pool")]
    NoLiquidityInPool,
    #[error("bump error")]
    BumpError,
    #[error("curve error")]
    CurveError,
    #[error("fee is greater than 100%")]
    InvalidFee,
    #[error("invalid update authority")]
    InvalidAuthority,
    #[error("no update authority set")]
    NoAuthoritySet,
    #[error("invalid amount")]
    InvalidAmount,
    #[error("invalid precision")]
    InvalidPrecision,
    #[error("insufficient balance")]
    InsufficientBalance,
    #[error("zero balance")]
    ZeroBalance,
}

impl From<AmmError> for ProgramError {
    fn from(e: AmmError) -> Self {
        ProgramError::Custom(e as u32)
    }
}

impl From<CurveError> for AmmError {
    fn from(error: CurveError) -> AmmError {
        match error {
            CurveError::InvalidPrecision => AmmError::InvalidPrecision,
            CurveError::Overflow => AmmError::Overflow,
            CurveError::Underflow => AmmError::Underflow,
            CurveError::InvalidFeeAmount => AmmError::InvalidFee,
            CurveError::InsufficientBalance => AmmError::InsufficientBalance,
            CurveError::ZeroBalance => AmmError::ZeroBalance,
            CurveError::SlippageLimitExceeded => AmmError::SlippageExceeded,
        }
    }
}
