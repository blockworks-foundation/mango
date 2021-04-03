use bytemuck::Contiguous;
use num_enum::IntoPrimitive;
use serum_dex::error::DexError;
use solana_program::program_error::ProgramError;
use thiserror::Error;

pub type MangoResult<T = ()> = Result<T, MangoError>;

#[repr(u8)]
#[derive(Error, Debug, Clone, Eq, PartialEq, Copy)]
pub enum SourceFileId {
    #[error("src/processor.rs")]
    Processor = 0,
    #[error("src/state.rs")]
    State = 1,
}

#[derive(Error, Debug, PartialEq, Eq)]
pub enum MangoError {
    #[error(transparent)]
    ProgramError(#[from] ProgramError),
    #[error("{mango_error_code}; source: {source_file_id}:{line}")]
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

impl From<DexError> for MangoError {
    fn from(de: DexError) -> Self {
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
