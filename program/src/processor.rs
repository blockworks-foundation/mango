use std::mem::size_of;

use arrayref::{array_ref, array_refs};
use fixed::types::U64F64;
use serum_dex::state::ToAlignedBytes;
use solana_program::account_info::AccountInfo;
use solana_program::clock::Clock;
use solana_program::msg;
use solana_program::program_error::ProgramError;
use solana_program::program_pack::{IsInitialized, Pack};
use solana_program::pubkey::Pubkey;
use solana_program::rent::Rent;
use solana_program::sysvar::Sysvar;
use spl_token::state::Account;

use crate::error::{check_assert, MangoResult, SourceFileId};
use crate::instruction::MangoInstruction;
use crate::state::{AccountFlag, load_asks_mut, load_bids_mut, load_market_state, load_open_orders,
                   Loadable, MangoGroup, MangoIndex, MarginAccount, NUM_MARKETS, NUM_TOKENS};
use crate::utils::{gen_signer_key, gen_signer_seeds, get_dex_best_price};

macro_rules! prog_assert {
    ($cond:expr) => {
        check_assert($cond, line!() as u16, SourceFileId::Processor)
    }
}
macro_rules! prog_assert_eq {
    ($x:expr, $y:expr) => {
        check_assert($x == $y, line!() as u16, SourceFileId::Processor)
    }
}

pub struct Processor {}

impl Processor {
    fn init_mango_group(
        program_id: &Pubkey,
        accounts: &[AccountInfo],
        signer_nonce: u64,
        maint_coll_ratio: U64F64,
        init_coll_ratio: U64F64
    ) -> MangoResult<()> {
        const NUM_FIXED: usize = 5;
        let accounts = array_ref![accounts, 0, NUM_FIXED + 2 * NUM_TOKENS + NUM_MARKETS];
        let (fixed_accs, token_mint_accs, vault_accs, spot_market_accs) =
            array_refs![accounts, NUM_FIXED, NUM_TOKENS, NUM_TOKENS, NUM_MARKETS];

        let [
            mango_group_acc,
            rent_acc,
            clock_acc,
            signer_acc,
            dex_prog_acc
        ] = fixed_accs;

        let rent = Rent::from_account_info(rent_acc)?;
        let clock = Clock::from_account_info(clock_acc)?;
        let mut mango_group = MangoGroup::load_mut(mango_group_acc)?;

        prog_assert_eq!(mango_group_acc.owner, program_id)?;
        prog_assert_eq!(mango_group.account_flags, 0)?;
        mango_group.account_flags = (AccountFlag::Initialized | AccountFlag::MangoGroup).bits();

        prog_assert!(rent.is_exempt(mango_group_acc.lamports(), size_of::<MangoGroup>()))?;

        prog_assert_eq!(gen_signer_key(signer_nonce, mango_group_acc.key, program_id)?, *signer_acc.key)?;
        mango_group.signer_nonce = signer_nonce;
        mango_group.signer_key = *signer_acc.key;
        mango_group.dex_program_id = *dex_prog_acc.key;
        mango_group.total_deposits = [U64F64::from_num(0); NUM_TOKENS];
        mango_group.total_borrows = [U64F64::from_num(0); NUM_TOKENS];
        mango_group.maint_coll_ratio = maint_coll_ratio;
        mango_group.init_coll_ratio = init_coll_ratio;
        let curr_ts = clock.unix_timestamp as u64;
        for i in 0..NUM_TOKENS {
            let mint_acc = &token_mint_accs[i];
            let vault_acc = &vault_accs[i];
            let vault = Account::unpack(&vault_acc.try_borrow_data()?)?;
            prog_assert!(vault.is_initialized())?;
            prog_assert_eq!(&vault.owner, signer_acc.key)?;
            prog_assert_eq!(&vault.mint, mint_acc.key)?;
            prog_assert_eq!(vault_acc.owner, &spl_token::id())?;
            mango_group.tokens[i] = *mint_acc.key;
            mango_group.vaults[i] = *vault_acc.key;
            mango_group.indexes[i] = MangoIndex {
                last_update: curr_ts,
                borrow: U64F64::from_num(1),
                deposit: U64F64::from_num(1)  // Smallest unit of interest is 0.0001% or 0.000001
            }
        }

        for i in 0..NUM_MARKETS {
            let spot_market_acc: &AccountInfo = &spot_market_accs[i];
            let spot_market = load_market_state(
                spot_market_acc, dex_prog_acc.key
            )?;
            let sm_base_mint = spot_market.coin_mint;
            let sm_quote_mint = spot_market.pc_mint;
            prog_assert_eq!(sm_base_mint, token_mint_accs[i].key.to_aligned_bytes())?;
            prog_assert_eq!(sm_quote_mint, token_mint_accs[NUM_MARKETS].key.to_aligned_bytes())?;
            mango_group.spot_markets[i] = *spot_market_acc.key;
        }

        Ok(())
    }

