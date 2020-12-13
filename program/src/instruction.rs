use arrayref::{array_ref, array_refs};
use serde::{Deserialize, Serialize};
use solana_program::instruction::{Instruction, AccountMeta};
use solana_program::program_error::ProgramError;
use solana_program::pubkey::Pubkey;


#[repr(C)]
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum MangoInstruction {
    /// Initialize a group of lending pools that can be cross margined
    ///
    /// Accounts expected by this instruction (5 + 2 * NUM_TOKENS + NUM_MARKETS):
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
    /// 5+NUM_TOKENS..5+2*NUM_TOKENS `[]`
    ///     vault_accs - Vault owned by signer_acc.key for each of the mints
    ///
    /// 5+2*NUM_TOKENS..5+2*NUM_TOKENS+NUM_MARKETS `[]`
    ///     spot_market_accs - MarketState account from serum dex for each of the spot markets
    InitMangoGroup {
        signer_nonce: u64
    },

    /// Initialize a margin account for a user
    ///
    /// Accounts expected by this instruction (4 + NUM_MARKETS):
    ///
    /// 0. `[]` mango_group_acc - MangoGroup that this margin account is for
    /// 1. `[writable]` margin_account_acc - the margin account data
    /// 2. `[signer]` owner_acc - Solana account of owner of the margin account
    /// 3. `[]` rent_acc - Rent sysvar account
    /// 4..4+NUM_MARKETS `[]` open_orders_accs - uninitialized serum dex open orders accounts
    InitMarginAccount,

    /// Deposit funds into margin account to be used as collateral and earn interest.
    ///
    /// Accounts expected by this instruction (8):
    ///
    /// 0. `[writable]` mango_group_acc - MangoGroup that this margin account is for
    /// 1. `[writable]` margin_account_acc - the margin account for this user
    /// 2. `[signer]` owner_acc - Solana account of owner of the margin account
    /// 3. `[]` mint_acc - Mint of the token being deposited
    /// 4. `[writable]` token_account_acc - TokenAccount owned by user which will be sending the funds
    /// 5. `[writable]` vault_acc - TokenAccount owned by MangoGroup
    /// 6. `[]` token_prog_acc - acc pointed to by SPL token program id
    /// 7. `[]` clock_acc - Clock sysvar account
    Deposit {
        quantity: u64
    },

    /// Withdraw funds that were deposited earlier.
    ///
    /// Accounts expected by this instruction (9 + 4 * NUM_MARKETS):
    ///
    /// 0. `[writable]` mango_group_acc - MangoGroup that this margin account is for
    /// 1. `[writable]` margin_account_acc - the margin account for this user
    /// 2. `[signer]` owner_acc - Solana account of owner of the margin account
    /// 3. `[]` mint_acc - Mint of the token being withdrawn
    /// 4. `[writable]` token_account_acc - TokenAccount owned by user which will be receiving the funds
    /// 5. `[writable]` vault_acc - TokenAccount owned by MangoGroup which will be sending
    /// 6. `[]` signer_acc - acc pointed to by signer_key
    /// 7. `[]` token_prog_acc - acc pointed to by SPL token program id
    /// 8. `[]` clock_acc - Clock sysvar account
    /// 9..9+NUM_MARKETS `[]` open_orders_accs - open orders for each of the spot market
    /// 9+NUM_MARKETS..9+2*NUM_MARKETS `[]`
    ///     spot_market_accs - MarketState accounts for serum dex
    /// 9+2*NUM_MARKETS..9+3*NUM_MARKETS `[]`
    ///     bids_accs - The bids for each of the spot markets
    /// 9+3*NUM_MARKETS..9+4*NUM_MARKETS `[]`
    ///     asks_accs - The asks for each of the spot markets
    Withdraw {
        quantity: u64
    },

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
            1 => {
                MangoInstruction::InitMarginAccount
            },
            2 => {
                let quantity = array_ref![data, 0, 8];
                MangoInstruction::Deposit { quantity: u64::from_le_bytes(*quantity) }
            },
            3 => {
                let quantity = array_ref![data, 0, 8];
                MangoInstruction::Withdraw { quantity: u64::from_le_bytes(*quantity) }
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
    mint_pks: &[Pubkey],
    vault_pks: &[Pubkey],
    spot_market_pks: &[Pubkey],
    signer_nonce: u64,
) -> Result<Instruction, ProgramError> {
    let mut accounts = vec![
        AccountMeta::new(*mango_group_pk, false),
        AccountMeta::new_readonly(solana_program::sysvar::rent::ID, false),
        AccountMeta::new_readonly(solana_program::sysvar::clock::ID, false),
        AccountMeta::new_readonly(*signer_pk, false),
        AccountMeta::new_readonly(*dex_prog_id, false)
    ];
    accounts.extend(mint_pks.iter().map(
        |pk| AccountMeta::new_readonly(*pk, false))
    );
    accounts.extend(vault_pks.iter().map(
        |pk| AccountMeta::new_readonly(*pk, false))
    );
    accounts.extend(spot_market_pks.iter().map(
        |pk| AccountMeta::new_readonly(*pk, false))
    );

    let instr = MangoInstruction::InitMangoGroup { signer_nonce };
    let data = instr.pack();
    Ok(Instruction {
        program_id: *program_id,
        accounts,
        data
    })
}

pub fn init_margin_account(
    program_id: &Pubkey,
    mango_group_pk: &Pubkey,
    margin_account_pk: &Pubkey,
    owner_pk: &Pubkey,
    open_orders_pks: &Vec<Pubkey>
) -> Result<Instruction, ProgramError> {
    let mut accounts = vec![
        AccountMeta::new_readonly(*mango_group_pk, false),
        AccountMeta::new(*margin_account_pk, false),
        AccountMeta::new_readonly(*owner_pk, true),
        AccountMeta::new_readonly(solana_program::sysvar::rent::ID, false),
    ];
    accounts.extend(open_orders_pks.iter().map(
        |pk| AccountMeta::new_readonly(*pk, false))
    );

    let instr = MangoInstruction::InitMarginAccount;
    let data = instr.pack();
    Ok(Instruction {
        program_id: *program_id,
        accounts,
        data
    })
}

pub fn deposit(
    program_id: &Pubkey,
    mango_group_pk: &Pubkey,
    margin_account_pk: &Pubkey,
    owner_pk: &Pubkey,
    mint_pk: &Pubkey,
    token_account_pk: &Pubkey,
    vault_pk: &Pubkey,
    quantity: u64
) -> Result<Instruction, ProgramError> {
    let accounts = vec![
        AccountMeta::new(*mango_group_pk, false),
        AccountMeta::new(*margin_account_pk, false),
        AccountMeta::new_readonly(*owner_pk, true),
        AccountMeta::new_readonly(*mint_pk, false),
        AccountMeta::new(*token_account_pk, false),
        AccountMeta::new(*vault_pk, false),
        AccountMeta::new_readonly(solana_program::sysvar::clock::ID, false),
    ];

    let instr = MangoInstruction::Deposit { quantity };
    let data = instr.pack();
    Ok(Instruction {
        program_id: *program_id,
        accounts,
        data
    })
}

pub fn withdraw(
    program_id: &Pubkey,
    mango_group_pk: &Pubkey,
    margin_account_pk: &Pubkey,
    owner_pk: &Pubkey,
    mint_pk: &Pubkey,
    token_account_pk: &Pubkey,
    vault_pk: &Pubkey,
    signer_pk: &Pubkey,
    open_orders_pks: &[Pubkey],
    spot_market_pks: &[Pubkey],
    bids_pks: &[Pubkey],
    asks_pks: &[Pubkey],
    quantity: u64
) -> Result<Instruction, ProgramError> {
    let mut accounts = vec![
        AccountMeta::new(*mango_group_pk, false),
        AccountMeta::new(*margin_account_pk, false),
        AccountMeta::new_readonly(*owner_pk, true),
        AccountMeta::new_readonly(*mint_pk, false),
        AccountMeta::new(*token_account_pk, false),
        AccountMeta::new(*vault_pk, false),
        AccountMeta::new_readonly(*signer_pk, false),
        AccountMeta::new_readonly(solana_program::sysvar::clock::ID, false),
    ];

    accounts.extend(open_orders_pks.iter().map(
        |pk| AccountMeta::new_readonly(*pk, false))
    );
    accounts.extend(spot_market_pks.iter().map(
        |pk| AccountMeta::new_readonly(*pk, false))
    );
    accounts.extend(bids_pks.iter().map(
        |pk| AccountMeta::new_readonly(*pk, false))
    );
    accounts.extend(asks_pks.iter().map(
        |pk| AccountMeta::new_readonly(*pk, false))
    );

    let instr = MangoInstruction::Withdraw { quantity };
    let data = instr.pack();
    Ok(Instruction {
        program_id: *program_id,
        accounts,
        data
    })
}