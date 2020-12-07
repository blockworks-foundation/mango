use arrayref::{array_ref, array_refs};
use serde::{Deserialize, Serialize};
use solana_program::instruction::{Instruction, AccountMeta};
use solana_program::program_error::ProgramError;
use solana_program::pubkey::Pubkey;
use crate::state::{NUM_TOKENS, NUM_MARKETS};


#[repr(C)]
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum MangoInstruction {
    /// Initialize a group of lending pools that can be cross margined
    ///
    /// Accounts expected by this instruction:
    ///
    /// 0. `[writable]` mango_group_acc - the data account to store mango group state vars
    /// 1. `[]` rent_acc - Rent sysvar account
    /// 2. `[]` clock_acc - clock sysvar account
    /// 3. `[]` signer_acc - pubkey of program_id hashed with signer_nonce and mango_group_acc.key
    /// 4. `[]` dex_prog_acc - program id of serum dex
    /// 5..5+NUM_TOKENS `[]` token_mint_accs - mint of each token in the same order as the spot
    ///     markets. Quote currency mint should be last.
    ///     e.g. for spot markets BTC/USDC, ETH/USDC -> [BTC, ETH, USDC]
    ///
    /// 5+NUM_TOKENS..5+2*NUM_TOKENS `[writable]`
    ///     vault_accs - Vault owned by signer_acc.key for each of the mints
    ///
    /// 5+2*NUM_TOKENS..5+2*NUM_TOKENS+NUM_MARKETS `[]`
    ///     spot_market_accs - MarketState account from serum dex for each of the spot markets
    ///
    /// Total number of accounts = 5 + 2 * NUM_TOKENS + NUM_MARKETS
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
    mango_group_pk: &Pubkey,
    signer_pk: &Pubkey,
    dex_prog_id: &Pubkey,
    mint_pks: &[Pubkey; NUM_TOKENS],
    vault_pks: &[Pubkey; NUM_TOKENS],
    spot_market_pks: &[Pubkey; NUM_MARKETS],
    signer_nonce: u64,
) -> Result<Instruction, ProgramError> {
    let instr = MangoInstruction::InitMangoGroup { signer_nonce };
    let mut accounts = vec![
        AccountMeta::new(*mango_group_pk, false),
        AccountMeta::new_readonly(solana_program::sysvar::rent::ID, false),
        AccountMeta::new_readonly(solana_program::sysvar::clock::ID, false),
        AccountMeta::new_readonly(*signer_pk, false),
        AccountMeta::new_readonly(*dex_prog_id, false)
    ];
    accounts.extend(mint_pks.iter().map(|pk| AccountMeta::new_readonly(*pk, false)));
    accounts.extend(vault_pks.iter().map(|pk| AccountMeta::new(*pk, false)));
    accounts.extend(spot_market_pks.iter().map(|pk| AccountMeta::new_readonly(*pk, false)));

    let data = instr.pack();
    Ok(Instruction {
        program_id: *program_id,
        accounts,
        data
    })
}