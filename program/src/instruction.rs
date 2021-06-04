use std::convert::TryInto;
use std::num::NonZeroU64;

use arrayref::{array_ref, array_refs};
use bytemuck::{cast_slice, cast_slice_mut};
use fixed::types::U64F64;
use num_enum::TryFromPrimitive;
use serde::{Deserialize, Serialize};
use solana_program::instruction::{AccountMeta, Instruction};
use solana_program::program_error::ProgramError;
use solana_program::pubkey::Pubkey;

use crate::state::{NUM_TOKENS, INFO_LEN};

#[repr(C)]
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum MangoInstruction {
    /// Initialize a group of lending pools that can be cross margined
    ///
    /// Accounts expected by this instruction (7 + 2 * NUM_TOKENS + 2 * NUM_MARKETS):
    ///
    /// 0. `[writable]` mango_group_acc - the data account to store mango group state vars
    /// 1. `[]` rent_acc - Rent sysvar account
    /// 2. `[]` clock_acc - clock sysvar account
    /// 3. `[]` signer_acc - pubkey of program_id hashed with signer_nonce and mango_group_acc.key
    /// 4. `[]` dex_prog_acc - program id of serum dex
    /// 5. `[]` srm_vault_acc - vault for fee tier reductions
    /// 6. `[signer]` admin_acc - admin key who can change borrow limits
    /// 7..7+NUM_TOKENS `[]` token_mint_accs - mint of each token in the same order as the spot
    ///     markets. Quote currency mint should be last.
    ///     e.g. for spot markets BTC/USDC, ETH/USDC -> [BTC, ETH, USDC]
    ///
    /// 7+NUM_TOKENS..7+2*NUM_TOKENS `[]`
    ///     vault_accs - Vault owned by signer_acc.key for each of the mints
    ///
    /// 7+2*NUM_TOKENS..7+2*NUM_TOKENS+NUM_MARKETS `[]`
    ///     spot_market_accs - MarketState account from serum dex for each of the spot markets
    /// 7+2*NUM_TOKENS+NUM_MARKETS..7+2*NUM_TOKENS+2*NUM_MARKETS `[]`
    ///     oracle_accs - Solana Flux Aggregator accounts corresponding to each trading pair
    InitMangoGroup {
        signer_nonce: u64,
        maint_coll_ratio: U64F64,
        init_coll_ratio: U64F64,
        borrow_limits: [u64; NUM_TOKENS]
    },

    /// Initialize a margin account for a user
    ///
    /// Accounts expected by this instruction (4):
    ///
    /// 0. `[]` mango_group_acc - MangoGroup that this margin account is for
    /// 1. `[writable]` margin_account_acc - the margin account data
    /// 2. `[signer]` owner_acc - Solana account of owner of the margin account
    /// 3. `[]` rent_acc - Rent sysvar account
    InitMarginAccount,

    /// Deposit funds into margin account to be used as collateral and earn interest.
    ///
    /// Accounts expected by this instruction (7):
    ///
    /// 0. `[writable]` mango_group_acc - MangoGroup that this margin account is for
    /// 1. `[writable]` margin_account_acc - the margin account for this user
    /// 2. `[signer]` owner_acc - Solana account of owner of the margin account
    /// 3. `[writable]` token_account_acc - TokenAccount owned by user which will be sending the funds
    /// 4. `[writable]` vault_acc - TokenAccount owned by MangoGroup
    /// 5. `[]` token_prog_acc - acc pointed to by SPL token program id
    /// 6. `[]` clock_acc - Clock sysvar account
    Deposit {
        quantity: u64
    },

    /// Withdraw funds that were deposited earlier.
    ///
    /// Accounts expected by this instruction (8 + 2 * NUM_MARKETS):
    ///
    /// 0. `[writable]` mango_group_acc - MangoGroup that this margin account is for
    /// 1. `[writable]` margin_account_acc - the margin account for this user
    /// 2. `[signer]` owner_acc - Solana account of owner of the margin account
    /// 3. `[writable]` token_account_acc - TokenAccount owned by user which will be receiving the funds
    /// 4. `[writable]` vault_acc - TokenAccount owned by MangoGroup which will be sending
    /// 5. `[]` signer_acc - acc pointed to by signer_key
    /// 6. `[]` token_prog_acc - acc pointed to by SPL token program id
    /// 7. `[]` clock_acc - Clock sysvar account
    /// 8..8+NUM_MARKETS `[]` open_orders_accs - open orders for each of the spot market
    /// 8+NUM_MARKETS..8+2*NUM_MARKETS `[]`
    ///     oracle_accs - flux aggregator feed accounts
    Withdraw {
        quantity: u64
    },

    /// Borrow by incrementing MarginAccount.borrows given collateral ratio is below init_coll_rat
    ///
    /// Accounts expected by this instruction (4 + 2 * NUM_MARKETS):
    ///
    /// 0. `[writable]` mango_group_acc - MangoGroup that this margin account is for
    /// 1. `[writable]` margin_account_acc - the margin account for this user
    /// 2. `[signer]` owner_acc - Solana account of owner of the margin account
    /// 3. `[]` clock_acc - Clock sysvar account
    /// 4..4+NUM_MARKETS `[]` open_orders_accs - open orders for each of the spot market
    /// 4+NUM_MARKETS..4+2*NUM_MARKETS `[]`
    ///     oracle_accs - flux aggregator feed accounts
    Borrow {
        token_index: usize,
        quantity: u64
    },

    /// Use this token's position and deposit to reduce borrows
    ///
    /// Accounts expected by this instruction (4):
    ///
    /// 0. `[writable]` mango_group_acc - MangoGroup that this margin account is for
    /// 1. `[writable]` margin_account_acc - the margin account for this user
    /// 2. `[signer]` owner_acc - Solana account of owner of the margin account
    /// 3. `[]` clock_acc - Clock sysvar account
    SettleBorrow {
        token_index: usize,
        quantity: u64
    },

    /// Take over a MarginAccount that is below init_coll_ratio by depositing funds
    ///
    /// Accounts expected by this instruction (5 + 2 * NUM_MARKETS + 2 * NUM_TOKENS):
    ///
    /// 0. `[writable]` mango_group_acc - MangoGroup that this margin account is for
    /// 1. `[signer]` liqor_acc - liquidator's solana account
    /// 2. `[writable]` liqee_margin_account_acc - MarginAccount of liquidatee
    /// 3. `[]` token_prog_acc - SPL token program id
    /// 4. `[]` clock_acc - Clock sysvar account
    /// 5..5+NUM_MARKETS `[]` open_orders_accs - open orders for each of the spot market
    /// 5+NUM_MARKETS..5+2*NUM_MARKETS `[]`
    ///     oracle_accs - flux aggregator feed accounts
    /// 5+2*NUM_MARKETS..5+2*NUM_MARKETS+NUM_TOKENS `[writable]`
    ///     vault_accs - MangoGroup vaults
    /// 5+2*NUM_MARKETS+NUM_TOKENS..5+2*NUM_MARKETS+2*NUM_TOKENS `[writable]`
    ///     liqor_token_account_accs - Liquidator's token wallets
    Liquidate {
        /// Quantity of each token liquidator is depositing in order to bring account above maint
        deposit_quantities: [u64; NUM_TOKENS]
    },

    /// Deposit SRM into the SRM vault for MangoGroup
    /// These SRM are not at risk and are not counted towards collateral or any margin calculations
    /// Depositing SRM is a strictly altruistic act with no upside and no downside
    ///
    /// Accounts expected by this instruction (8):
    ///
    /// 0. `[writable]` mango_group_acc - MangoGroup that this margin account is for
    /// 1. `[writable]` mango_srm_account_acc - the mango srm account for user
    /// 2. `[signer]` owner_acc - Solana account of owner of the margin account
    /// 3. `[writable]` srm_account_acc - TokenAccount owned by user which will be sending the funds
    /// 4. `[writable]` vault_acc - SRM vault of MangoGroup
    /// 5. `[]` token_prog_acc - acc pointed to by SPL token program id
    /// 6. `[]` clock_acc - Clock sysvar account
    /// 7. `[]` rent_acc - Rent sysvar account
    DepositSrm {
        quantity: u64
    },
    /// Withdraw SRM owed to this MarginAccount
    /// These SRM are not at risk and are not counted towards collateral or any margin calculations
    /// Depositing SRM is a strictly altruistic act with no upside and no downside
    ///
    /// Accounts expected by this instruction (8):
    ///
    /// 0. `[writable]` mango_group_acc - MangoGroup that this margin account is for
    /// 1. `[writable]` mango_srm_account_acc - the mango srm account for user
    /// 2. `[signer]` owner_acc - Solana account of owner of the margin account
    /// 3. `[writable]` srm_account_acc - TokenAccount owned by user which will be sending the funds
    /// 4. `[writable]` vault_acc - SRM vault of MangoGroup
    /// 5. `[]` signer_acc - acc pointed to by signer_key
    /// 6. `[]` token_prog_acc - acc pointed to by SPL token program id
    /// 7. `[]` clock_acc - Clock sysvar account
    WithdrawSrm {
        quantity: u64
    },

    // Proxy instructions to Dex
    /// Place an order on the Serum Dex using Mango margin facilities
    ///
    /// Accounts expected by this instruction (17 + 2 * NUM_MARKETS):
    ///
    /// 0. `[writable]` mango_group_acc - MangoGroup that this margin account is for
    /// 1. `[signer]` owner_acc - MarginAccount owner
    /// 2. `[writable]` margin_account_acc - MarginAccount
    /// 3. `[]` clock_acc - Clock sysvar account
    /// 4. `[]` dex_prog_acc - program id of serum dex
    /// 5. `[writable]` spot_market_acc - serum dex MarketState
    /// 6. `[writable]` dex_request_queue_acc - serum dex request queue for this market
    /// 7. `[writable]` dex_event_queue - serum dex event queue for this market
    /// 8. `[writable]` bids_acc - serum dex bids for this market
    /// 9. `[writable]` asks_acc - serum dex asks for this market
    /// 10. `[writable]` vault_acc - mango's vault for this currency (quote if buying, base if selling)
    /// 11. `[]` signer_acc - mango signer key
    /// 12. `[writable]` dex_base_acc - serum dex market's vault for base (coin) currency
    /// 13. `[writable]` dex_quote_acc - serum dex market's vault for quote (pc) currency
    /// 14. `[]` spl token program
    /// 15. `[]` the rent sysvar
    /// 16. `[writable]` srm_vault_acc - MangoGroup's srm_vault used for fee reduction
    /// 17..17+NUM_MARKETS `[writable]` open_orders_accs - open orders for each of the spot market
    /// 17+NUM_MARKETS..17+2*NUM_MARKETS `[]`
    ///     oracle_accs - flux aggregator feed accounts
    PlaceOrder {
        order: serum_dex::instruction::NewOrderInstructionV3
    },

    /// Settle all funds from serum dex open orders into MarginAccount positions
    ///
    /// Accounts expected by this instruction (14):
    ///
    /// 0. `[writable]` mango_group_acc - MangoGroup that this margin account is for
    /// 1. `[signer]` owner_acc - MarginAccount owner
    /// 2. `[writable]` margin_account_acc - MarginAccount
    /// 3. `[]` clock_acc - Clock sysvar account
    /// 4. `[]` dex_prog_acc - program id of serum dex
    /// 5  `[writable]` spot_market_acc - dex MarketState account
    /// 6  `[writable]` open_orders_acc - open orders for this market for this MarginAccount
    /// 7. `[]` signer_acc - MangoGroup signer key
    /// 8. `[writable]` dex_base_acc - base vault for dex MarketState
    /// 9. `[writable]` dex_quote_acc - quote vault for dex MarketState
    /// 10. `[writable]` base_vault_acc - MangoGroup base vault acc
    /// 11. `[writable]` quote_vault_acc - MangoGroup quote vault acc
    /// 12. `[]` dex_signer_acc - dex Market signer account
    /// 13. `[]` spl token program
    SettleFunds,

    /// Cancel an order using dex instruction
    ///
    /// Accounts expected by this instruction (11):
    ///
    /// 0. `[writable]` mango_group_acc - MangoGroup that this margin account is for
    /// 1. `[signer]` owner_acc - MarginAccount owner
    /// 2. `[]` margin_account_acc - MarginAccount
    /// 3. `[]` clock_acc - Clock sysvar account
    /// 4. `[]` dex_prog_acc - program id of serum dex
    /// 5. `[writable]` spot_market_acc - serum dex MarketState
    /// 6. `[writable]` bids_acc - serum dex bids
    /// 7. `[writable]` asks_acc - serum dex asks
    /// 8. `[writable]` open_orders_acc - OpenOrders for the market this order belongs to
    /// 9. `[]` signer_acc - MangoGroup signer key
    /// 10. `[writable]` dex_event_queue_acc - serum dex event queue for this market
    CancelOrder {
        order: serum_dex::instruction::CancelOrderInstructionV2
    },

    /// Cancel an order using client_id
    ///
    /// Accounts expected by this instruction (11):
    ///
    /// 0. `[writable]` mango_group_acc - MangoGroup that this margin account is for
    /// 1. `[signer]` owner_acc - MarginAccount owner
    /// 2. `[]` margin_account_acc - MarginAccount
    /// 3. `[]` clock_acc - Clock sysvar account
    /// 4. `[]` dex_prog_acc - program id of serum dex
    /// 5. `[writable]` spot_market_acc - serum dex MarketState
    /// 6. `[writable]` bids_acc - serum dex bids
    /// 7. `[writable]` asks_acc - serum dex asks
    /// 8. `[writable]` open_orders_acc - OpenOrders for the market this order belongs to
    /// 9. `[]` signer_acc - MangoGroup signer key
    /// 10. `[writable]` dex_event_queue_acc - serum dex event queue for this market
    CancelOrderByClientId {
        client_id: u64
    },

    /// Change the borrow limit using admin key. This will not affect any open positions on any MarginAccount
    /// This is intended to be an instruction only in alpha stage while liquidity is slowly improved
    ///
    /// Accounts expected by this instruction (2):
    ///
    /// 0. `[writable]` mango_group_acc - MangoGroup that this margin account is for
    /// 1. `[signer]` admin_acc - admin of the MangoGroup
    ChangeBorrowLimit {
        token_index: usize,
        borrow_limit: u64
    },

    /// Place an order on the Serum Dex and settle funds from the open orders account
    ///
    /// Accounts expected by this instruction (19 + 2 * NUM_MARKETS):
    ///
    /// 0. `[writable]` mango_group_acc - MangoGroup that this margin account is for
    /// 1. `[signer]` owner_acc - MarginAccount owner
    /// 2. `[writable]` margin_account_acc - MarginAccount
    /// 3. `[]` clock_acc - Clock sysvar account
    /// 4. `[]` dex_prog_acc - program id of serum dex
    /// 5. `[writable]` spot_market_acc - serum dex MarketState
    /// 6. `[writable]` dex_request_queue_acc - serum dex request queue for this market
    /// 7. `[writable]` dex_event_queue - serum dex event queue for this market
    /// 8. `[writable]` bids_acc - serum dex bids for this market
    /// 9. `[writable]` asks_acc - serum dex asks for this market
    /// 10. `[writable]` base_vault_acc - mango vault for base currency
    /// 11. `[writable]` quote_vault_acc - mango vault for quote currency
    /// 12. `[]` signer_acc - mango signer key
    /// 13. `[writable]` dex_base_acc - serum dex market's vault for base (coin) currency
    /// 14. `[writable]` dex_quote_acc - serum dex market's vault for quote (pc) currency
    /// 15. `[]` spl token program
    /// 16. `[]` the rent sysvar
    /// 17. `[writable]` srm_vault_acc - MangoGroup's srm_vault used for fee reduction
    /// 18. `[]` dex_signer_acc - signer for serum dex MarketState
    /// 19..19+NUM_MARKETS `[writable]` open_orders_accs - open orders for each of the spot market
    /// 19+NUM_MARKETS..19+2*NUM_MARKETS `[]`
    ///     oracle_accs - flux aggregator feed accounts
    PlaceAndSettle {
        order: serum_dex::instruction::NewOrderInstructionV3
    },

    /// Allow a liquidator to cancel open orders and settle to recoup funds for partial liquidation
    ///
    /// Accounts expected by this instruction (16 + 2 * NUM_MARKETS):
    ///
    /// 0. `[writable]` mango_group_acc - MangoGroup that this margin account is for
    /// 1. `[signer]` liqor_acc - liquidator's solana account
    /// 2. `[writable]` liqee_margin_account_acc - MarginAccount of liquidatee
    /// 3. `[writable]` base_vault_acc - mango vault for base currency
    /// 4. `[writable]` quote_vault_acc - mango vault for quote currency
    /// 5. `[writable]` spot_market_acc - serum dex MarketState
    /// 6. `[writable]` bids_acc - serum dex bids for this market
    /// 7. `[writable]` asks_acc - serum dex asks for this market
    /// 8. `[]` signer_acc - mango signer key
    /// 9. `[writable]` dex_event_queue - serum dex event queue for this market
    /// 10. `[writable]` dex_base_acc - serum dex market's vault for base (coin) currency
    /// 11. `[writable]` dex_quote_acc - serum dex market's vault for quote (pc) currency
    /// 12. `[]` dex_signer_acc - signer for serum dex MarketState
    /// 13. `[]` token_prog_acc - SPL token program
    /// 14. `[]` dex_prog_acc - Serum dex program id
    /// 15. `[]` clock_acc - Clock sysvar account
    /// 16..16+NUM_MARKETS `[writable]` open_orders_accs - open orders for each of the spot market
    /// 16+NUM_MARKETS..16+2*NUM_MARKETS `[]`
    ///     oracle_accs - flux aggregator feed accounts
    ForceCancelOrders {
        /// Max orders to cancel -- could be useful to lower this if running into compute limits
        /// Recommended: 5
        limit: u8
    },

    /// Take over a MarginAccount that is below init_coll_ratio by depositing funds
    ///
    /// Accounts expected by this instruction (10 + 2 * NUM_MARKETS):
    ///
    /// 0. `[writable]` mango_group_acc - MangoGroup that this margin account is for
    /// 1. `[signer]` liqor_acc - liquidator's solana account
    /// 2. `[writable]` liqor_in_token_acc - liquidator's token account to deposit
    /// 3. `[writable]` liqor_out_token_acc - liquidator's token account to withdraw into
    /// 4. `[writable]` liqee_margin_account_acc - MarginAccount of liquidatee
    /// 5. `[writable]` in_vault_acc - Mango vault of in_token
    /// 6. `[writable]` out_vault_acc - Mango vault of out_token
    /// 7. `[]` signer_acc
    /// 8. `[]` token_prog_acc - Token program id
    /// 9. `[]` clock_acc - Clock sysvar account
    /// 10..10+NUM_MARKETS `[]` open_orders_accs - open orders for each of the spot market
    /// 10+NUM_MARKETS..10+2*NUM_MARKETS `[]`
    ///     oracle_accs - flux aggregator feed accounts
    PartialLiquidate {
        /// Quantity of the token being deposited to repay borrows
        max_deposit: u64
    },


    AddMarginAccountInfo {
        info: [u8; INFO_LEN]
    }
}


