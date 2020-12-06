use arrayref::{array_ref, array_refs};
use serde::{Deserialize, Serialize};
use solana_program::instruction::{AccountMeta, Instruction};
use solana_program::program_error::ProgramError;
use solana_program::pubkey::Pubkey;

#[repr(C)]
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum MangoInstruction {
    /// Initialize a group of lending pools that can be cross margined
    ///
    /// Accounts expected by this instruction:
    ///
    /// 0. `[writable]` mango_group_acc - the data account to store mango group state vars
    /// 1.
    InitMangoGroup {
        signer_nonce: u64
    },

    InitMarginAccount,

    Deposit,

    Withdraw,

    Liquidate,

    // Proxy instructions to Dex
    PlaceOrder,
    SettleFunds,
    CancelOrder,
    CancelOrderByClientId,
}


impl MangoInstruction {
    pub fn unpack(input: &[u8]) -> Option<Self> {
        let (&discrim, data) = array_refs![input, 4; ..;];
        let discrim = u32::from_le_bytes(discrim);
        Some(match discrim {
            0 => {
                let signer_nonce = array_ref![data, 0, 8];
                MangoInstruction::InitMangoGroup {
                    signer_nonce: u64::from_le_bytes(*signer_nonce)
                }
            }
            _ => { return None; }
        })
    }
    pub fn pack(&self) -> Vec<u8> {
        bincode::serialize(self).unwrap()
    }
}

pub fn init_mango_group(
    program_id: &Pubkey,
    signer_nonce: u64,
) -> Result<Instruction, ProgramError> {
    let instr = MangoInstruction::InitMangoGroup { signer_nonce };
    let accounts = vec![];
    let data = instr.pack();
    Ok(Instruction {
        program_id: *program_id,
        accounts,
        data
    })
}