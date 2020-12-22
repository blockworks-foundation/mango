use std::error::Error;
use std::fmt::{Display, Formatter};

use num_enum::IntoPrimitive;
use solana_program::program_error::ProgramError;
use thiserror::Error;
use serum_dex::error::DexError;

pub type MangoResult<T = ()> = Result<T, MangoError>;

#[repr(u8)]
#[derive(Error, Debug, Clone, Eq, PartialEq)]
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
        write!(f, "AssertionError(file: {:?} line: {})", self.file_id, self.line)
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
    #[error(transparent)]
    MangoErrorCode(#[from] MangoErrorCode),  // Just wraps MangoErrorCode into MangoError::ErrorCode
    #[error(transparent)]
    AssertionError(#[from] AssertionError)
}

#[derive(Debug, Error, IntoPrimitive, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum MangoErrorCode {
    #[error("Invalid group owner; MangoGroup must be owned by Mango program id")]
    InvalidGroupOwner
}

impl From<MangoError> for ProgramError {
    fn from(e: MangoError) -> ProgramError {
        match e {
            MangoError::ProgramError(pe) => pe,
            MangoError::MangoErrorCode(mec) => ProgramError::Custom(mec.into()),
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
pub fn check_assert(cond: bool, line: u16, file_id: SourceFileId) -> Result<(), AssertionError> {
    if cond {
        Ok(())
    } else {
        Err(AssertionError {
            line,
            file_id
        })
    }
}
