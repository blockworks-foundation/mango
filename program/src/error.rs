use std::error::Error;
use std::fmt::{Display, Formatter};

use num_enum::IntoPrimitive;
use serum_dex::error::DexError;
use solana_program::program_error::ProgramError;
use thiserror::Error;
use bytemuck::Contiguous;

pub type MangoResult<T = ()> = Result<T, MangoError>;

#[repr(u8)]
#[derive(Error, Debug, Clone, Eq, PartialEq, Copy)]
pub enum SourceFileId {
    #[error("src/processor.rs")]
    Processor = 0,
    #[error("src/state.rs")]
    State = 1,

}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct AssertionError {
    pub line: u16,
    pub file_id: SourceFileId,
}

impl Display for AssertionError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        let file_name = match self.file_id {
            SourceFileId::Processor => "src/processor.rs",
            SourceFileId::State => "src/state.rs"
        };
        write!(f, "AssertionError(file: {} line: {})", file_name, self.line)
    }
}
impl Error for AssertionError {}

impl From<AssertionError> for u32 {
    fn from(err: AssertionError) -> u32 {
        (err.line as u32) + ((err.file_id as u8 as u32) << 24)
    }
}

#[derive(Error, Debug, PartialEq, Eq)]
pub enum MangoError {
    #[error(transparent)]
    ProgramError(#[from] ProgramError),
    #[error("{mango_error_code}; source: {source_file_id}:{line}")]
    MangoErrorCode { mango_error_code: MangoErrorCode, line: u32, source_file_id: SourceFileId},  // Just wraps MangoErrorCode into MangoError::MangoErrorCode
    #[error(transparent)]
    AssertionError(#[from] AssertionError)
}

#[derive(Debug, Error, Clone, Copy, PartialEq, Eq, IntoPrimitive)]
#[repr(u32)]
pub enum MangoErrorCode {
    #[error("MangoErrorCode::AboveBorrowLimit This transaction would exceed the borrow limit")]
    AboveBorrowLimit,
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

    #[error("MangoErrorCode::Default")]
    Default = u32::MAX_VALUE,
}

impl From<MangoError> for ProgramError {
    fn from(e: MangoError) -> ProgramError {
        match e {
            MangoError::ProgramError(pe) => pe,
            MangoError::MangoErrorCode { mango_error_code, line: _, source_file_id: _ } => ProgramError::Custom(mango_error_code.into()),
            MangoError::AssertionError(ae) => ProgramError::Custom(ae.into())
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
pub fn check_assert(cond: bool, line: u16, file_id: SourceFileId) -> MangoResult<()> {
    if cond {
        Ok(())
    } else {
        Err(MangoError::AssertionError(AssertionError {
            line,
            file_id
        }))
    }
}

#[inline]
pub fn check_assert2(
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