    fn init_margin_account(
        program_id: &Pubkey,
        accounts: &[AccountInfo]
    ) -> MangoResult<()> {
        const NUM_FIXED: usize = 4;
        let accounts = array_ref![accounts, 0, NUM_FIXED + NUM_MARKETS];
        let (fixed_accs, open_orders_accs) = array_refs![accounts, NUM_FIXED, NUM_MARKETS];

        let [
            mango_group_acc,
            margin_account_acc,
            owner_acc,
            rent_acc
        ] = fixed_accs;

        let mango_group = MangoGroup::load(mango_group_acc)?;
        prog_assert_eq!(mango_group.account_flags, (AccountFlag::Initialized | AccountFlag::MangoGroup).bits())?;
        prog_assert_eq!(mango_group_acc.owner, program_id)?;

        let mut margin_account = MarginAccount::load_mut(margin_account_acc)?;
        let rent = Rent::from_account_info(rent_acc)?;

        prog_assert_eq!(margin_account_acc.owner, program_id)?;
        prog_assert!(rent.is_exempt(margin_account_acc.lamports(), size_of::<MarginAccount>()))?;
        prog_assert_eq!(margin_account.account_flags, 0)?;
        prog_assert!(owner_acc.is_signer)?;

        margin_account.account_flags = (AccountFlag::Initialized | AccountFlag::MarginAccount).bits();
        margin_account.mango_group = *mango_group_acc.key;
        margin_account.owner = *owner_acc.key;

        for i in 0..NUM_MARKETS {
            let open_orders_acc = &open_orders_accs[i];
            let open_orders = load_open_orders(open_orders_acc)?;

            prog_assert!(rent.is_exempt(open_orders_acc.lamports(), size_of::<serum_dex::state::OpenOrders>()))?;
            let open_orders_flags = open_orders.account_flags;
            prog_assert_eq!(open_orders_flags, 0)?;
            prog_assert_eq!(open_orders_acc.owner, &mango_group.dex_program_id)?;

            margin_account.open_orders[i] = *open_orders_acc.key;
        }

        // TODO is this necessary?
        margin_account.deposits = [U64F64::from_num(0); NUM_TOKENS];
        margin_account.borrows = [U64F64::from_num(0); NUM_TOKENS];
        margin_account.positions = [0; NUM_TOKENS];

        Ok(())
    }

