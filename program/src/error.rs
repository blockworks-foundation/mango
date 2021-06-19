use bytemuck::Contiguous;
use num_enum::IntoPrimitive;
use solana_program::program_error::ProgramError;
use thiserror::Error;

pub type MangoResult<T = ()> = Result<T, MangoError>;

#[repr(u8)]
#[derive(Debug, Clone, Eq, PartialEq, Copy)]
pub enum SourceFileId {
    Processor = 0,
    State = 1,
}

impl std::fmt::Display for SourceFileId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SourceFileId::Processor => write!(f, "src/processor.rs"),
            SourceFileId::State => write!(f, "src/state.rs")
        }
    }
}


#[derive(Error, Debug, PartialEq, Eq)]
pub enum MangoError {
    #[error(transparent)]
    ProgramError(#[from] ProgramError),
    #[error("{mango_error_code}; {source_file_id}:{line}")]
    MangoErrorCode { mango_error_code: MangoErrorCode, line: u32, source_file_id: SourceFileId},
}

#[derive(Debug, Error, Clone, Copy, PartialEq, Eq, IntoPrimitive)]
#[repr(u32)]
pub enum MangoErrorCode {
    #[error("MangoErrorCode::BorrowLimitExceeded This instruction would exceed the borrow limit")]
    BorrowLimitExceeded,
    #[error("MangoErrorCode::CollateralRatioLimit Your collateral ratio is below the minimum initial collateral ratio")]
    CollateralRatioLimit,
    #[error("MangoErrorCode::InsufficientFunds Quantity requested is above the available balance")]
    InsufficientFunds,
    #[error("MangoErrorCode::InvalidMangoGroupSize")]
    InvalidMangoGroupSize,
    #[error("MangoErrorCode::InvalidGroupOwner")]
    InvalidGroupOwner,
    #[error("MangoErrorCode::InvalidGroupFlags")]
    InvalidGroupFlags,
    #[error("MangoErrorCode::InvalidMarginAccountOwner This margin account is not owned by the wallet address")]
    InvalidMarginAccountOwner,
    #[error("MangoErrorCode::GroupNotRentExempt")]
    GroupNotRentExempt,
    #[error("MangoErrorCode::InvalidSignerKey")]
    InvalidSignerKey,
    #[error("MangoErrorCode::InvalidProgramId")]
    InvalidProgramId,
    #[error("MangoErrorCode::NotLiquidatable")]
    NotLiquidatable,
    #[error("MangoErrorCode::InvalidOpenOrdersAccount")]
    InvalidOpenOrdersAccount,
    #[error("MangoErrorCode::SignerNecessary")]
    SignerNecessary,
    #[error("MangoErrorCode::InvalidMangoVault")]
    InvalidMangoVault,
    #[error("MangoErrorCode::BeingLiquidated The margin account has restricted functionality while being liquidated")]
    BeingLiquidated,
    #[error("MangoErrorCode::FeeDiscountFunctionality SRM is already part of MangoGroup. Deposit and withdraw SRM functionality disabled.")]
    FeeDiscountFunctionality,
    #[error("MangoErrorCode::Deprecated")]
    Deprecated,

    #[error("MangoErrorCode::Default Check the source code for more info")]
    Default = u32::MAX_VALUE,
}

impl From<MangoError> for ProgramError {
    fn from(e: MangoError) -> ProgramError {
        match e {
            MangoError::ProgramError(pe) => pe,
            MangoError::MangoErrorCode {
                mango_error_code,
                line: _,
                source_file_id: _
            } => ProgramError::Custom(mango_error_code.into()),
        }
    }
}

impl From<serum_dex::error::DexError> for MangoError {
    fn from(de: serum_dex::error::DexError) -> Self {
        let pe: ProgramError = de.into();
        pe.into()
    }
}

#[inline]
pub fn check_assert(
    cond: bool,
    mango_error_code:
    MangoErrorCode,
    line: u32,
    source_file_id: SourceFileId
) -> MangoResult<()> {
    if cond {
        Ok(())
    } else {
        Err(MangoError::MangoErrorCode { mango_error_code, line, source_file_id })
    }
}
