pub mod processor;
pub mod state;

use solana_program::{
    account_info::AccountInfo, entrypoint::ProgramResult, entrypoint, pubkey::Pubkey,
};

use crate::processor::Processor;

#[cfg(not(feature = "no-entrypoint"))]
entrypoint!(process_instruction);
fn process_instruction(
    program_id: &Pubkey,
    accounts: &[AccountInfo],
    instruction_data: &[u8],
) -> ProgramResult {
    Processor::process(program_id, accounts, instruction_data)
}