    fn deposit(program_id: &Pubkey, accounts: &[AccountInfo], quantity: u64) -> MangoResult<()> {
        const NUM_FIXED: usize = 8;
        let accounts = array_ref![accounts, 0, NUM_FIXED];
        let [
            mango_group_acc,
            margin_account_acc,
            owner_acc,
            mint_acc,
            token_account_acc,
            vault_acc,
            token_prog_acc,
            clock_acc,
        ] = accounts;
        // prog_assert!(owner_acc.is_signer)?; // anyone can deposit, not just owner

        // TODO move this into load_mut_checked function
        let mut mango_group = MangoGroup::load_mut(mango_group_acc)?;
        prog_assert_eq!(mango_group.account_flags, (AccountFlag::Initialized | AccountFlag::MangoGroup).bits())?;
        prog_assert_eq!(mango_group_acc.owner, program_id)?;

        let mut margin_account = MarginAccount::load_mut(margin_account_acc)?;
        prog_assert_eq!(margin_account.account_flags, (AccountFlag::Initialized | AccountFlag::MarginAccount).bits())?;
        // prog_assert_eq!(&margin_account.owner, owner_acc.key)?;  // this check not necessary here
        prog_assert_eq!(&margin_account.mango_group, mango_group_acc.key)?;

        let token_index = mango_group.get_token_index(mint_acc.key).unwrap();
        prog_assert_eq!(&mango_group.vaults[token_index], vault_acc.key)?;

        let clock = Clock::from_account_info(clock_acc)?;
        mango_group.update_indexes(&clock)?;

        let deposit_instruction = spl_token::instruction::transfer(
            &spl_token::id(),
            token_account_acc.key,
            vault_acc.key,
            &owner_acc.key, &[], quantity
        )?;
        let deposit_accs = [
            token_account_acc.clone(),
            vault_acc.clone(),
            owner_acc.clone(),
            token_prog_acc.clone()
        ];

        solana_program::program::invoke_signed(&deposit_instruction, &deposit_accs, &[])?;

        let deposit: U64F64 = U64F64::from_num(quantity) / mango_group.indexes[token_index].deposit;
        margin_account.deposits[token_index] += deposit;
        mango_group.total_deposits[token_index] += deposit;

        Ok(())
    }

    fn withdraw(program_id: &Pubkey, accounts: &[AccountInfo], quantity: u64) -> MangoResult<()> {
        const NUM_FIXED: usize = 9;
        let accounts = array_ref![accounts, 0, NUM_FIXED + 4 * NUM_MARKETS];
        let (
            fixed_accs,
            open_orders_accs,
            spot_market_accs,
            bids_accs,
            asks_accs
        ) = array_refs![accounts, NUM_FIXED, NUM_MARKETS, NUM_MARKETS, NUM_MARKETS, NUM_MARKETS];

        let [
            mango_group_acc,
            margin_account_acc,
            owner_acc,
            mint_acc,
            token_account_acc,
            vault_acc,
            signer_acc,
            token_prog_acc,
            clock_acc,
        ] = fixed_accs;

        let mut mango_group = MangoGroup::load_mut_checked(mango_group_acc, program_id)?;
        let mut margin_account = MarginAccount::load_mut_checked(
            margin_account_acc, mango_group_acc.key)?;
        prog_assert!(owner_acc.is_signer)?;
        prog_assert_eq!(&margin_account.owner, owner_acc.key)?;

        for i in 0..NUM_MARKETS {
            // TODO if open orders initialized make sure it has proper owner else it's 0
            prog_assert_eq!(open_orders_accs[i].key, &margin_account.open_orders[i])?;
        }

        let token_index = mango_group.get_token_index(mint_acc.key).unwrap();
        prog_assert_eq!(&mango_group.vaults[token_index], vault_acc.key)?;

        let clock = Clock::from_account_info(clock_acc)?;
        mango_group.update_indexes(&clock)?;

        let index: &MangoIndex = &mango_group.indexes[token_index];
        let available: u64 = (margin_account.deposits[token_index] * index.deposit).to_num();
        prog_assert!(available >= quantity)?;  // TODO just borrow (quantity - available)

        let prices = Self::get_prices(&mango_group, spot_market_accs, bids_accs, asks_accs)?;
        let free_equity = margin_account.get_free_equity(&mango_group, &prices, open_orders_accs)?;
        let val_withdraw = prices[token_index] * U64F64::from_num(quantity);
        prog_assert!(free_equity >= val_withdraw)?;

        let withdraw_instruction = spl_token::instruction::transfer(
            &spl_token::ID,
            vault_acc.key,
            token_account_acc.key,
            signer_acc.key,
            &[],
            quantity
        )?;
        let withdraw_accs = [
            vault_acc.clone(),
            token_account_acc.clone(),
            signer_acc.clone(),
            token_prog_acc.clone()
        ];
        let signer_seeds = gen_signer_seeds(&mango_group.signer_nonce, mango_group_acc.key);
        solana_program::program::invoke_signed(&withdraw_instruction, &withdraw_accs, &[&signer_seeds])?;

        let withdrew: U64F64 = U64F64::from_num(quantity) / index.deposit;
        margin_account.deposits[token_index] -= withdrew;
        mango_group.total_deposits[token_index] -= withdrew;

        Ok(())
    }

