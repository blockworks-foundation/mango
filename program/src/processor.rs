use std::cell::{Ref, RefMut};
use std::mem::size_of;

use arrayref::{array_ref, array_refs};
use fixed::types::U64F64;
use serum_dex::state::ToAlignedBytes;
use solana_program::account_info::AccountInfo;
use solana_program::clock::Clock;
use solana_program::entrypoint::ProgramResult;
use solana_program::msg;
use solana_program::program_error::ProgramError;
use solana_program::program_pack::{IsInitialized, Pack};
use solana_program::pubkey::Pubkey;
use solana_program::rent::Rent;
use solana_program::sysvar::Sysvar;
use spl_token::state::Account;

use crate::state::{AccountFlag, Loadable, MangoGroup, MangoIndex, MarginAccount, NUM_MARKETS, NUM_TOKENS, load_bids_mut, load_asks_mut, load_open_orders};
use crate::utils::{gen_signer_key, get_dex_best_price, gen_signer_seeds};
use crate::instruction::MangoInstruction;

pub struct Processor {}


impl Processor {
    fn init_mango_group(
        program_id: &Pubkey,
        accounts: &[AccountInfo],
        signer_nonce: u64
    ) -> ProgramResult {
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

        assert_eq!(mango_group_acc.owner, program_id);
        assert_eq!(mango_group.account_flags, 0);
        mango_group.account_flags = (AccountFlag::Initialized | AccountFlag::MangoGroup).bits();

        assert!(rent.is_exempt(mango_group_acc.lamports(), size_of::<MangoGroup>()));

        assert_eq!(gen_signer_key(signer_nonce, mango_group_acc.key, program_id)?, *signer_acc.key);
        mango_group.signer_nonce = signer_nonce;
        mango_group.signer_key = *signer_acc.key;
        mango_group.dex_program_id = *dex_prog_acc.key;
        mango_group.total_deposits = [U64F64::from_num(0); NUM_TOKENS];
        mango_group.total_borrows = [U64F64::from_num(0); NUM_TOKENS];

        let quote_mint_acc = &token_mint_accs[NUM_MARKETS];
        let quote_vault_acc = &vault_accs[NUM_MARKETS];
        let quote_vault = Account::unpack(&quote_vault_acc.try_borrow_data()?)?;
        assert!(quote_vault.is_initialized());
        assert_eq!(&quote_vault.owner, signer_acc.key);
        assert_eq!(&quote_vault.mint, quote_mint_acc.key);
        assert_eq!(quote_vault_acc.owner, &spl_token::id());

        let curr_ts = clock.unix_timestamp as u64;

        for i in 0..NUM_MARKETS {
            let spot_market_acc = &spot_market_accs[i];
            let spot_market = serum_dex::state::MarketState::load(
                spot_market_acc, dex_prog_acc.key
            )?;
            let base_mint_acc = &token_mint_accs[i];
            let base_vault_acc = &vault_accs[i];
            let base_vault = Account::unpack(&base_vault_acc.try_borrow_data()?)?;
            assert!(base_vault.is_initialized());
            assert_eq!(&base_vault.owner, signer_acc.key);
            assert_eq!(&base_vault.mint, base_mint_acc.key);
            assert_eq!(base_vault_acc.owner, &spl_token::id());

            let sm_base_mint = spot_market.coin_mint;
            let sm_quote_mint = spot_market.pc_mint;
            assert_eq!(sm_base_mint, base_mint_acc.key.to_aligned_bytes());
            assert_eq!(sm_quote_mint, quote_mint_acc.key.to_aligned_bytes());
            mango_group.spot_markets[i] = *spot_market_acc.key;
            mango_group.tokens[i] = *base_mint_acc.key;
            mango_group.vaults[i] = *base_vault_acc.key;


            // TODO what to initialize index to?
            mango_group.indexes[i] = MangoIndex {
                last_update: curr_ts,
                borrow: U64F64::from_num(1),
                deposit: U64F64::from_num(1)  // Smallest unit of interest is 0.0001% or 0.000001
            }
        }

        Ok(())
    }

    fn init_margin_account(
        program_id: &Pubkey,
        accounts: &[AccountInfo]
    ) -> ProgramResult {
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
        assert_eq!(mango_group.account_flags, (AccountFlag::Initialized | AccountFlag::MangoGroup).bits());
        assert_eq!(mango_group_acc.owner, program_id);

        let mut margin_account = MarginAccount::load_mut(margin_account_acc)?;
        let rent = Rent::from_account_info(rent_acc)?;

        assert_eq!(margin_account_acc.owner, program_id);
        assert!(rent.is_exempt(margin_account_acc.lamports(), size_of::<MarginAccount>()));
        assert_eq!(margin_account.account_flags, 0);
        assert!(owner_acc.is_signer);

        margin_account.account_flags = (AccountFlag::Initialized | AccountFlag::MarginAccount).bits();
        margin_account.mango_group = *mango_group_acc.key;
        margin_account.owner = *owner_acc.key;

        for i in 0..NUM_MARKETS {
            let open_orders_acc = &open_orders_accs[i];
            let open_orders = load_open_orders(open_orders_acc)?;

            assert!(rent.is_exempt(open_orders_acc.lamports(), size_of::<serum_dex::state::OpenOrders>()));
            let open_orders_flags = open_orders.account_flags;
            assert_eq!(open_orders_flags, 0);
            assert_eq!(open_orders_acc.owner, &mango_group.dex_program_id);

            margin_account.open_orders[i] = *open_orders_acc.key;
        }

        // TODO is this necessary?
        margin_account.deposits = [U64F64::from_num(0); NUM_TOKENS];
        margin_account.borrows = [U64F64::from_num(0); NUM_TOKENS];
        margin_account.positions = [0; NUM_TOKENS];

        Ok(())
    }

