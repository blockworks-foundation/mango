use solana_program::{
    msg, account_info::AccountInfo, entrypoint::ProgramResult, entrypoint, pubkey::Pubkey,
};

use crate::processor::Processor;


entrypoint!(process_instruction);
pub fn process_instruction(
    program_id: &Pubkey,
    accounts: &[AccountInfo],
    instruction_data: &[u8],
) -> ProgramResult {
    Processor::process(program_id, accounts, instruction_data).map_err(
        |e| {
            msg!("{}", e);  // log the error
            e.into()  // convert MangoError to generic ProgramError
        }
    )
}