impl MangoInstruction {
    pub fn unpack(input: &[u8]) -> Option<Self> {
        let (&discrim, data) = array_refs![input, 4; ..;];
        let discrim = u32::from_le_bytes(discrim);
        Some(match discrim {
            0 => {
                let data = array_ref![data, 0, 40 + 8 * NUM_TOKENS];
                let (
                    signer_nonce,
                    maint_coll_ratio,
                    init_coll_ratio,
                    borrow_limits
                ) = array_refs![data, 8, 16, 16, 8 * NUM_TOKENS];

                let mut aligned_borrow_limits = [0u64; NUM_TOKENS];
                let buffer: &mut [u8] = cast_slice_mut(&mut aligned_borrow_limits);
                buffer.copy_from_slice(borrow_limits);

                MangoInstruction::InitMangoGroup {
                    signer_nonce: u64::from_le_bytes(*signer_nonce),
                    maint_coll_ratio: U64F64::from_le_bytes(*maint_coll_ratio),
                    init_coll_ratio: U64F64::from_le_bytes(*init_coll_ratio),
                    borrow_limits: aligned_borrow_limits
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
                let data = array_ref![data, 0, 8];
                MangoInstruction::Withdraw {
                    quantity: u64::from_le_bytes(*data)
                }
            },
            4 => {
                let data = array_ref![data, 0, 16];
                let (token_index, quantity) = array_refs![data, 8, 8];

                MangoInstruction::Borrow {
                    token_index: usize::from_le_bytes(*token_index),
                    quantity: u64::from_le_bytes(*quantity)
                }
            },
            5 => {
                let data = array_ref![data, 0, 16];
                let (token_index, quantity) = array_refs![data, 8, 8];

                MangoInstruction::SettleBorrow {
                    token_index: usize::from_le_bytes(*token_index),
                    quantity: u64::from_le_bytes(*quantity)
                }
            },
            6 => {
                if data.len() < 8 * NUM_TOKENS { return None; }
                let data = array_ref![data, 0, 8 * NUM_TOKENS];

                let mut aligned_arr = [0u64; NUM_TOKENS];
                let buffer: &mut [u8] = cast_slice_mut(&mut aligned_arr);
                buffer.copy_from_slice(data);

                let deposit_quantities: &[u64] = cast_slice(buffer);
                let deposit_quantities = array_ref![deposit_quantities, 0, NUM_TOKENS];
                MangoInstruction::Liquidate {
                    deposit_quantities: *deposit_quantities
                }
            },
            7 => {
                let quantity = array_ref![data, 0, 8];
                MangoInstruction::DepositSrm { quantity: u64::from_le_bytes(*quantity) }
            }
            8 => {
                let quantity = array_ref![data, 0, 8];
                MangoInstruction::WithdrawSrm { quantity: u64::from_le_bytes(*quantity) }
            }
            9 => {
                let data_arr = array_ref![data, 0, 46];
                let order = unpack_dex_new_order_v3(data_arr)?;
                MangoInstruction::PlaceOrder {
                    order
                }

            },
            10 => {
                MangoInstruction::SettleFunds
            },
            11 => {
                let data_array = array_ref![data, 0, 20];
                let fields = array_refs![data_array, 4, 16];
                let side = match u32::from_le_bytes(*fields.0) {
                    0 => serum_dex::matching::Side::Bid,
                    1 => serum_dex::matching::Side::Ask,
                    _ => return None,
                };
                let order_id = u128::from_le_bytes(*fields.1);
                let order = serum_dex::instruction::CancelOrderInstructionV2 {
                    side,
                    order_id,
                };

                MangoInstruction::CancelOrder {
                    order
                }
            },
            12 => {
                let client_id = array_ref![data, 0, 8];
                MangoInstruction::CancelOrderByClientId {
                    client_id: u64::from_le_bytes(*client_id)
                }

            }
            13 => {
                let data = array_ref![data, 0, 16];
                let (token_index, borrow_limit) = array_refs![data, 8, 8];
                MangoInstruction::ChangeBorrowLimit {
                    token_index: usize::from_le_bytes(*token_index),
                    borrow_limit: u64::from_le_bytes(*borrow_limit)
                }
            }
            14 => {
                let data_arr = array_ref![data, 0, 46];
                let order = unpack_dex_new_order_v3(data_arr)?;
                MangoInstruction::PlaceAndSettle {
                    order
                }
            }
            15 => {
                let limit = array_ref![data, 0, 1];
                MangoInstruction::ForceCancelOrders {
                    limit: u8::from_le_bytes(*limit)
                }
            }
            16 => {
                let max_deposit = array_ref![data, 0, 8];
                MangoInstruction::PartialLiquidate {
                    max_deposit: u64::from_le_bytes(*max_deposit)
                }
            }
            17 => {
                let info = array_ref![data, 0, INFO_LEN];
                MangoInstruction::AddMarginAccountInfo {
                    info: *info
                }
            }
            _ => { return None; }
        })
    }
    pub fn pack(&self) -> Vec<u8> {
        bincode::serialize(self).unwrap()
    }
}


fn unpack_dex_new_order_v3(data: &[u8; 46]) -> Option<serum_dex::instruction::NewOrderInstructionV3> {
    let (
        &side_arr,
        &price_arr,
        &max_coin_qty_arr,
        &max_native_pc_qty_arr,
        &self_trade_behavior_arr,
        &otype_arr,
        &client_order_id_bytes,
        &limit_arr,
    ) = array_refs![data, 4, 8, 8, 8, 4, 4, 8, 2];

    let side = serum_dex::matching::Side::try_from_primitive(u32::from_le_bytes(side_arr).try_into().ok()?).ok()?;
    let limit_price = NonZeroU64::new(u64::from_le_bytes(price_arr))?;
    let max_coin_qty = NonZeroU64::new(u64::from_le_bytes(max_coin_qty_arr))?;
    let max_native_pc_qty_including_fees =
        NonZeroU64::new(u64::from_le_bytes(max_native_pc_qty_arr))?;
    let self_trade_behavior = serum_dex::instruction::SelfTradeBehavior::try_from_primitive(
        u32::from_le_bytes(self_trade_behavior_arr)
            .try_into()
            .ok()?,
    )
        .ok()?;
    let order_type = serum_dex::matching::OrderType::try_from_primitive(u32::from_le_bytes(otype_arr).try_into().ok()?).ok()?;
    let client_order_id = u64::from_le_bytes(client_order_id_bytes);
    let limit = u16::from_le_bytes(limit_arr);

    Some(serum_dex::instruction::NewOrderInstructionV3 {
        side,
        limit_price,
        max_coin_qty,
        max_native_pc_qty_including_fees,
        self_trade_behavior,
        order_type,
        client_order_id,
        limit,
    })
}


pub fn init_mango_group(
    program_id: &Pubkey,
    mango_group_pk: &Pubkey,
    signer_pk: &Pubkey,
    dex_prog_id: &Pubkey,
    srm_vault_pk: &Pubkey,
    admin_pk: &Pubkey,
    mint_pks: &[Pubkey],
    vault_pks: &[Pubkey],
    spot_market_pks: &[Pubkey],
    oracle_pks: &[Pubkey],
    signer_nonce: u64,
    maint_coll_ratio: U64F64,
    init_coll_ratio: U64F64,
    borrow_limits: [u64; NUM_TOKENS]
) -> Result<Instruction, ProgramError> {
    let mut accounts = vec![
        AccountMeta::new(*mango_group_pk, false),
        AccountMeta::new_readonly(solana_program::sysvar::rent::ID, false),
        AccountMeta::new_readonly(solana_program::sysvar::clock::ID, false),
        AccountMeta::new_readonly(*signer_pk, false),
        AccountMeta::new_readonly(*dex_prog_id, false),
        AccountMeta::new_readonly(*srm_vault_pk, false),
        AccountMeta::new_readonly(*admin_pk, true)
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
    accounts.extend(oracle_pks.iter().map(
        |pk| AccountMeta::new_readonly(*pk, false))
    );

    let instr = MangoInstruction::InitMangoGroup {
        signer_nonce,
        maint_coll_ratio,
        init_coll_ratio,
        borrow_limits
    };

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
) -> Result<Instruction, ProgramError> {
    let accounts = vec![
        AccountMeta::new_readonly(*mango_group_pk, false),
        AccountMeta::new(*margin_account_pk, false),
        AccountMeta::new_readonly(*owner_pk, true),
        AccountMeta::new_readonly(solana_program::sysvar::rent::ID, false),
    ];

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
    token_account_pk: &Pubkey,
    vault_pk: &Pubkey,
    quantity: u64
) -> Result<Instruction, ProgramError> {
    let accounts = vec![
        AccountMeta::new(*mango_group_pk, false),
        AccountMeta::new(*margin_account_pk, false),
        AccountMeta::new_readonly(*owner_pk, true),
        AccountMeta::new(*token_account_pk, false),
        AccountMeta::new(*vault_pk, false),
        AccountMeta::new_readonly(spl_token::ID, false),
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
    token_account_pk: &Pubkey,
    vault_pk: &Pubkey,
    signer_pk: &Pubkey,
    open_orders_pks: &[Pubkey],
    oracle_pks: &[Pubkey],
    quantity: u64
) -> Result<Instruction, ProgramError> {
    let mut accounts = vec![
        AccountMeta::new(*mango_group_pk, false),
        AccountMeta::new(*margin_account_pk, false),
        AccountMeta::new_readonly(*owner_pk, true),
        AccountMeta::new(*token_account_pk, false),
        AccountMeta::new(*vault_pk, false),
        AccountMeta::new_readonly(*signer_pk, false),
        AccountMeta::new_readonly(spl_token::ID, false),
        AccountMeta::new_readonly(solana_program::sysvar::clock::ID, false),
    ];

    accounts.extend(open_orders_pks.iter().map(
        |pk| AccountMeta::new_readonly(*pk, false))
    );
    accounts.extend(oracle_pks.iter().map(
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

pub fn borrow(
    program_id: &Pubkey,
    mango_group_pk: &Pubkey,
    margin_account_pk: &Pubkey,
    owner_pk: &Pubkey,
    open_orders_pks: &[Pubkey],
    oracle_pks: &[Pubkey],
    token_index: usize,
    quantity: u64
) -> Result<Instruction, ProgramError> {
    let mut accounts = vec![
        AccountMeta::new(*mango_group_pk, false),
        AccountMeta::new(*margin_account_pk, false),
        AccountMeta::new_readonly(*owner_pk, true),
        AccountMeta::new_readonly(solana_program::sysvar::clock::ID, false),
    ];

    accounts.extend(open_orders_pks.iter().map(
        |pk| AccountMeta::new_readonly(*pk, false))
    );
    accounts.extend(oracle_pks.iter().map(
        |pk| AccountMeta::new_readonly(*pk, false))
    );

    let instr = MangoInstruction::Borrow { token_index, quantity };
    let data = instr.pack();
    Ok(Instruction {
        program_id: *program_id,
        accounts,
        data
    })
}

pub fn settle_borrow(
    program_id: &Pubkey,
    mango_group_pk: &Pubkey,
    margin_account_pk: &Pubkey,
    owner_pk: &Pubkey,
    token_index: usize,
    quantity: u64
) -> Result<Instruction, ProgramError> {
    let accounts = vec![
        AccountMeta::new(*mango_group_pk, false),
        AccountMeta::new(*margin_account_pk, false),
        AccountMeta::new_readonly(*owner_pk, true),
        AccountMeta::new_readonly(solana_program::sysvar::clock::ID, false),
    ];

    let instr = MangoInstruction::SettleBorrow { token_index, quantity };
    let data = instr.pack();
    Ok(Instruction {
        program_id: *program_id,
        accounts,
        data
    })
}

pub fn liquidate(
    program_id: &Pubkey,
    mango_group_pk: &Pubkey,
    liqor_pk: &Pubkey,
    liqee_margin_account_pk: &Pubkey,
    open_orders_pks: &[Pubkey],
    oracle_pks: &[Pubkey],
    vault_pks: &[Pubkey],
    liqor_token_account_pks: &[Pubkey],
    deposit_quantities: [u64; NUM_TOKENS]
) -> Result<Instruction, ProgramError> {
    let mut accounts = vec![
        AccountMeta::new(*mango_group_pk, false),
        AccountMeta::new_readonly(*liqor_pk, true),
        AccountMeta::new(*liqee_margin_account_pk, false),
        AccountMeta::new_readonly(spl_token::ID, false),
        AccountMeta::new_readonly(solana_program::sysvar::clock::ID, false),
    ];

    accounts.extend(open_orders_pks.iter().map(
        |pk| AccountMeta::new_readonly(*pk, false))
    );
    accounts.extend(oracle_pks.iter().map(
        |pk| AccountMeta::new_readonly(*pk, false))
    );
    accounts.extend(vault_pks.iter().map(
        |pk| AccountMeta::new(*pk, false))
    );
    accounts.extend(liqor_token_account_pks.iter().map(
        |pk| AccountMeta::new(*pk, false))
    );

    let instr = MangoInstruction::Liquidate { deposit_quantities };
    let data = instr.pack();
    Ok(Instruction {
        program_id: *program_id,
        accounts,
        data
    })
}

pub fn deposit_srm(
    program_id: &Pubkey,
    mango_group_pk: &Pubkey,
    mango_srm_account_pk: &Pubkey,
    owner_pk: &Pubkey,
    srm_account_pk: &Pubkey,
    vault_pk: &Pubkey,
    quantity: u64
) -> Result<Instruction, ProgramError> {
    let accounts = vec![
        AccountMeta::new(*mango_group_pk, false),
        AccountMeta::new(*mango_srm_account_pk, false),
        AccountMeta::new_readonly(*owner_pk, true),
        AccountMeta::new(*srm_account_pk, false),
        AccountMeta::new(*vault_pk, false),
        AccountMeta::new_readonly(spl_token::ID, false),
        AccountMeta::new_readonly(solana_program::sysvar::clock::ID, false),
        AccountMeta::new_readonly(solana_program::sysvar::rent::ID, false),
    ];

    let instr = MangoInstruction::DepositSrm { quantity };
    let data = instr.pack();
    Ok(Instruction {
        program_id: *program_id,
        accounts,
        data
    })
}

pub fn withdraw_srm(
    program_id: &Pubkey,
    mango_group_pk: &Pubkey,
    mango_srm_account_pk: &Pubkey,
    owner_pk: &Pubkey,
    srm_account_pk: &Pubkey,
    vault_pk: &Pubkey,
    signer_pk: &Pubkey,
    quantity: u64
) -> Result<Instruction, ProgramError> {
    let accounts = vec![
        AccountMeta::new(*mango_group_pk, false),
        AccountMeta::new(*mango_srm_account_pk, false),
        AccountMeta::new_readonly(*owner_pk, true),
        AccountMeta::new(*srm_account_pk, false),
        AccountMeta::new(*vault_pk, false),
        AccountMeta::new_readonly(*signer_pk, false),
        AccountMeta::new_readonly(spl_token::ID, false),
        AccountMeta::new_readonly(solana_program::sysvar::clock::ID, false),
    ];

    let instr = MangoInstruction::WithdrawSrm { quantity };
    let data = instr.pack();
    Ok(Instruction {
        program_id: *program_id,
        accounts,
        data
    })
}

pub fn place_order(
    program_id: &Pubkey,
    mango_group_pk: &Pubkey,
    owner_pk: &Pubkey,
    margin_account_pk: &Pubkey,
    dex_prog_id: &Pubkey,
    spot_market_pk: &Pubkey,
    dex_request_queue_pk: &Pubkey,
    dex_event_queue_pk: &Pubkey,
    bids_pk: &Pubkey,
    asks_pk: &Pubkey,
    vault_pk: &Pubkey,
    signer_pk: &Pubkey,
    dex_base_pk: &Pubkey,
    dex_quote_pk: &Pubkey,
    srm_vault_pk: &Pubkey,
    open_orders_pks: &[Pubkey],
    oracle_pks: &[Pubkey],
    order: serum_dex::instruction::NewOrderInstructionV3
) -> Result<Instruction, ProgramError> {

    let mut accounts = vec![
        AccountMeta::new(*mango_group_pk, false),
        AccountMeta::new_readonly(*owner_pk, true),
        AccountMeta::new(*margin_account_pk, false),
        AccountMeta::new_readonly(solana_program::sysvar::clock::ID, false),
        AccountMeta::new_readonly(*dex_prog_id, false),
        AccountMeta::new(*spot_market_pk, false),
        AccountMeta::new(*dex_request_queue_pk, false),
        AccountMeta::new(*dex_event_queue_pk, false),
        AccountMeta::new(*bids_pk, false),
        AccountMeta::new(*asks_pk, false),
        AccountMeta::new(*vault_pk, false),
        AccountMeta::new_readonly(*signer_pk, false),
        AccountMeta::new(*dex_base_pk, false),
        AccountMeta::new(*dex_quote_pk, false),
        AccountMeta::new_readonly(spl_token::ID, false),
        AccountMeta::new_readonly(solana_program::sysvar::rent::ID, false),
        AccountMeta::new(*srm_vault_pk, false),
    ];

    accounts.extend(open_orders_pks.iter().map(
        |pk| AccountMeta::new(*pk, false))
    );
    accounts.extend(oracle_pks.iter().map(
        |pk| AccountMeta::new_readonly(*pk, false))
    );

    let instr = MangoInstruction::PlaceOrder { order };
    let data = instr.pack();
    Ok(Instruction {
        program_id: *program_id,
        accounts,
        data
    })
}

pub fn place_and_settle(
    program_id: &Pubkey,
    mango_group_pk: &Pubkey,
    owner_pk: &Pubkey,
    margin_account_pk: &Pubkey,
    dex_prog_id: &Pubkey,
    spot_market_pk: &Pubkey,
    dex_request_queue_pk: &Pubkey,
    dex_event_queue_pk: &Pubkey,
    bids_pk: &Pubkey,
    asks_pk: &Pubkey,
    base_vault_pk: &Pubkey,
    quote_vault_pk: &Pubkey,
    signer_pk: &Pubkey,
    dex_base_pk: &Pubkey,
    dex_quote_pk: &Pubkey,
    srm_vault_pk: &Pubkey,
    dex_signer_pk: &Pubkey,
    open_orders_pks: &[Pubkey],
    oracle_pks: &[Pubkey],
    order: serum_dex::instruction::NewOrderInstructionV3
) -> Result<Instruction, ProgramError> {

    let mut accounts = vec![
        AccountMeta::new(*mango_group_pk, false),
        AccountMeta::new_readonly(*owner_pk, true),
        AccountMeta::new(*margin_account_pk, false),
        AccountMeta::new_readonly(solana_program::sysvar::clock::ID, false),
        AccountMeta::new_readonly(*dex_prog_id, false),
        AccountMeta::new(*spot_market_pk, false),
        AccountMeta::new(*dex_request_queue_pk, false),
        AccountMeta::new(*dex_event_queue_pk, false),
        AccountMeta::new(*bids_pk, false),
        AccountMeta::new(*asks_pk, false),
        AccountMeta::new(*base_vault_pk, false),
        AccountMeta::new(*quote_vault_pk, false),
        AccountMeta::new_readonly(*signer_pk, false),
        AccountMeta::new(*dex_base_pk, false),
        AccountMeta::new(*dex_quote_pk, false),
        AccountMeta::new_readonly(spl_token::ID, false),
        AccountMeta::new_readonly(solana_program::sysvar::rent::ID, false),
        AccountMeta::new(*srm_vault_pk, false),
        AccountMeta::new_readonly(*dex_signer_pk, false),
    ];

    accounts.extend(open_orders_pks.iter().map(
        |pk| AccountMeta::new(*pk, false))
    );
    accounts.extend(oracle_pks.iter().map(
        |pk| AccountMeta::new_readonly(*pk, false))
    );

    let instr = MangoInstruction::PlaceAndSettle { order };
    let data = instr.pack();
    Ok(Instruction {
        program_id: *program_id,
        accounts,
        data
    })
}


pub fn settle_funds(
    program_id: &Pubkey,
    mango_group_pk: &Pubkey,
    owner_pk: &Pubkey,
    margin_account_pk: &Pubkey,
    dex_prog_id: &Pubkey,
    spot_market_pk: &Pubkey,
    open_orders_pk: &Pubkey,
    signer_pk: &Pubkey,
    dex_base_pk: &Pubkey,
    dex_quote_pk: &Pubkey,
    base_vault_pk: &Pubkey,
    quote_vault_pk: &Pubkey,
    dex_signer_pk: &Pubkey,
) -> Result<Instruction, ProgramError> {

    let accounts = vec![
        AccountMeta::new(*mango_group_pk, false),
        AccountMeta::new_readonly(*owner_pk, true),
        AccountMeta::new(*margin_account_pk, false),
        AccountMeta::new_readonly(solana_program::sysvar::clock::ID, false),
        AccountMeta::new_readonly(*dex_prog_id, false),
        AccountMeta::new(*spot_market_pk, false),
        AccountMeta::new(*open_orders_pk, false),
        AccountMeta::new_readonly(*signer_pk, false),
        AccountMeta::new(*dex_base_pk, false),
        AccountMeta::new(*dex_quote_pk, false),
        AccountMeta::new(*base_vault_pk, false),
        AccountMeta::new(*quote_vault_pk, false),
        AccountMeta::new_readonly(*dex_signer_pk, false),
        AccountMeta::new_readonly(spl_token::ID, false),
    ];

    let instr = MangoInstruction::SettleFunds;
    let data = instr.pack();
    Ok(Instruction {
        program_id: *program_id,
        accounts,
        data
    })
}

pub fn cancel_order(
    program_id: &Pubkey,
    mango_group_pk: &Pubkey,
    owner_pk: &Pubkey,
    margin_account_pk: &Pubkey,
    dex_prog_id: &Pubkey,
    spot_market_pk: &Pubkey,
    bids_pk: &Pubkey,
    asks_pk: &Pubkey,
    open_orders_pk: &Pubkey,
    signer_pk: &Pubkey,
    dex_event_queue_pk: &Pubkey,
    order: serum_dex::instruction::CancelOrderInstructionV2
) -> Result<Instruction, ProgramError> {
    let accounts = vec![
        AccountMeta::new(*mango_group_pk, false),
        AccountMeta::new_readonly(*owner_pk, true),
        AccountMeta::new_readonly(*margin_account_pk, false),
        AccountMeta::new_readonly(solana_program::sysvar::clock::ID, false),
        AccountMeta::new_readonly(*dex_prog_id, false),
        AccountMeta::new(*spot_market_pk, false),
        AccountMeta::new(*bids_pk, false),
        AccountMeta::new(*asks_pk, false),
        AccountMeta::new(*open_orders_pk, false),
        AccountMeta::new_readonly(*signer_pk, false),
        AccountMeta::new(*dex_event_queue_pk, false),
    ];

    let instr = MangoInstruction::CancelOrder { order };
    let data = instr.pack();
    Ok(Instruction {
        program_id: *program_id,
        accounts,
        data
    })
}

pub fn cancel_order_by_client_id(
    program_id: &Pubkey,
    mango_group_pk: &Pubkey,
    owner_pk: &Pubkey,
    margin_account_pk: &Pubkey,
    dex_prog_id: &Pubkey,
    spot_market_pk: &Pubkey,
    bids_pk: &Pubkey,
    asks_pk: &Pubkey,
    open_orders_pk: &Pubkey,
    signer_pk: &Pubkey,
    dex_event_queue_pk: &Pubkey,
    client_id: u64
) -> Result<Instruction, ProgramError> {
    let accounts = vec![
        AccountMeta::new(*mango_group_pk, false),
        AccountMeta::new_readonly(*owner_pk, true),
        AccountMeta::new_readonly(*margin_account_pk, false),
        AccountMeta::new_readonly(solana_program::sysvar::clock::ID, false),
        AccountMeta::new_readonly(*dex_prog_id, false),
        AccountMeta::new(*spot_market_pk, false),
        AccountMeta::new(*bids_pk, false),
        AccountMeta::new(*asks_pk, false),
        AccountMeta::new(*open_orders_pk, false),
        AccountMeta::new_readonly(*signer_pk, false),
        AccountMeta::new(*dex_event_queue_pk, false),
    ];
    let instr = MangoInstruction::CancelOrderByClientId { client_id };
    let data = instr.pack();
    Ok(Instruction {
        program_id: *program_id,
        accounts,
        data
    })
}


pub fn change_borrow_limit(
    program_id: &Pubkey,
    mango_group_pk: &Pubkey,
    admin_pk: &Pubkey,
    token_index: usize,
    borrow_limit: u64
) -> Result<Instruction, ProgramError> {
    let accounts = vec![
        AccountMeta::new(*mango_group_pk, false),
        AccountMeta::new_readonly(*admin_pk, true),
    ];

    let instr = MangoInstruction::ChangeBorrowLimit { token_index, borrow_limit };
    let data = instr.pack();
    Ok(Instruction {
        program_id: *program_id,
        accounts,
        data
    })
}

pub fn force_cancel_orders(
    program_id: &Pubkey,
    mango_group_pk: &Pubkey,
    liqor_pk: &Pubkey,
    liqee_margin_account_acc: &Pubkey,
    base_vault_pk: &Pubkey,
    quote_vault_pk: &Pubkey,
    spot_market_pk: &Pubkey,
    bids_pk: &Pubkey,
    asks_pk: &Pubkey,
    signer_pk: &Pubkey,
    dex_event_queue_pk: &Pubkey,
    dex_base_pk: &Pubkey,
    dex_quote_pk: &Pubkey,
    dex_signer_pk: &Pubkey,
    dex_prog_id: &Pubkey,
    open_orders_pks: &[Pubkey],
    oracle_pks: &[Pubkey],
    limit: u8
) -> Result<Instruction, ProgramError> {

    let mut accounts = vec![
        AccountMeta::new(*mango_group_pk, false),
        AccountMeta::new_readonly(*liqor_pk, true),
        AccountMeta::new(*liqee_margin_account_acc, false),
        AccountMeta::new(*base_vault_pk, false),
        AccountMeta::new(*quote_vault_pk, false),
        AccountMeta::new(*spot_market_pk, false),
        AccountMeta::new(*bids_pk, false),
        AccountMeta::new(*asks_pk, false),
        AccountMeta::new_readonly(*signer_pk, false),
        AccountMeta::new(*dex_event_queue_pk, false),
        AccountMeta::new(*dex_base_pk, false),
        AccountMeta::new(*dex_quote_pk, false),
        AccountMeta::new_readonly(*dex_signer_pk, false),
        AccountMeta::new_readonly(spl_token::ID, false),
        AccountMeta::new_readonly(*dex_prog_id, false),
        AccountMeta::new_readonly(solana_program::sysvar::clock::ID, false),
    ];

    accounts.extend(open_orders_pks.iter().map(
        |pk| AccountMeta::new(*pk, false))
    );
    accounts.extend(oracle_pks.iter().map(
        |pk| AccountMeta::new_readonly(*pk, false))
    );

    let instr = MangoInstruction::ForceCancelOrders { limit };
    let data = instr.pack();
    Ok(Instruction {
        program_id: *program_id,
        accounts,
        data
    })
}


pub fn partial_liquidate(
    program_id: &Pubkey,
    mango_group_pk: &Pubkey,
    liqor_pk: &Pubkey,
    liqor_in_token_pk: &Pubkey,
    liqor_out_token_pk: &Pubkey,
    liqee_margin_account_acc: &Pubkey,
    in_vault_pk: &Pubkey,
    out_vault_pk: &Pubkey,
    signer_pk: &Pubkey,
    open_orders_pks: &[Pubkey],
    oracle_pks: &[Pubkey],
    max_deposit: u64
) -> Result<Instruction, ProgramError> {

    let mut accounts = vec![
        AccountMeta::new(*mango_group_pk, false),
        AccountMeta::new_readonly(*liqor_pk, true),
        AccountMeta::new(*liqor_in_token_pk, false),
        AccountMeta::new(*liqor_out_token_pk, false),
        AccountMeta::new(*liqee_margin_account_acc, false),
        AccountMeta::new(*in_vault_pk, false),
        AccountMeta::new(*out_vault_pk, false),
        AccountMeta::new_readonly(*signer_pk, false),
        AccountMeta::new_readonly(spl_token::ID, false),
        AccountMeta::new_readonly(solana_program::sysvar::clock::ID, false),
    ];

    accounts.extend(open_orders_pks.iter().map(
        |pk| AccountMeta::new_readonly(*pk, false))
    );
    accounts.extend(oracle_pks.iter().map(
        |pk| AccountMeta::new_readonly(*pk, false))
    );

    let instr = MangoInstruction::PartialLiquidate { max_deposit };
    let data = instr.pack();
    Ok(Instruction {
        program_id: *program_id,
        accounts,
        data
    })
}

pub fn add_margin_account_info(
    program_id: &Pubkey,
    mango_group_pk: &Pubkey,
    margin_account_pk: &Pubkey,
    owner_pk: &Pubkey,
    info: [u8; INFO_LEN]
) -> Result<Instruction, ProgramError> {
    let accounts = vec![
        AccountMeta::new(*mango_group_pk, false),
        AccountMeta::new(*margin_account_pk, false),
        AccountMeta::new_readonly(*owner_pk, true),
    ];

    let instr = MangoInstruction::AddMarginAccountInfo { info };
    let data = instr.pack();
    Ok(Instruction {
        program_id: *program_id,
        accounts,
        data
    })
}