    fn deposit(program_id: &Pubkey, accounts: &[AccountInfo], quantity: u64) -> ProgramResult {
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
        assert!(owner_acc.is_signer);

        // TODO move this into load_mut_checked function
        let mut mango_group = MangoGroup::load_mut(mango_group_acc)?;
        assert_eq!(mango_group.account_flags, (AccountFlag::Initialized | AccountFlag::MangoGroup).bits());
        assert_eq!(mango_group_acc.owner, program_id);

        let mut margin_account = MarginAccount::load_mut(margin_account_acc)?;
        assert_eq!(margin_account.account_flags, (AccountFlag::Initialized | AccountFlag::MarginAccount).bits());
        assert_eq!(&margin_account.owner, owner_acc.key);
        assert_eq!(&margin_account.mango_group, mango_group_acc.key);

        let token_index = mango_group.get_token_index(mint_acc.key).unwrap();
        assert_eq!(&mango_group.vaults[token_index], vault_acc.key);

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

    fn withdraw(program_id: &Pubkey, accounts: &[AccountInfo], quantity: u64) -> ProgramResult {
        const NUM_FIXED: usize = 8;
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
            token_prog_acc,
            clock_acc,
        ] = fixed_accs;
        assert!(owner_acc.is_signer);

        // TODO move this into load_mut_checked function
        let mut mango_group = MangoGroup::load_mut(mango_group_acc)?;
        assert_eq!(mango_group.account_flags, (AccountFlag::Initialized | AccountFlag::MangoGroup).bits());
        assert_eq!(mango_group_acc.owner, program_id);

        let mut margin_account = MarginAccount::load_mut(margin_account_acc)?;
        assert_eq!(margin_account.account_flags, (AccountFlag::Initialized | AccountFlag::MarginAccount).bits());
        assert_eq!(&margin_account.owner, owner_acc.key);
        assert_eq!(&margin_account.mango_group, mango_group_acc.key);

        let token_index = mango_group.get_token_index(mint_acc.key).unwrap();
        assert_eq!(&mango_group.vaults[token_index], vault_acc.key);

        let clock = Clock::from_account_info(clock_acc)?;
        mango_group.update_indexes(&clock)?;

        let prices = Self::get_prices(&mango_group, spot_market_accs, bids_accs, asks_accs)?;
        let free_equity = margin_account.get_free_equity(&mango_group, &prices, open_orders_accs)?;
        let val_withdraw = prices[token_index] * U64F64::from_num(quantity);
        assert!(free_equity >= val_withdraw);

        // let withdraw_instruction = spl_token::instruction::transfer(
        //     token_prog_acc.key,
        //     vault_acc.key,
        //     token_account_acc.key,
        //     ,
        //     &[],
        //     quantity
        // )?;
        // let withdraw_accs = [
        //     vault_acc.clone(),
        //     user_quote_acc.clone(),
        //     omega_signer_acc.clone(),
        //     spl_token_program_acc.clone()
        // ];
        // let signer_seeds = gen_signer_seeds(&omega_contract.signer_nonce, omega_contract_acc.key);
        // solana_program::program::invoke_signed(&withdraw_instruction, &withdraw_accs, &[&signer_seeds])?;


        Ok(())
    }

    fn get_prices(
        mango_group: &MangoGroup,
        spot_market_accs: &[AccountInfo],
        bids_accs: &[AccountInfo],
        asks_accs: &[AccountInfo]
    ) -> Result<[U64F64; NUM_TOKENS], ProgramError> {
        // Determine prices from serum dex (TODO in the future use oracle)
        let mut prices = [U64F64::from_num(0); NUM_TOKENS];
        prices[NUM_MARKETS] = U64F64::from_num(1);  // quote currency is 1


        for i in 0..NUM_MARKETS {
            let spot_market_acc = &spot_market_accs[i];
            assert_eq!(&mango_group.spot_markets[i], spot_market_acc.key);
            let spot_market = serum_dex::state::MarketState::load(
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
    ) -> ProgramResult {
        let instruction = MangoInstruction::unpack(data).ok_or(ProgramError::InvalidInstructionData)?;
        match instruction {
            MangoInstruction::InitMangoGroup {
                signer_nonce
            } => {
                msg!("InitMangoGroup");
                Self::init_mango_group(program_id, accounts, signer_nonce)?;
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
            MangoInstruction::Liquidate => {}
            MangoInstruction::PlaceOrder => {}
            MangoInstruction::SettleFunds => {}
            MangoInstruction::CancelOrder => {}
            MangoInstruction::CancelOrderByClientId => {}
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