    fn liquidate(
        program_id: &Pubkey,
        accounts: &[AccountInfo],
        deposit_quantities: [u64; NUM_TOKENS]
    ) -> MangoResult<()> {
        const NUM_FIXED: usize = 5;
        let accounts = array_ref![accounts, 0, NUM_FIXED + 4 * NUM_MARKETS + 2 * NUM_TOKENS];
        let (
            fixed_accs,
            open_orders_accs,
            spot_market_accs,
            bids_accs,
            asks_accs,
            vaults_accs,
            liqor_token_account_accs
        ) = array_refs![accounts, NUM_FIXED, NUM_MARKETS, NUM_MARKETS, NUM_MARKETS, NUM_MARKETS, NUM_TOKENS, NUM_TOKENS];

        let [
        mango_group_acc,
        liqor_acc,
        liqee_margin_account_acc,
        token_prog_acc,
        clock_acc
        ] = fixed_accs;

        // margin ratio = equity / val(borrowed)
        // equity = val(positions) - val(borrowed) + val(collateral)
        prog_assert!(liqor_acc.is_signer)?;
        let mut mango_group = MangoGroup::load_mut_checked(mango_group_acc, program_id)?;
        let mut liqee_margin_account = MarginAccount::load_mut_checked(
            liqee_margin_account_acc, mango_group_acc.key
        )?;
        let clock = Clock::from_account_info(clock_acc)?;
        mango_group.update_indexes(&clock)?;

        for i in 0..NUM_MARKETS {
            // TODO if open orders initialized make sure it has proper owner else it's 0
            // TODO what if user deletes open orders after initializing (it is owned by dex so only dex can delete)
            prog_assert_eq!(open_orders_accs[i].key, &liqee_margin_account.open_orders[i])?;
        }

        let prices = Self::get_prices(&mango_group, spot_market_accs, bids_accs, asks_accs)?;
        let assets_val = liqee_margin_account.get_assets_val(&mango_group, &prices, open_orders_accs)?;
        let liabs_val = liqee_margin_account.get_liabs_val(&mango_group, &prices)?;

        prog_assert!(liabs_val > U64F64::from_num(0))?;
        let collateral_ratio: U64F64 = assets_val / liabs_val;

        // No liquidations if account above maint collateral ratio
        prog_assert!(collateral_ratio < mango_group.maint_coll_ratio)?;

        // Determine if the amount liqor's deposits can bring this account above init_coll_ratio
        let mut new_deposits_val = U64F64::from_num(0);
        for i in 0..NUM_TOKENS {
            new_deposits_val += prices[i] * U64F64::from_num(deposit_quantities[i]);
        }
        prog_assert!((assets_val + new_deposits_val) / liabs_val >= mango_group.init_coll_ratio)?;

        // Pull deposits from liqor's token wallets
        for i in 0..NUM_TOKENS {
            let quantity = deposit_quantities[i];
            if quantity == 0 {
                continue;
            }

            let vault_acc: &AccountInfo = &vaults_accs[i];
            let token_account_acc: &AccountInfo = &liqor_token_account_accs[i];
            let deposit_instruction = spl_token::instruction::transfer(
                &spl_token::id(),
                token_account_acc.key,
                vault_acc.key,
                &liqor_acc.key, &[], quantity
            )?;
            let deposit_accs = [
                token_account_acc.clone(),
                vault_acc.clone(),
                liqor_acc.clone(),
                token_prog_acc.clone()
            ];

            solana_program::program::invoke_signed(&deposit_instruction, &deposit_accs, &[])?;

            let deposit: U64F64 = U64F64::from_num(quantity) / mango_group.indexes[i].deposit;
            liqee_margin_account.deposits[i] += deposit;
            mango_group.total_deposits[i] += deposit;
        }

        // If all deposits are good, transfer ownership of margin account to liqor
        liqee_margin_account.owner = *liqor_acc.key;

        Ok(())
    }

    // fn settle_borrows(
    //     program_id: &Pubkey,
    //     accounts: &[AccountInfo],
    //     quantity: u64
    // ) -> MangoResult<()> {
    //     // Use all value from positions, open orders and deposits to reduce borrows
    //     // It is expected that the client will make the trades necessary to do this
    //
    //     Ok(())
    // }
    //
    // fn borrow(
    //     token_index: u64,
    //     quantity: u64,
    // ) -> MangoResult<()> {
    //     /*
    //     Verify there is enough margin to borrow
    //     Borrow by incrementing margin_account.borrows and positions
    //
    //      */
    //     Ok(())
    // }


    fn get_prices(
        mango_group: &MangoGroup,
        spot_market_accs: &[AccountInfo],
        bids_accs: &[AccountInfo],
        asks_accs: &[AccountInfo]
    ) -> MangoResult<[U64F64; NUM_TOKENS]> {
        // Determine prices from serum dex (TODO in the future use oracle)
        let mut prices = [U64F64::from_num(0); NUM_TOKENS];
        prices[NUM_MARKETS] = U64F64::from_num(1);  // quote currency is 1

        for i in 0..NUM_MARKETS {
            let spot_market_acc = &spot_market_accs[i];
            prog_assert_eq!(&mango_group.spot_markets[i], spot_market_acc.key)?;
            let spot_market = load_market_state(
                spot_market_acc, &mango_group.dex_program_id
            )?;

            let bids = load_bids_mut(&spot_market, &bids_accs[i])?;
            let asks = load_asks_mut(&spot_market, &asks_accs[i])?;

            let bid_price = get_dex_best_price(bids, true);
            let ask_price = get_dex_best_price(asks, false);

            let lot_size_adj = U64F64::from_num(spot_market.pc_lot_size) / U64F64::from_num(spot_market.coin_lot_size);
            prices[i] = match (bid_price, ask_price) {  // TODO better error
                (None, None) => { panic!("No orders on the book!") },
                (Some(b), None) => U64F64::from_num(b) * lot_size_adj,
                (None, Some(a)) => U64F64::from_num(a) * lot_size_adj,
                (Some(b), Some(a)) => lot_size_adj * U64F64::from_num(b + a) / 2  // TODO checked add
            };
        }
        Ok(prices)
    }

    pub fn process(
        program_id: &Pubkey,
        accounts: &[AccountInfo],
        data: &[u8]
    ) -> MangoResult<()> {
        let instruction = MangoInstruction::unpack(data).ok_or(ProgramError::InvalidInstructionData)?;
        match instruction {
            MangoInstruction::InitMangoGroup {
                signer_nonce, maint_coll_ratio, init_coll_ratio
            } => {
                msg!("InitMangoGroup");
                Self::init_mango_group(program_id, accounts, signer_nonce, maint_coll_ratio, init_coll_ratio)?;
            }
            MangoInstruction::InitMarginAccount => {
                msg!("InitMarginAccount");
                Self::init_margin_account(program_id, accounts)?;
            }
            MangoInstruction::Deposit {
                quantity
            } => {
                msg!("Deposit");
                Self::deposit(program_id, accounts, quantity)?;
            }
            MangoInstruction::Withdraw {
                quantity
            } => {
                msg!("Withdraw");
                Self::withdraw(program_id, accounts, quantity)?;
            }
            MangoInstruction::Liquidate {

            } => {
                // Either user takes the position
                // Or the program can liquidate on the serum dex (in case no liquidator wants to take pos)
                msg!("Liquidate")

            }
            MangoInstruction::PlaceOrder => {

            }
            MangoInstruction::SettleFunds => {

            }
            MangoInstruction::CancelOrder => {

            }
            MangoInstruction::CancelOrderByClientId => {

            }
        }
        Ok(())
    }

}




/*
TODO
Initial launch
- UI
- provide liquidity
- liquidation bot
- cranks
- testing
- oracle program + bot
 */

/*
Perp Bond
- cleaner
- no way to enforce loss on bond holders
- risk horizon is potentially infinite
-
 */

/*
FMB (Fixed Maturity Bond)
- enforcers keep a list of all who have liab balances and submit at settlement
- liab holders may set if they want auto roll and to which bond they want to auto roll
-

 */

/*
Lending Pool
- Enforcers periodically update index based on time past and interest rate
- https://docs.dydx.exchange/#interest
 */

/*
Dynamic Expansion



 */