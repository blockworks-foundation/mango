use std::cmp;
use std::cmp::min;
use std::mem::size_of;

use arrayref::{array_ref, array_refs};
use fixed::types::U64F64;
use fixed_macro::types::U64F64;
use flux_aggregator::borsh_state::InitBorshState;
use serum_dex::matching::Side;
use serum_dex::state::ToAlignedBytes;
use solana_program::account_info::AccountInfo;
use solana_program::clock::Clock;
use solana_program::entrypoint::ProgramResult;
use solana_program::instruction::{AccountMeta, Instruction};
use solana_program::msg;
use solana_program::program_error::ProgramError;
use solana_program::program_pack::{IsInitialized, Pack};
use solana_program::pubkey::Pubkey;
use solana_program::rent::Rent;
use solana_program::sysvar::Sysvar;
use spl_token::state::{Account, Mint};

use crate::error::{check_assert, MangoError, MangoErrorCode, MangoResult, SourceFileId};
use crate::instruction::MangoInstruction;
use crate::state::{AccountFlag, check_open_orders, DUST_THRESHOLD, load_asks_mut, load_bids_mut, load_market_state, load_open_orders, Loadable, MangoGroup, MangoIndex, MangoSrmAccount, MarginAccount, NUM_MARKETS, NUM_TOKENS, ONE_U64F64, PARTIAL_LIQ_INCENTIVE, ZERO_U64F64, INFO_LEN};
use crate::utils::{gen_signer_key, gen_signer_seeds};

macro_rules! check_default {
    ($cond:expr) => {
        check_assert($cond, MangoErrorCode::Default, line!(), SourceFileId::Processor)
    }
}

macro_rules! check_eq_default {
    ($x:expr, $y:expr) => {
        check_assert($x == $y, MangoErrorCode::Default, line!(), SourceFileId::Processor)
    }
}


macro_rules! check {
    ($cond:expr, $err:expr) => {
        check_assert($cond, $err, line!(), SourceFileId::Processor)
    }
}

macro_rules! check_eq {
    ($x:expr, $y:expr, $err:expr) => {
        check_assert($x == $y, $err, line!(), SourceFileId::Processor)
    }
}

macro_rules! throw_err {
    ($err:expr) => {
        Err(MangoError::MangoErrorCode { mango_error_code: $err, line: line!(), source_file_id: SourceFileId::Processor })
    }
}

pub mod srm_token {
    use solana_program::declare_id;

    #[cfg(feature = "devnet")]
    declare_id!("9FbAMDvXqNjPqZSYt4EWTguJuDrGkfvwr3gSFpiSbX9S");
    #[cfg(not(feature = "devnet"))]
    declare_id!("SRMuApVNdxXokk5GT7XD5cUUgXMBCoAz2LHeuAoKWRt");
}

pub const LIQ_MIN_COLL_RATIO: U64F64 = U64F64!(1.01);

pub struct Processor {}

impl Processor {
    #[inline(never)]
    fn init_mango_group(
        program_id: &Pubkey,
        accounts: &[AccountInfo],
        signer_nonce: u64,
        maint_coll_ratio: U64F64,
        init_coll_ratio: U64F64,
        borrow_limits: [u64; NUM_TOKENS]
    ) -> MangoResult<()> {
        const NUM_FIXED: usize = 7;
        let accounts = array_ref![accounts, 0, NUM_FIXED + 2 * NUM_TOKENS + 2 * NUM_MARKETS];
        let (
            fixed_accs,
            token_mint_accs,
            vault_accs,
            spot_market_accs,
            oracle_accs,
        ) = array_refs![accounts, NUM_FIXED, NUM_TOKENS, NUM_TOKENS, NUM_MARKETS, NUM_MARKETS];

        let [
            mango_group_acc,
            rent_acc,
            clock_acc,
            signer_acc,
            dex_prog_acc,
            srm_vault_acc,
            admin_acc
        ] = fixed_accs;

        // Note: no need to check rent and clock because they're being checked in from_account_info
        let rent = Rent::from_account_info(rent_acc)?;
        let clock = Clock::from_account_info(clock_acc)?;

        // TODO this may not be necessary since load_mut maps the data and will fail if size incorrect
        check_eq!(size_of::<MangoGroup>(), mango_group_acc.data_len(), MangoErrorCode::InvalidMangoGroupSize)?;

        let mut mango_group = MangoGroup::load_mut(mango_group_acc)?;

        check_eq!(mango_group_acc.owner, program_id, MangoErrorCode::InvalidGroupOwner)?;
        check_eq!(mango_group.account_flags, 0, MangoErrorCode::InvalidGroupFlags)?;
        mango_group.account_flags = (AccountFlag::Initialized | AccountFlag::MangoGroup).bits();

        check!(rent.is_exempt(mango_group_acc.lamports(), size_of::<MangoGroup>()), MangoErrorCode::GroupNotRentExempt)?;
        check!(gen_signer_key(signer_nonce, mango_group_acc.key, program_id)? == *signer_acc.key, MangoErrorCode::InvalidSignerKey)?;
        mango_group.signer_nonce = signer_nonce;
        mango_group.signer_key = *signer_acc.key;
        mango_group.dex_program_id = *dex_prog_acc.key;
        mango_group.maint_coll_ratio = maint_coll_ratio;
        mango_group.init_coll_ratio = init_coll_ratio;

        // verify SRM vault is valid then set
        let srm_vault = Account::unpack(&srm_vault_acc.try_borrow_data()?)?;
        check!(srm_vault.is_initialized(), MangoErrorCode::Default)?;
        check_eq!(&srm_vault.owner, signer_acc.key, MangoErrorCode::Default)?;
        check_eq!(srm_token::ID, srm_vault.mint, MangoErrorCode::Default)?;
        check_eq!(srm_vault_acc.owner, &spl_token::id(), MangoErrorCode::Default)?;
        mango_group.srm_vault = *srm_vault_acc.key;

        // Set the admin key and make sure it's a signer
        check!(admin_acc.is_signer, MangoErrorCode::Default)?;
        mango_group.admin = *admin_acc.key;
        mango_group.borrow_limits = borrow_limits;

        let curr_ts = clock.unix_timestamp as u64;
        for i in 0..NUM_TOKENS {
            let mint_acc = &token_mint_accs[i];
            let mint = Mint::unpack(&mint_acc.try_borrow_data()?)?;
            let vault_acc = &vault_accs[i];
            let vault = Account::unpack(&vault_acc.try_borrow_data()?)?;
            check!(vault.is_initialized(), MangoErrorCode::Default)?;
            check_eq!(&vault.owner, signer_acc.key, MangoErrorCode::Default)?;
            check_eq!(&vault.mint, mint_acc.key, MangoErrorCode::Default)?;
            check_eq!(vault_acc.owner, &spl_token::id(), MangoErrorCode::Default)?;
            mango_group.tokens[i] = *mint_acc.key;
            mango_group.vaults[i] = *vault_acc.key;
            mango_group.indexes[i] = MangoIndex {
                last_update: curr_ts,
                borrow: ONE_U64F64,
                deposit: ONE_U64F64  // Smallest unit of interest is 0.0001% or 0.000001
            };
            mango_group.mint_decimals[i] = mint.decimals;
        }

        for i in 0..NUM_MARKETS {
            let spot_market_acc: &AccountInfo = &spot_market_accs[i];
            let spot_market = load_market_state(
                spot_market_acc, dex_prog_acc.key
            )?;
            let sm_base_mint = spot_market.coin_mint;
            let sm_quote_mint = spot_market.pc_mint;
            check_eq!(sm_base_mint, token_mint_accs[i].key.to_aligned_bytes(), MangoErrorCode::Default)?;
            check_eq!(sm_quote_mint, token_mint_accs[NUM_MARKETS].key.to_aligned_bytes(), MangoErrorCode::Default)?;
            mango_group.spot_markets[i] = *spot_market_acc.key;
            mango_group.oracles[i] = *oracle_accs[i].key;

            let oracle = flux_aggregator::state::Aggregator::load_initialized(&oracle_accs[i])?;
            mango_group.oracle_decimals[i] = oracle.config.decimals;
        }

        Ok(())
    }

    #[inline(never)]
    fn init_margin_account(
        program_id: &Pubkey,
        accounts: &[AccountInfo]
    ) -> MangoResult<()> {
        const NUM_FIXED: usize = 4;
        let accounts = array_ref![accounts, 0, NUM_FIXED];

        let [
            mango_group_acc,
            margin_account_acc,
            owner_acc,
            rent_acc
        ] = accounts;

        let _mango_group = MangoGroup::load_checked(mango_group_acc, program_id)?;
        let mut margin_account = MarginAccount::load_mut(margin_account_acc)?;
        let rent = Rent::from_account_info(rent_acc)?;

        check_eq_default!(margin_account_acc.owner, program_id)?;
        check_default!(rent.is_exempt(margin_account_acc.lamports(), size_of::<MarginAccount>()))?;
        check_eq_default!(margin_account.account_flags, 0)?;
        check_default!(owner_acc.is_signer)?;

        margin_account.account_flags = (AccountFlag::Initialized | AccountFlag::MarginAccount).bits();
        margin_account.mango_group = *mango_group_acc.key;
        margin_account.owner = *owner_acc.key;

        Ok(())
    }

    #[inline(never)]
    fn deposit(
        program_id: &Pubkey,
        accounts: &[AccountInfo],
        quantity: u64
    ) -> MangoResult<()> {
        const NUM_FIXED: usize = 7;
        let accounts = array_ref![accounts, 0, NUM_FIXED];
        let [
            mango_group_acc,
            margin_account_acc,
            owner_acc,
            token_account_acc,
            vault_acc,
            token_prog_acc,
            clock_acc,
        ] = accounts;

        let mut mango_group = MangoGroup::load_mut_checked(mango_group_acc, program_id)?;
        let mut margin_account = MarginAccount::load_mut_checked(
            program_id, margin_account_acc, mango_group_acc.key
        )?;

        let clock = Clock::from_account_info(clock_acc)?;
        mango_group.update_indexes(&clock)?;


        let token_index = mango_group.get_token_index_with_vault(vault_acc.key).unwrap();
        check_eq_default!(&mango_group.vaults[token_index], vault_acc.key)?;

        check_eq_default!(token_prog_acc.key, &spl_token::id())?;
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
        checked_add_deposit(&mut mango_group, &mut margin_account, token_index, deposit)?;
        settle_borrow_full_unchecked(&mut mango_group, &mut margin_account, token_index)?;

        Ok(())
    }

    #[inline(never)]
    fn withdraw(
        program_id: &Pubkey,
        accounts: &[AccountInfo],
        quantity: u64
    ) -> MangoResult<()> {

        const NUM_FIXED: usize = 8;
        let accounts = array_ref![accounts, 0, NUM_FIXED + 2 * NUM_MARKETS];
        let (
            fixed_accs,
            open_orders_accs,
            oracle_accs,
        ) = array_refs![accounts, NUM_FIXED, NUM_MARKETS, NUM_MARKETS];

        let [
            mango_group_acc,
            margin_account_acc,
            owner_acc,
            token_account_acc,
            vault_acc,
            signer_acc,
            token_prog_acc,
            clock_acc,
        ] = fixed_accs;


        let mut mango_group = MangoGroup::load_mut_checked(
            mango_group_acc, program_id
        )?;
        let mut margin_account = MarginAccount::load_mut_checked(
            program_id, margin_account_acc, mango_group_acc.key
        )?;

        let clock = Clock::from_account_info(clock_acc)?;
        mango_group.update_indexes(&clock)?;

        check_default!(owner_acc.is_signer)?;
        check_eq_default!(&margin_account.owner, owner_acc.key)?;

        for i in 0..NUM_MARKETS {
            check_eq_default!(open_orders_accs[i].key, &margin_account.open_orders[i])?;
            check_open_orders(&open_orders_accs[i], signer_acc.key)?;
        }

        let token_index = mango_group.get_token_index_with_vault(vault_acc.key).unwrap();
        check_eq_default!(&mango_group.vaults[token_index], vault_acc.key)?;

        let index: &MangoIndex = &mango_group.indexes[token_index];
        let native_deposits: u64 = (margin_account.deposits[token_index].checked_mul(index.deposit).unwrap()).to_num();
        let available = native_deposits;

        check!(available >= quantity, MangoErrorCode::InsufficientFunds)?;
        // TODO just borrow (quantity - available)
        let prices = get_prices(&mango_group, oracle_accs)?;
        // Withdraw from deposit
        let withdrew: U64F64 = U64F64::from_num(quantity) / index.deposit;
        checked_sub_deposit(&mut mango_group, &mut margin_account, token_index, withdrew)?;

        // Make sure accounts are in valid state after withdrawal
        let coll_ratio = margin_account.get_collateral_ratio(&mango_group, &prices, open_orders_accs)?;
        check!(coll_ratio >= mango_group.init_coll_ratio, MangoErrorCode::CollateralRatioLimit)?;
        check_default!(mango_group.has_valid_deposits_borrows(token_index))?;

        // Send out withdraw instruction to SPL token program
        check_eq_default!(token_prog_acc.key, &spl_token::id())?;
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

        Ok(())
    }

    #[inline(never)]
    fn borrow(
        program_id: &Pubkey,
        accounts: &[AccountInfo],
        token_index: usize,
        quantity: u64
    ) -> MangoResult<()> {
        const NUM_FIXED: usize = 4;
        let accounts = array_ref![accounts, 0, NUM_FIXED + 2 * NUM_MARKETS];
        let (
            fixed_accs,
            open_orders_accs,
            oracle_accs,
        ) = array_refs![accounts, NUM_FIXED, NUM_MARKETS, NUM_MARKETS];

        let [
            mango_group_acc,
            margin_account_acc,
            owner_acc,
            clock_acc,
        ] = fixed_accs;

        let mut mango_group = MangoGroup::load_mut_checked(mango_group_acc, program_id)?;
        let mut margin_account = MarginAccount::load_mut_checked(
            program_id, margin_account_acc, mango_group_acc.key
        )?;
        check_default!(owner_acc.is_signer)?;
        check_eq_default!(&margin_account.owner, owner_acc.key)?;

        for i in 0..NUM_MARKETS {
            check_eq_default!(open_orders_accs[i].key, &margin_account.open_orders[i])?;
            check_open_orders(&open_orders_accs[i], &mango_group.signer_key)?;
        }
        let clock = Clock::from_account_info(clock_acc)?;
        mango_group.update_indexes(&clock)?;

        let index: MangoIndex = mango_group.indexes[token_index];

        let borrow = U64F64::from_num(quantity) / index.borrow;
        let deposit = U64F64::from_num(quantity) / index.deposit;

        checked_add_deposit(&mut mango_group, &mut margin_account, token_index, deposit)?;
        checked_add_borrow(&mut mango_group, &mut margin_account, token_index, borrow)?;

        let prices = get_prices(&mango_group, oracle_accs)?;
        let coll_ratio = margin_account.get_collateral_ratio(&mango_group, &prices, open_orders_accs)?;

        check_default!(coll_ratio >= mango_group.init_coll_ratio)?;
        check_default!(mango_group.has_valid_deposits_borrows(token_index))?;
        Ok(())
    }

    #[inline(never)]
    fn settle_borrow(
        program_id: &Pubkey,
        accounts: &[AccountInfo],
        token_index: usize,
        quantity: u64
    ) -> MangoResult<()> {
        const NUM_FIXED: usize = 4;
        let accounts = array_ref![accounts, 0, NUM_FIXED];
        let [
            mango_group_acc,
            margin_account_acc,
            owner_acc,
            clock_acc,
        ] = accounts;

        let mut mango_group = MangoGroup::load_mut_checked(mango_group_acc, program_id)?;
        let mut margin_account = MarginAccount::load_mut_checked(
            program_id, margin_account_acc, mango_group_acc.key
        )?;
        let clock = Clock::from_account_info(clock_acc)?;
        mango_group.update_indexes(&clock)?;
        check_default!(owner_acc.is_signer)?;
        check_eq_default!(&margin_account.owner, owner_acc.key)?;

        settle_borrow_unchecked(&mut mango_group, &mut margin_account, token_index, quantity)?;
        Ok(())
    }

    #[inline(never)]
    fn liquidate(
        _program_id: &Pubkey,
        _accounts: &[AccountInfo],
        _deposit_quantities: [u64; NUM_TOKENS]
    ) -> MangoResult<()> {
        throw_err!(MangoErrorCode::Deprecated)
    }

    #[inline(never)]
    fn deposit_srm(
        program_id: &Pubkey,
        accounts: &[AccountInfo],
        quantity: u64
    ) -> MangoResult<()> {

        const NUM_FIXED: usize = 8;
        let accounts = array_ref![accounts, 0, NUM_FIXED];
        let [
            mango_group_acc,
            mango_srm_account_acc,
            owner_acc,
            srm_account_acc,
            vault_acc,
            token_prog_acc,
            clock_acc,
            rent_acc,
        ] = accounts;
        // prog_assert!(owner_acc.is_signer)?; // anyone can deposit, not just owner

        let mut mango_group = MangoGroup::load_mut_checked(mango_group_acc, program_id)?;

        // Check if SRM is part of the MangoGroup, if so throw err
        check!(mango_group.get_token_index(&srm_token::ID).is_none(), MangoErrorCode::FeeDiscountFunctionality)?;

        // if MangoSrmAccount is empty, initialize it
        check_eq_default!(mango_srm_account_acc.data_len(), size_of::<MangoSrmAccount>())?;
        let mut mango_srm_account = MangoSrmAccount::load_mut(mango_srm_account_acc)?;
        check_eq_default!(mango_srm_account_acc.owner, program_id)?;

        if mango_srm_account.account_flags == 0 {
            let rent = Rent::from_account_info(rent_acc)?;
            check_default!(rent.is_exempt(mango_srm_account_acc.lamports(), size_of::<MangoSrmAccount>()))?;

            mango_srm_account.account_flags = (AccountFlag::Initialized | AccountFlag::MangoSrmAccount).bits();
            mango_srm_account.mango_group = *mango_group_acc.key;
            check_default!(owner_acc.is_signer)?;  // this is not necessary but whatever
            mango_srm_account.owner = *owner_acc.key;
        } else {
            check_eq_default!(mango_srm_account.account_flags, (AccountFlag::Initialized | AccountFlag::MangoSrmAccount).bits())?;
            check_eq_default!(&mango_srm_account.mango_group, mango_group_acc.key)?;
        }

        let clock = Clock::from_account_info(clock_acc)?;
        mango_group.update_indexes(&clock)?;

        check_eq_default!(vault_acc.key, &mango_group.srm_vault)?;
        check_eq_default!(token_prog_acc.key, &spl_token::id())?;
        let deposit_instruction = spl_token::instruction::transfer(
            &spl_token::id(),
            srm_account_acc.key,
            vault_acc.key,
            &owner_acc.key, &[], quantity
        )?;
        let deposit_accs = [
            srm_account_acc.clone(),
            vault_acc.clone(),
            owner_acc.clone(),
            token_prog_acc.clone()
        ];

        solana_program::program::invoke_signed(&deposit_instruction, &deposit_accs, &[])?;
        mango_srm_account.amount = mango_srm_account.amount.checked_add(quantity).unwrap();
        Ok(())
    }

    #[inline(never)]
    fn withdraw_srm(
        program_id: &Pubkey,
        accounts: &[AccountInfo],
        quantity: u64
    ) -> MangoResult<()> {
        const NUM_FIXED: usize = 8;
        let accounts = array_ref![accounts, 0, NUM_FIXED];
        let [
            mango_group_acc,
            mango_srm_account_acc,
            owner_acc,
            srm_account_acc,
            vault_acc,
            signer_acc,
            token_prog_acc,
            clock_acc,
        ] = accounts;

        let mut mango_group = MangoGroup::load_mut_checked(mango_group_acc, program_id)?;

        // Check if SRM is part of the MangoGroup, if so throw err
        check!(mango_group.get_token_index(&srm_token::ID).is_none(), MangoErrorCode::FeeDiscountFunctionality)?;

        let mut mango_srm_account = MangoSrmAccount::load_mut_checked(
            program_id, mango_srm_account_acc, mango_group_acc.key)?;

        let clock = Clock::from_account_info(clock_acc)?;
        mango_group.update_indexes(&clock)?;
        check_default!(owner_acc.is_signer)?;
        check_eq_default!(&mango_srm_account.owner, owner_acc.key)?;
        check_eq_default!(vault_acc.key, &mango_group.srm_vault)?;
        check_default!(mango_srm_account.amount >= quantity)?;
        check_eq_default!(token_prog_acc.key, &spl_token::id())?;

        // Send out withdraw instruction to SPL token program
        let withdraw_instruction = spl_token::instruction::transfer(
            &spl_token::id(),
            vault_acc.key,
            srm_account_acc.key,
            signer_acc.key,
            &[],
            quantity
        )?;
        let withdraw_accs = [
            vault_acc.clone(),
            srm_account_acc.clone(),
            signer_acc.clone(),
            token_prog_acc.clone()
        ];
        let signer_seeds = gen_signer_seeds(&mango_group.signer_nonce, mango_group_acc.key);
        solana_program::program::invoke_signed(&withdraw_instruction, &withdraw_accs, &[&signer_seeds])?;
        mango_srm_account.amount = mango_srm_account.amount.checked_sub(quantity).unwrap();

        Ok(())
    }

    #[inline(never)]
    fn change_borrow_limit(
        program_id: &Pubkey,
        accounts: &[AccountInfo],
        token_index: usize,
        borrow_limit: u64
    ) -> MangoResult<()> {
        const NUM_FIXED: usize = 2;
        let accounts = array_ref![accounts, 0, NUM_FIXED];
        let [
            mango_group_acc,
            admin_acc,
        ] = accounts;

        let mut mango_group = MangoGroup::load_mut_checked(
            mango_group_acc,
            program_id
        )?;

        check_eq_default!(admin_acc.key, &mango_group.admin)?;
        check_default!(admin_acc.is_signer)?;

        mango_group.borrow_limits[token_index] = borrow_limit;
        Ok(())
    }

    #[inline(never)]
    fn place_order(
        program_id: &Pubkey,
        accounts: &[AccountInfo],
        order: serum_dex::instruction::NewOrderInstructionV3
    ) -> MangoResult<()> {
        const NUM_FIXED: usize = 17;
        let accounts = array_ref![accounts, 0, NUM_FIXED + 2 * NUM_MARKETS];
        let (
            fixed_accs,
            open_orders_accs,
            oracle_accs,
        ) = array_refs![accounts, NUM_FIXED, NUM_MARKETS, NUM_MARKETS];

        let [
            mango_group_acc,
            owner_acc,
            margin_account_acc,
            clock_acc,
            dex_prog_acc,
            spot_market_acc,
            dex_request_queue_acc,
            dex_event_queue_acc,
            bids_acc,
            asks_acc,
            vault_acc,
            signer_acc,
            dex_base_acc,
            dex_quote_acc,
            token_prog_acc,
            rent_acc,
            srm_vault_acc,
        ] = fixed_accs;

        let mut mango_group = MangoGroup::load_mut_checked(mango_group_acc, program_id)?;
        let mut margin_account = MarginAccount::load_mut_checked(
            program_id, margin_account_acc, mango_group_acc.key
        )?;

        let clock = Clock::from_account_info(clock_acc)?;
        mango_group.update_indexes(&clock)?;

        let prices = get_prices(&mango_group, oracle_accs)?;
        let coll_ratio = margin_account.get_collateral_ratio(&mango_group, &prices, open_orders_accs)?;
        if margin_account.being_liquidated {
            if coll_ratio >= mango_group.init_coll_ratio {
                margin_account.being_liquidated = false;
            } else {
                throw_err!(MangoErrorCode::BeingLiquidated)?;
            }
        }
        let reduce_only = coll_ratio < mango_group.init_coll_ratio;

        check_default!(owner_acc.is_signer)?;
        check_eq_default!(&margin_account.owner, owner_acc.key)?;

        let market_i = mango_group.get_market_index(spot_market_acc.key).unwrap();
        let token_i = match order.side {
            Side::Bid => NUM_MARKETS,
            Side::Ask => market_i
        };
        check_eq_default!(&mango_group.vaults[token_i], vault_acc.key)?;

        let pre_amount = {  // this is to keep track of how much funds were transferred out
            let vault = Account::unpack(&vault_acc.try_borrow_data()?)?;
            vault.amount
        };

        for i in 0..NUM_MARKETS {
            let open_orders_acc = &open_orders_accs[i];
            if i == market_i {  // this one must not be default pubkey
                check_default!(*open_orders_acc.key != Pubkey::default())?;
                if margin_account.open_orders[i] == Pubkey::default() {
                    let open_orders = load_open_orders(open_orders_acc)?;
                    check_eq_default!(open_orders.account_flags, 0)?;
                    margin_account.open_orders[i] = *open_orders_acc.key;
                }
            } else {
                check_eq_default!(open_orders_accs[i].key, &margin_account.open_orders[i])?;
                check_open_orders(&open_orders_accs[i], &mango_group.signer_key)?;
            }
        }

        check_eq_default!(token_prog_acc.key, &spl_token::id())?;
        check_eq_default!(dex_prog_acc.key, &mango_group.dex_program_id)?;
        let data = serum_dex::instruction::MarketInstruction::NewOrderV3(order).pack();
        let instruction = Instruction {
            program_id: *dex_prog_acc.key,
            data,
            accounts: vec![
                AccountMeta::new(*spot_market_acc.key, false),
                AccountMeta::new(*open_orders_accs[market_i].key, false),
                AccountMeta::new(*dex_request_queue_acc.key, false),
                AccountMeta::new(*dex_event_queue_acc.key, false),
                AccountMeta::new(*bids_acc.key, false),
                AccountMeta::new(*asks_acc.key, false),
                AccountMeta::new(*vault_acc.key, false),
                AccountMeta::new_readonly(*signer_acc.key, true),
                AccountMeta::new(*dex_base_acc.key, false),
                AccountMeta::new(*dex_quote_acc.key, false),
                AccountMeta::new_readonly(*token_prog_acc.key, false),
                AccountMeta::new_readonly(*rent_acc.key, false),
                AccountMeta::new(*srm_vault_acc.key, false),
            ],
        };
        let account_infos = [
            dex_prog_acc.clone(),  // Have to add account of the program id
            spot_market_acc.clone(),
            open_orders_accs[market_i].clone(),
            dex_request_queue_acc.clone(),
            dex_event_queue_acc.clone(),
            bids_acc.clone(),
            asks_acc.clone(),
            vault_acc.clone(),
            signer_acc.clone(),
            dex_base_acc.clone(),
            dex_quote_acc.clone(),
            token_prog_acc.clone(),
            rent_acc.clone(),
            srm_vault_acc.clone(),
        ];

        let signer_seeds = gen_signer_seeds(&mango_group.signer_nonce, mango_group_acc.key);
        solana_program::program::invoke_signed(&instruction, &account_infos, &[&signer_seeds])?;

        let post_amount = {
            let vault = Account::unpack(&vault_acc.try_borrow_data()?)?;
            vault.amount
        };

        let spent = pre_amount.checked_sub(post_amount).unwrap();
        let index: MangoIndex = mango_group.indexes[token_i];
        let native_deposit = margin_account.get_native_deposit(&index, token_i);

        // user deposits will be used first.
        // If user does not want that to happen, they must first issue a borrow command
        if native_deposit >= spent {
            let spent_deposit = U64F64::from_num(spent) / index.deposit;
            checked_sub_deposit(&mut mango_group, &mut margin_account, token_i, spent_deposit)?;
        } else {

            let avail_deposit = margin_account.deposits[token_i];
            checked_sub_deposit(&mut mango_group, &mut margin_account, token_i, avail_deposit)?;
            let rem_spend = U64F64::from_num(spent - native_deposit);

            check_default!(!reduce_only)?;  // Cannot borrow more in reduce only mode
            checked_add_borrow(&mut mango_group, &mut margin_account, token_i , rem_spend / index.borrow)?;
        }

        let coll_ratio = margin_account.get_collateral_ratio(&mango_group, &prices, open_orders_accs)?;
        check_default!(reduce_only || coll_ratio >= mango_group.init_coll_ratio)?;

        check_default!(mango_group.has_valid_deposits_borrows(token_i))?;
        Ok(())
    }

    #[inline(never)]
    fn settle_funds(
        program_id: &Pubkey,
        accounts: &[AccountInfo]
    ) -> MangoResult<()> {
        const NUM_FIXED: usize = 14;
        let accounts = array_ref![accounts, 0, NUM_FIXED];
        let [
            mango_group_acc,
            owner_acc,  // signer
            margin_account_acc,
            clock_acc,

            dex_prog_acc,
            spot_market_acc,
            open_orders_acc,
            signer_acc,
            dex_base_acc,
            dex_quote_acc,
            base_vault_acc,
            quote_vault_acc,
            dex_signer_acc,
            token_prog_acc,
        ] = accounts;

        let mut mango_group = MangoGroup::load_mut_checked(mango_group_acc, program_id)?;
        let mut margin_account = MarginAccount::load_mut_checked(
            program_id,
            margin_account_acc,
            mango_group_acc.key
        )?;
        let clock = Clock::from_account_info(clock_acc)?;
        mango_group.update_indexes(&clock)?;

        let market_i = mango_group.get_market_index(spot_market_acc.key).unwrap();

        check_default!(owner_acc.is_signer)?;
        check_eq_default!(owner_acc.key, &margin_account.owner)?;
        check_eq_default!(&margin_account.open_orders[market_i], open_orders_acc.key)?;
        check_eq_default!(base_vault_acc.key, &mango_group.vaults[market_i])?;
        check_eq_default!(quote_vault_acc.key, &mango_group.vaults[NUM_MARKETS])?;
        check_eq_default!(token_prog_acc.key, &spl_token::id())?;
        check_eq_default!(dex_prog_acc.key, &mango_group.dex_program_id)?;

        if *open_orders_acc.key == Pubkey::default() {
            return Ok(());
        }

        let (pre_base, pre_quote) = {
            let open_orders = load_open_orders(open_orders_acc)?;
            (open_orders.native_coin_free, open_orders.native_pc_free + open_orders.referrer_rebates_accrued)
        };

        if pre_base == 0 && pre_quote == 0 {
            return Ok(());
        }

        let signer_seeds = gen_signer_seeds(&mango_group.signer_nonce, mango_group_acc.key);
        invoke_settle_funds(
            dex_prog_acc,
            spot_market_acc,
            open_orders_acc,
            signer_acc,
            dex_base_acc,
            dex_quote_acc,
            base_vault_acc,
            quote_vault_acc,
            dex_signer_acc,
            token_prog_acc,
            &[&signer_seeds]
        )?;

        let (post_base, post_quote) = {
            let open_orders = load_open_orders(open_orders_acc)?;
            (open_orders.native_coin_free, open_orders.native_pc_free + open_orders.referrer_rebates_accrued)
        };

        check_default!(post_base <= pre_base)?;
        check_default!(post_quote <= pre_quote)?;

        let base_change = U64F64::from_num(pre_base - post_base) / mango_group.indexes[market_i].deposit;
        let quote_change = U64F64::from_num(pre_quote - post_quote) / mango_group.indexes[NUM_MARKETS].deposit;

        checked_add_deposit(&mut mango_group, &mut margin_account, market_i, base_change)?;
        checked_add_deposit(&mut mango_group, &mut margin_account, NUM_MARKETS, quote_change)?;
        Ok(())
    }

    #[inline(never)]
    fn cancel_order(
        program_id: &Pubkey,
        accounts: &[AccountInfo],
        data: Vec<u8>
    ) -> MangoResult<()> {
        const NUM_FIXED: usize = 11;
        let accounts = array_ref![accounts, 0, NUM_FIXED];

        let [
            mango_group_acc,
            owner_acc,  // signer
            margin_account_acc,
            clock_acc,
            dex_prog_acc,
            spot_market_acc,
            bids_acc,
            asks_acc,
            open_orders_acc,
            signer_acc,
            dex_event_queue_acc,
        ] = accounts;

        let mut mango_group = MangoGroup::load_mut_checked(mango_group_acc, program_id)?;
        let margin_account = MarginAccount::load_checked(
            program_id,
            margin_account_acc,
            mango_group_acc.key
        )?;
        let clock = Clock::from_account_info(clock_acc)?;
        mango_group.update_indexes(&clock)?;
        check_eq_default!(dex_prog_acc.key, &mango_group.dex_program_id)?;

        check_default!(owner_acc.is_signer)?;
        check_eq_default!(&margin_account.owner, owner_acc.key)?;
        let market_i = mango_group.get_market_index(spot_market_acc.key).unwrap();
        check_eq_default!(&margin_account.open_orders[market_i], open_orders_acc.key)?;

        let signer_seeds = gen_signer_seeds(&mango_group.signer_nonce, mango_group_acc.key);
        invoke_cancel_order(
            dex_prog_acc,
            spot_market_acc,
            bids_acc,
            asks_acc,
            open_orders_acc,
            signer_acc,
            dex_event_queue_acc,
            data,
            &[&signer_seeds]
        )?;
        Ok(())
    }

    #[inline(never)]
    fn place_and_settle(
        program_id: &Pubkey,
        accounts: &[AccountInfo],
        order: serum_dex::instruction::NewOrderInstructionV3
    ) -> MangoResult<()> {
        const NUM_FIXED: usize = 19;
        let accounts = array_ref![accounts, 0, NUM_FIXED + 2 * NUM_MARKETS];
        let (
            fixed_accs,
            open_orders_accs,
            oracle_accs,
        ) = array_refs![accounts, NUM_FIXED, NUM_MARKETS, NUM_MARKETS];

        let [
            mango_group_acc,
            owner_acc,
            margin_account_acc,
            clock_acc,
            dex_prog_acc,
            spot_market_acc,
            dex_request_queue_acc,
            dex_event_queue_acc,
            bids_acc,
            asks_acc,
            base_vault_acc,
            quote_vault_acc,
            signer_acc,
            dex_base_acc,
            dex_quote_acc,
            token_prog_acc,
            rent_acc,
            srm_vault_acc,
            dex_signer_acc
        ] = fixed_accs;

        let mut mango_group = MangoGroup::load_mut_checked(mango_group_acc, program_id)?;
        let mut margin_account = MarginAccount::load_mut_checked(
            program_id, margin_account_acc, mango_group_acc.key
        )?;

        let clock = Clock::from_account_info(clock_acc)?;
        mango_group.update_indexes(&clock)?;

        let prices = get_prices(&mango_group, oracle_accs)?;
        let coll_ratio = margin_account.get_collateral_ratio(&mango_group, &prices, open_orders_accs)?;

        if margin_account.being_liquidated {
            if coll_ratio >= mango_group.init_coll_ratio {
                margin_account.being_liquidated = false;
            } else {
                throw_err!(MangoErrorCode::BeingLiquidated)?;
            }
        }

        let reduce_only = coll_ratio < mango_group.init_coll_ratio;

        check_default!(owner_acc.is_signer)?;
        check_eq_default!(&margin_account.owner, owner_acc.key)?;

        let market_i = mango_group.get_market_index(spot_market_acc.key).unwrap();
        let side = order.side;
        let (in_token_i, out_token_i, vault_acc) = match side {
            Side::Bid => (market_i, NUM_MARKETS, quote_vault_acc),
            Side::Ask => (NUM_MARKETS, market_i, base_vault_acc)
        };
        check_eq_default!(&mango_group.vaults[market_i], base_vault_acc.key)?;
        check_eq_default!(&mango_group.vaults[NUM_MARKETS], quote_vault_acc.key)?;

        let (pre_base, pre_quote) = {
            (Account::unpack(&base_vault_acc.try_borrow_data()?)?.amount,
             Account::unpack(&quote_vault_acc.try_borrow_data()?)?.amount)
        };

        for i in 0..NUM_MARKETS {
            let open_orders_acc = &open_orders_accs[i];
            if i == market_i {  // this one must not be default pubkey
                check_default!(*open_orders_acc.key != Pubkey::default())?;

                // if this is first time using this open_orders_acc, check and save it
                if margin_account.open_orders[i] == Pubkey::default() {
                    let open_orders = load_open_orders(open_orders_acc)?;
                    check_eq_default!(open_orders.account_flags, 0)?;
                    margin_account.open_orders[i] = *open_orders_acc.key;
                } else {
                    check_eq_default!(open_orders_accs[i].key, &margin_account.open_orders[i])?;
                    check_open_orders(&open_orders_accs[i], &mango_group.signer_key)?;
                }
            } else {
                check_eq_default!(open_orders_accs[i].key, &margin_account.open_orders[i])?;
                check_open_orders(&open_orders_accs[i], &mango_group.signer_key)?;
            }
        }

        check_eq_default!(token_prog_acc.key, &spl_token::id())?;
        check_eq_default!(dex_prog_acc.key, &mango_group.dex_program_id)?;
        let data = serum_dex::instruction::MarketInstruction::NewOrderV3(order).pack();
        let instruction = Instruction {
            program_id: *dex_prog_acc.key,
            data,
            accounts: vec![
                AccountMeta::new(*spot_market_acc.key, false),
                AccountMeta::new(*open_orders_accs[market_i].key, false),
                AccountMeta::new(*dex_request_queue_acc.key, false),
                AccountMeta::new(*dex_event_queue_acc.key, false),
                AccountMeta::new(*bids_acc.key, false),
                AccountMeta::new(*asks_acc.key, false),
                AccountMeta::new(*vault_acc.key, false),
                AccountMeta::new_readonly(*signer_acc.key, true),
                AccountMeta::new(*dex_base_acc.key, false),
                AccountMeta::new(*dex_quote_acc.key, false),
                AccountMeta::new_readonly(*token_prog_acc.key, false),
                AccountMeta::new_readonly(*rent_acc.key, false),
                AccountMeta::new(*srm_vault_acc.key, false),
            ],
        };
        let account_infos = [
            dex_prog_acc.clone(),  // Have to add account of the program id
            spot_market_acc.clone(),
            open_orders_accs[market_i].clone(),
            dex_request_queue_acc.clone(),
            dex_event_queue_acc.clone(),
            bids_acc.clone(),
            asks_acc.clone(),
            vault_acc.clone(),
            signer_acc.clone(),
            dex_base_acc.clone(),
            dex_quote_acc.clone(),
            token_prog_acc.clone(),
            rent_acc.clone(),
            srm_vault_acc.clone(),
        ];

        let signer_seeds = gen_signer_seeds(&mango_group.signer_nonce, mango_group_acc.key);
        solana_program::program::invoke_signed(&instruction, &account_infos, &[&signer_seeds])?;

        // Settle funds for this market
        invoke_settle_funds(
            dex_prog_acc,
            spot_market_acc,
            &open_orders_accs[market_i],
            signer_acc,
            dex_base_acc,
            dex_quote_acc,
            base_vault_acc,
            quote_vault_acc,
            dex_signer_acc,
            token_prog_acc,
            &[&signer_seeds]
        )?;

        let (post_base, post_quote) = {
            (Account::unpack(&base_vault_acc.try_borrow_data()?)?.amount,
             Account::unpack(&quote_vault_acc.try_borrow_data()?)?.amount)
        };

        let (pre_in, pre_out, post_in, post_out) = match side {
            Side::Bid => (pre_base, pre_quote, post_base, post_quote),
            Side::Ask => (pre_quote, pre_base, post_quote, post_base)
        };

        // It's possible the net change was positive for both tokens
        // It's not possible for in_token to be negative
        let out_index: MangoIndex = mango_group.indexes[out_token_i];
        let in_index: MangoIndex = mango_group.indexes[in_token_i];

        // if out token was net negative, then you may need to borrow more
        if post_out < pre_out {
            let total_out = pre_out.checked_sub(post_out).unwrap();
            let native_deposit = margin_account.get_native_deposit(&out_index, out_token_i);
            if native_deposit < total_out {  // need to borrow
                let avail_deposit = margin_account.deposits[out_token_i];
                checked_sub_deposit(&mut mango_group, &mut margin_account, out_token_i, avail_deposit)?;
                let rem_spend = U64F64::from_num(total_out - native_deposit);

                check_default!(!reduce_only)?;  // Cannot borrow more in reduce only mode
                checked_add_borrow(&mut mango_group, &mut margin_account, out_token_i, rem_spend / out_index.borrow)?;
            } else {  // just spend user deposits
                let mango_spent = U64F64::from_num(total_out) / out_index.deposit;
                checked_sub_deposit(&mut mango_group, &mut margin_account, out_token_i, mango_spent)?;
            }
        } else {  // Add out token deposit
            let deposit = U64F64::from_num(post_out.checked_sub(pre_out).unwrap()) / out_index.deposit;
            checked_add_deposit(&mut mango_group, &mut margin_account, out_token_i, deposit)?;
        }

        let total_in = U64F64::from_num(post_in.checked_sub(pre_in).unwrap()) / in_index.deposit;
        checked_add_deposit(&mut mango_group, &mut margin_account, in_token_i, total_in)?;

        // Settle borrow
        // TODO only do ops on tokens that have borrows and deposits
        settle_borrow_full_unchecked(&mut mango_group, &mut margin_account, out_token_i)?;
        settle_borrow_full_unchecked(&mut mango_group, &mut margin_account, in_token_i)?;

        let coll_ratio = margin_account.get_collateral_ratio(&mango_group, &prices, open_orders_accs)?;
        check!(reduce_only || coll_ratio >= mango_group.init_coll_ratio, MangoErrorCode::CollateralRatioLimit)?;
        check_default!(mango_group.has_valid_deposits_borrows(out_token_i))?;

        Ok(())
    }

    /// Liquidator is allowed to cancel orders of an account that is being_liquidated
    /// This will also settle funds
    #[inline(never)]
    fn force_cancel_orders(
        program_id: &Pubkey,
        accounts: &[AccountInfo],
        limit: u8
    ) -> MangoResult<()> {
        const NUM_FIXED: usize = 16;
        let accounts = array_ref![accounts, 0, NUM_FIXED + 2 * NUM_MARKETS];
        let (
            fixed_accs,
            open_orders_accs,
            oracle_accs,
        ) = array_refs![accounts, NUM_FIXED, NUM_MARKETS, NUM_MARKETS];

        let [
            mango_group_acc,
            liqor_acc,
            liqee_margin_account_acc,
            base_vault_acc,
            quote_vault_acc,
            spot_market_acc,
            bids_acc,
            asks_acc,
            signer_acc,
            dex_event_queue_acc,
            dex_base_acc,
            dex_quote_acc,
            dex_signer_acc,
            token_prog_acc,
            dex_prog_acc,
            clock_acc
        ] = fixed_accs;

        check_eq!(token_prog_acc.key, &spl_token::id(), MangoErrorCode::InvalidProgramId)?;
        check!(liqor_acc.is_signer, MangoErrorCode::SignerNecessary)?;
        let mut mango_group = MangoGroup::load_mut_checked(
            mango_group_acc, program_id
        )?;
        check_eq!(dex_prog_acc.key, &mango_group.dex_program_id, MangoErrorCode::InvalidProgramId)?;
        check_eq!(signer_acc.key, &mango_group.signer_key, MangoErrorCode::InvalidSignerKey)?;

        let market_i = mango_group.get_market_index(spot_market_acc.key).unwrap();
        check_eq!(&mango_group.vaults[market_i], base_vault_acc.key, MangoErrorCode::InvalidMangoVault)?;
        check_eq!(&mango_group.vaults[NUM_MARKETS], quote_vault_acc.key, MangoErrorCode::InvalidMangoVault)?;
        check_eq_default!(spot_market_acc.key, &mango_group.spot_markets[market_i])?;

        let mut liqee_margin_account = MarginAccount::load_mut_checked(
            program_id, liqee_margin_account_acc, mango_group_acc.key
        )?;

        for i in 0..NUM_MARKETS {
            check_eq!(open_orders_accs[i].key, &liqee_margin_account.open_orders[i],
                MangoErrorCode::InvalidOpenOrdersAccount)?;
            check_open_orders(&open_orders_accs[i], &mango_group.signer_key)?;
        }

        let clock = Clock::from_account_info(clock_acc)?;
        mango_group.update_indexes(&clock)?;
        let prices = get_prices(&mango_group, oracle_accs)?;
        let coll_ratio = liqee_margin_account.get_collateral_ratio(
            &mango_group, &prices, open_orders_accs)?;

        // Only allow liquidations on accounts already being liquidated and below init or accounts below maint
        if liqee_margin_account.being_liquidated {
            if coll_ratio >= mango_group.init_coll_ratio {
                liqee_margin_account.being_liquidated = false;
                return Ok(());
            }
        } else if coll_ratio < mango_group.maint_coll_ratio {
            liqee_margin_account.being_liquidated = true;
        } else {
            throw_err!(MangoErrorCode::NotLiquidatable)?;
        }
        let open_orders_acc = &open_orders_accs[market_i];
        let signers_seeds = gen_signer_seeds(&mango_group.signer_nonce, mango_group_acc.key);

        invoke_cancel_orders(open_orders_acc, dex_prog_acc, spot_market_acc, bids_acc, asks_acc, signer_acc,
                             dex_event_queue_acc, &[&signers_seeds], limit)?;

        let (pre_base, pre_quote) = {
            let open_orders = load_open_orders(open_orders_acc)?;
            (open_orders.native_coin_free, open_orders.native_pc_free + open_orders.referrer_rebates_accrued)
        };

        if pre_base == 0 && pre_quote == 0 {
            return Ok(());
        }

        invoke_settle_funds(dex_prog_acc, spot_market_acc, open_orders_acc, signer_acc, dex_base_acc,
                            dex_quote_acc, base_vault_acc, quote_vault_acc, dex_signer_acc,
                            token_prog_acc, &[&signers_seeds])?;

        let (post_base, post_quote) = {
            let open_orders = load_open_orders(open_orders_acc)?;
            (open_orders.native_coin_free, open_orders.native_pc_free + open_orders.referrer_rebates_accrued)
        };

        check_default!(post_base <= pre_base)?;
        check_default!(post_quote <= pre_quote)?;

        let base_change = U64F64::from_num(pre_base - post_base) / mango_group.indexes[market_i].deposit;
        let quote_change = U64F64::from_num(pre_quote - post_quote) / mango_group.indexes[NUM_MARKETS].deposit;

        checked_add_deposit(&mut mango_group, &mut liqee_margin_account, market_i, base_change)?;
        checked_add_deposit(&mut mango_group, &mut liqee_margin_account, NUM_MARKETS, quote_change)?;

        Ok(())
    }
    #[inline(never)]
    fn partial_liquidate(
        program_id: &Pubkey,
        accounts: &[AccountInfo],
        max_deposit: u64
    ) -> MangoResult<()> {

        const NUM_FIXED: usize = 10;
        // TODO make it so canceling orders feature is optional if no orders outstanding to cancel
        let accounts = array_ref![accounts, 0, NUM_FIXED + 2 * NUM_MARKETS];
        let (
            fixed_accs,
            open_orders_accs,
            oracle_accs,
        ) = array_refs![accounts, NUM_FIXED, NUM_MARKETS, NUM_MARKETS];

        let [
            mango_group_acc,
            liqor_acc,
            liqor_in_token_acc,
            liqor_out_token_acc,
            liqee_margin_account_acc,
            in_vault_acc,
            out_vault_acc,
            signer_acc,
            token_prog_acc,
            _clock_acc,
        ] = fixed_accs;
        check!(token_prog_acc.key == &spl_token::ID, MangoErrorCode::InvalidProgramId)?;
        check!(liqor_acc.is_signer, MangoErrorCode::SignerNecessary)?;
        let mut mango_group = MangoGroup::load_mut_checked(
            mango_group_acc, program_id
        )?;
        check_eq!(signer_acc.key, &mango_group.signer_key, MangoErrorCode::InvalidSignerKey)?;

        let liqor_in_token_account = Account::unpack(&liqor_in_token_acc.try_borrow_data()?)?;
        let in_token_index = mango_group.get_token_index(&liqor_in_token_account.mint).unwrap();
        let liqor_out_token_account = Account::unpack(&liqor_out_token_acc.try_borrow_data()?)?;
        let out_token_index = mango_group.get_token_index(&liqor_out_token_account.mint).unwrap();
        check_default!(in_token_index != out_token_index)?;

        check_eq!(&mango_group.vaults[in_token_index], in_vault_acc.key, MangoErrorCode::InvalidMangoVault)?;
        check_eq!(&mango_group.vaults[out_token_index], out_vault_acc.key, MangoErrorCode::InvalidMangoVault)?;

        let mut liqee_margin_account = MarginAccount::load_mut_checked(
            program_id, liqee_margin_account_acc, mango_group_acc.key
        )?;

        for i in 0..NUM_MARKETS {
            check_eq!(open_orders_accs[i].key, &liqee_margin_account.open_orders[i],
                MangoErrorCode::InvalidOpenOrdersAccount)?;
            check_open_orders(&open_orders_accs[i], &mango_group.signer_key)?;
        }

        // TODO - add a check to make sure indexes were updated in last hour
        //      if not updated, then update indexes and return without continuing
        //      there is not enough compute to continue
        //      code is written below but needs to be tested on devnet first

        // let clock = Clock::from_account_info(clock_acc)?;
        // let now_ts = clock.unix_timestamp as u64;
        // for i in 0..NUM_TOKENS {
        //     if now_ts > mango_group.indexes[i].last_update + 3600 {
        //         msg!("Invalid indexes");
        //         mango_group.update_indexes(&clock)?;
        //         return Ok(());
        //     }
        // }

        let prices = get_prices(&mango_group, oracle_accs)?;
        let start_assets = liqee_margin_account.get_assets(&mango_group, open_orders_accs)?;
        let start_liabs = liqee_margin_account.get_liabs(&mango_group)?;
        let coll_ratio = liqee_margin_account.coll_ratio_from_assets_liabs(
            &prices, &start_assets, &start_liabs)?;

        // Only allow liquidations on accounts already being liquidated and below init or accounts below maint
        if liqee_margin_account.being_liquidated {
            if coll_ratio >= mango_group.init_coll_ratio {
                liqee_margin_account.being_liquidated = false;
                return Ok(());
            }
        } else if coll_ratio >= mango_group.maint_coll_ratio {
            throw_err!(MangoErrorCode::NotLiquidatable)?;
        }

        // Settle borrows to increase coll ratio if possible
        for i in 0..NUM_TOKENS {
            settle_borrow_full_unchecked(&mut mango_group, &mut liqee_margin_account, i)?;
        }

        // Check again to see if account still liquidatable
        let coll_ratio = liqee_margin_account.get_collateral_ratio(
            &mango_group, &prices, open_orders_accs)?;

        if liqee_margin_account.being_liquidated {
            if coll_ratio >= mango_group.init_coll_ratio {
                // TODO make sure liquidator knows why tx was success but he didn't receive any funds
                msg!("Account above init_coll_ratio after settling borrows");
                liqee_margin_account.being_liquidated = false;
                return Ok(());
            }
        } else if coll_ratio >= mango_group.maint_coll_ratio {
            msg!("Account above maint_coll_ratio after settling borrows");
            return Ok(());
        } else {
            liqee_margin_account.being_liquidated = true;
        }

        // Get how much to deposit and how much to withdraw
        let (in_quantity, out_quantity) = get_in_out_quantities(
            &mut mango_group, &mut liqee_margin_account, open_orders_accs, &prices, in_token_index,
            out_token_index, max_deposit
        )?;
        let signer_nonce = mango_group.signer_nonce;
        let signers_seeds = gen_signer_seeds(&signer_nonce, mango_group_acc.key);
        invoke_transfer(token_prog_acc, liqor_in_token_acc, in_vault_acc, liqor_acc,
                        &[&signers_seeds], in_quantity)?;
        invoke_transfer(token_prog_acc, out_vault_acc, liqor_out_token_acc, signer_acc,
                        &[&signers_seeds], out_quantity)?;

        // Check if account valid now
        let end_assets = liqee_margin_account.get_assets(&mango_group, open_orders_accs)?;
        let end_liabs = liqee_margin_account.get_liabs(&mango_group)?;
        let coll_ratio = liqee_margin_account.coll_ratio_from_assets_liabs(
            &prices, &end_assets, &end_liabs)?;
        let mut total_deposits = [ZERO_U64F64; NUM_TOKENS];

        let mut socialized_losses = false;
        if coll_ratio >= mango_group.init_coll_ratio {
            // set margin account to no longer being liquidated
            liqee_margin_account.being_liquidated = false;
        } else {
            // if all asset vals is dust (less than 1 cent?) socialize loss on lenders
            let assets_val = liqee_margin_account.get_assets_val(&mango_group, &prices, open_orders_accs)?;

            if assets_val < DUST_THRESHOLD {
                for i in 0..NUM_TOKENS {
                    let native_borrow: U64F64 = end_liabs[i];
                    let total_deposits_native: U64F64 = mango_group.total_deposits[i] * mango_group.indexes[i].deposit;

                    total_deposits[i] = total_deposits_native;

                    if native_borrow > 0 {
                        socialized_losses = true;
                        socialize_loss(
                            &mut mango_group,
                            &mut liqee_margin_account,
                            i,
                            native_borrow,
                            total_deposits_native
                        )?;
                    }
                }
            }
        }

        // Note total_deposits is only logged with reasonable values if assets_val < DUST_THRESHOLD
        log_liquidation_details(&start_assets, &start_liabs, &end_assets, &end_liabs, &prices, socialized_losses, &total_deposits);
        // TODO do I need to check total deposits and total borrows?
        // TODO log deposit indexes before and after liquidation as a way to measure socialize of losses
        Ok(())
    }

    #[inline(never)]
    fn add_margin_account_info(
        program_id: &Pubkey,
        accounts: &[AccountInfo],
        info: [u8; INFO_LEN]
    ) -> MangoResult<()> {
        const NUM_FIXED: usize = 3;
        let accounts = array_ref![accounts, 0, NUM_FIXED];
        let [
            mango_group_acc,
            margin_account_acc,
            owner_acc,
        ] = accounts;

        let mut margin_account = MarginAccount::load_mut_checked(
            program_id, margin_account_acc, mango_group_acc.key)?;
        check_eq!(owner_acc.key, &margin_account.owner, MangoErrorCode::InvalidMarginAccountOwner)?;
        check!(owner_acc.is_signer, MangoErrorCode::SignerNecessary)?;
        margin_account.info = info;
        Ok(())
    }
    pub fn process(
        program_id: &Pubkey,
        accounts: &[AccountInfo],
        data: &[u8]
    ) -> MangoResult<()> {
        let instruction = MangoInstruction::unpack(data).ok_or(ProgramError::InvalidInstructionData)?;
        match instruction {
            MangoInstruction::InitMangoGroup {
                signer_nonce, maint_coll_ratio, init_coll_ratio, borrow_limits
            } => {
                msg!("Mango: InitMangoGroup");
                Self::init_mango_group(program_id, accounts, signer_nonce, maint_coll_ratio, init_coll_ratio, borrow_limits)?;
            }
            MangoInstruction::InitMarginAccount => {
                msg!("Mango: InitMarginAccount");
                Self::init_margin_account(program_id, accounts)?;
            }
            MangoInstruction::Deposit {
                quantity
            } => {
                msg!("Mango: Deposit");
                Self::deposit(program_id, accounts, quantity)?;
            }
            MangoInstruction::Withdraw {
                quantity
            } => {
                msg!("Mango: Withdraw");
                Self::withdraw(program_id, accounts, quantity)?;
            }
            MangoInstruction::Borrow {
                token_index,
                quantity
            } => {
                msg!("Mango: Borrow");
                Self::borrow(program_id, accounts, token_index, quantity)?;
            }
            MangoInstruction::SettleBorrow {
                token_index,
                quantity
            } => {
                msg!("Mango: SettleBorrow");
                Self::settle_borrow(program_id, accounts, token_index, quantity)?;
            }
            MangoInstruction::Liquidate {
                deposit_quantities
            } => {
                // Either user takes the position
                // Or the program can liquidate on the serum dex (in case no liquidator wants to take pos)
                msg!("Mango: Liquidate");
                Self::liquidate(program_id, accounts, deposit_quantities)?;
            }
            MangoInstruction::DepositSrm {
                quantity
            } => {
                msg!("Mango: DepositSrm");
                Self::deposit_srm(program_id, accounts, quantity)?;
            }
            MangoInstruction::WithdrawSrm {
                quantity
            } => {
                msg!("Mango: WithdrawSrm");
                Self::withdraw_srm(program_id, accounts, quantity)?;
            }
            MangoInstruction::PlaceOrder {
                order
            } => {
                msg!("Mango: PlaceOrder");
                Self::place_order(program_id, accounts, order)?;
            }
            MangoInstruction::SettleFunds => {
                msg!("Mango: SettleFunds");
                Self::settle_funds(program_id, accounts)?;
            }
            MangoInstruction::CancelOrder {
                order
            } => {
                msg!("Mango: CancelOrder");
                let data =  serum_dex::instruction::MarketInstruction::CancelOrderV2(order).pack();
                Self::cancel_order(program_id, accounts, data)?;
            }
            MangoInstruction::CancelOrderByClientId {
                client_id
            } => {
                msg!("Mango: CancelOrderByClientId");
                Self::cancel_order(program_id, accounts, client_id.to_le_bytes().to_vec())?;
            }

            MangoInstruction::ChangeBorrowLimit {
                token_index, borrow_limit
            } => {
                msg!("Mango: ChangeBorrowLimit");
                Self::change_borrow_limit(program_id, accounts, token_index, borrow_limit)?;
            }
            MangoInstruction::PlaceAndSettle {
                order
            } => {
                msg!("Mango: PlaceAndSettle");
                Self::place_and_settle(program_id, accounts, order)?;
            }
            MangoInstruction::ForceCancelOrders {
                limit
            } => {
                msg!("Mango: ForceCancelOrders");
                Self::force_cancel_orders(program_id, accounts, limit)?;
            }
            MangoInstruction::PartialLiquidate {
                max_deposit
            } => {
                msg!("Mango: PartialLiquidate");
                Self::partial_liquidate(program_id, accounts, max_deposit)?;
            }
            MangoInstruction::AddMarginAccountInfo {
                info
            } => {
                msg!("Mango: AddMarginAccountInfo");
                Self::add_margin_account_info(program_id, accounts, info)?;
            }
        }
        Ok(())
    }
}

fn log_liquidation_details(
    start_assets: &[U64F64; NUM_TOKENS],
    start_liabs: &[U64F64; NUM_TOKENS],
    end_assets: &[U64F64; NUM_TOKENS],
    end_liabs: &[U64F64; NUM_TOKENS],
    prices: &[U64F64; NUM_TOKENS],
    socialized_losses: bool,
    total_deposits: &[U64F64; NUM_TOKENS]
) {
    let mut prices_f64 = [0_f64; NUM_TOKENS];
    let mut start_assets_u64 = [0u64; NUM_TOKENS];
    let mut start_liabs_u64 = [0u64; NUM_TOKENS];
    let mut end_assets_u64 = [0u64; NUM_TOKENS];
    let mut end_liabs_u64 = [0u64; NUM_TOKENS];
    let mut total_deposits_u64 = [0u64; NUM_TOKENS];
    for i in 0..NUM_TOKENS {
        prices_f64[i] = prices[i].to_num::<f64>();
        start_assets_u64[i] = start_assets[i].to_num();
        start_liabs_u64[i] = start_liabs[i].to_num();
        end_assets_u64[i] = end_assets[i].to_num();
        end_liabs_u64[i] = end_liabs[i].to_num();
        total_deposits_u64[i] = total_deposits[i].to_num();
    }

    msg!("liquidation details: {{ \
                \"start\": {{ \"assets\": {:?}, \"liabs\": {:?} }}, \
                \"end\": {{ \"assets\": {:?}, \"liabs\": {:?} }}, \
                \"prices\": {:?}, \
                \"socialized_losses\": {}, \
                \"total_deposits\": {:?} \
            }}", start_assets_u64, start_liabs_u64, end_assets_u64, end_liabs_u64, prices_f64, socialized_losses, total_deposits_u64);
}

fn settle_borrow_unchecked(
    mango_group: &mut MangoGroup,
    margin_account: &mut MarginAccount,
    token_index: usize,
    quantity: u64
) -> MangoResult<()> {
    let deposit_index = mango_group.indexes[token_index].deposit;
    let borrow_index = mango_group.indexes[token_index].borrow;
    let native_borrow: U64F64 = margin_account.borrows[token_index] * borrow_index;
    let native_deposit: U64F64 = margin_account.deposits[token_index] * deposit_index;
    let quantity = U64F64::from_num(quantity);

    let quantity = min(quantity, native_deposit);
    if quantity >= native_borrow {  // Reduce borrows to 0 to prevent rounding related dust
        // NOTE: native_borrow / index.borrow is same as margin_account.borrows[token_index]
        checked_sub_deposit(mango_group, margin_account, token_index, native_borrow / deposit_index)?;
        checked_sub_borrow(mango_group, margin_account, token_index, margin_account.borrows[token_index])?;
    } else {
        checked_sub_deposit(mango_group, margin_account, token_index, quantity / deposit_index)?;
        checked_sub_borrow(mango_group, margin_account, token_index, quantity / borrow_index)?;
    }

    // No need to check collateralization ratio or deposits/borrows validity
    Ok(())

}

fn settle_borrow_full_unchecked(
    mango_group: &mut MangoGroup,
    margin_account: &mut MarginAccount,
    token_index: usize,
) -> MangoResult<()> {
    let index: &MangoIndex = &mango_group.indexes[token_index];

    let native_borrow = margin_account.get_native_borrow(index, token_index);
    let native_deposit = margin_account.get_native_deposit(index, token_index);

    let quantity = cmp::min(native_borrow, native_deposit);

    let borr_settle = U64F64::from_num(quantity) / index.borrow;
    let dep_settle = U64F64::from_num(quantity) / index.deposit;

    checked_sub_deposit(mango_group, margin_account, token_index, dep_settle)?;
    checked_sub_borrow(mango_group, margin_account, token_index, borr_settle)?;

    // No need to check collateralization ratio or deposits/borrows validity

    Ok(())

}

fn socialize_loss(
    mango_group: &mut MangoGroup,
    margin_account: &mut MarginAccount,
    token_index: usize,
    reduce_quantity_native: U64F64,
    total_deposits_native: U64F64
) -> MangoResult<()> {

    // reduce borrow for this margin_account by appropriate amount
    // decrease MangoIndex.deposit by appropriate amount

    // TODO make sure there is enough funds to socialize losses
    let quantity: U64F64 = reduce_quantity_native / mango_group.indexes[token_index].borrow;
    checked_sub_borrow(mango_group, margin_account, token_index, quantity)?;

    let percentage_loss = reduce_quantity_native.checked_div(total_deposits_native).unwrap();
    let index: &mut MangoIndex = &mut mango_group.indexes[token_index];
    index.deposit = index.deposit
        .checked_sub(percentage_loss.checked_mul(index.deposit).unwrap()).unwrap();

    Ok(())
}

fn checked_sub_deposit(
    mango_group: &mut MangoGroup,
    margin_account: &mut MarginAccount,
    token_index: usize,
    quantity: U64F64
) -> MangoResult<()> {
    margin_account.checked_sub_deposit(token_index, quantity)?;
    mango_group.checked_sub_deposit(token_index, quantity)
}

fn checked_sub_borrow(
    mango_group: &mut MangoGroup,
    margin_account: &mut MarginAccount,
    token_index: usize,
    quantity: U64F64
) -> MangoResult<()> {
    margin_account.checked_sub_borrow(token_index, quantity)?;
    mango_group.checked_sub_borrow(token_index, quantity)?;

    let mut has_borrows = false;
    for i in 0..NUM_TOKENS {
        if margin_account.borrows[i] > 0 {
            has_borrows = true;
        }
    }
    margin_account.has_borrows = has_borrows;

    Ok(())
}

fn checked_add_deposit(
    mango_group: &mut MangoGroup,
    margin_account: &mut MarginAccount,
    token_index: usize,
    quantity: U64F64
) -> MangoResult<()> {
    margin_account.checked_add_deposit(token_index, quantity)?;
    mango_group.checked_add_deposit(token_index, quantity)
}

fn checked_add_borrow(
    mango_group: &mut MangoGroup,
    margin_account: &mut MarginAccount,
    token_index: usize,
    quantity: U64F64
) -> MangoResult<()> {
    margin_account.checked_add_borrow(token_index, quantity)?;
    mango_group.checked_add_borrow(token_index, quantity)?;

    if !margin_account.has_borrows && quantity > 0 {
        margin_account.has_borrows = true;
    }

    Ok(())
}

pub fn get_prices(
    mango_group: &MangoGroup,
    oracle_accs: &[AccountInfo]
) -> MangoResult<[U64F64; NUM_TOKENS]> {
    let mut prices = [ZERO_U64F64; NUM_TOKENS];
    prices[NUM_MARKETS] = ONE_U64F64;  // quote currency is 1
    let quote_decimals: u8 = mango_group.mint_decimals[NUM_MARKETS];

    for i in 0..NUM_MARKETS {
        check_eq_default!(&mango_group.oracles[i], oracle_accs[i].key)?;

        // TODO store this info in MangoGroup, first make sure it cannot be changed by solink
        let quote_adj = U64F64::from_num(
            10u64.pow(quote_decimals.checked_sub(mango_group.oracle_decimals[i]).unwrap() as u32)
        );

        let answer = flux_aggregator::read_median(&oracle_accs[i])?; // this is in USD cents

        let value = U64F64::from_num(answer.median);

        let base_adj = U64F64::from_num(10u64.pow(mango_group.mint_decimals[i] as u32));
        prices[i] = quote_adj
            .checked_div(base_adj).unwrap()
            .checked_mul(value).unwrap();
    }
    Ok(prices)
}

fn invoke_settle_funds<'a>(
    dex_prog_acc: &AccountInfo<'a>,
    spot_market_acc: &AccountInfo<'a>,
    open_orders_acc: &AccountInfo<'a>,
    signer_acc: &AccountInfo<'a>,
    dex_base_acc: &AccountInfo<'a>,
    dex_quote_acc: &AccountInfo<'a>,
    base_vault_acc: &AccountInfo<'a>,
    quote_vault_acc: &AccountInfo<'a>,
    dex_signer_acc: &AccountInfo<'a>,
    token_prog_acc: &AccountInfo<'a>,
    signers_seeds: &[&[&[u8]]]
) -> ProgramResult {
    let data = serum_dex::instruction::MarketInstruction::SettleFunds.pack();
    let instruction = Instruction {
        program_id: *dex_prog_acc.key,
        data,
        accounts: vec![
            AccountMeta::new(*spot_market_acc.key, false),
            AccountMeta::new(*open_orders_acc.key, false),
            AccountMeta::new_readonly(*signer_acc.key, true),
            AccountMeta::new(*dex_base_acc.key, false),
            AccountMeta::new(*dex_quote_acc.key, false),
            AccountMeta::new(*base_vault_acc.key, false),
            AccountMeta::new(*quote_vault_acc.key, false),
            AccountMeta::new_readonly(*dex_signer_acc.key, false),
            AccountMeta::new_readonly(*token_prog_acc.key, false),
            AccountMeta::new(*quote_vault_acc.key, false),
        ],
    };

    let account_infos = [
        dex_prog_acc.clone(),
        spot_market_acc.clone(),
        open_orders_acc.clone(),
        signer_acc.clone(),
        dex_base_acc.clone(),
        dex_quote_acc.clone(),
        base_vault_acc.clone(),
        quote_vault_acc.clone(),
        dex_signer_acc.clone(),
        token_prog_acc.clone(),
        quote_vault_acc.clone(),
    ];
    solana_program::program::invoke_signed(&instruction, &account_infos, signers_seeds)
}

fn invoke_cancel_order<'a>(
    dex_prog_acc: &AccountInfo<'a>,
    spot_market_acc: &AccountInfo<'a>,
    bids_acc: &AccountInfo<'a>,
    asks_acc: &AccountInfo<'a>,
    open_orders_acc: &AccountInfo<'a>,
    signer_acc: &AccountInfo<'a>,
    dex_event_queue_acc: &AccountInfo<'a>,
    data: Vec<u8>,
    signers_seeds: &[&[&[u8]]]
) -> ProgramResult {
    let instruction = Instruction {
        program_id: *dex_prog_acc.key,
        data,
        accounts: vec![
            AccountMeta::new(*spot_market_acc.key, false),
            AccountMeta::new(*bids_acc.key, false),
            AccountMeta::new(*asks_acc.key, false),
            AccountMeta::new(*open_orders_acc.key, false),
            AccountMeta::new_readonly(*signer_acc.key, true),
            AccountMeta::new(*dex_event_queue_acc.key, false),

        ],
    };

    let account_infos = [
        dex_prog_acc.clone(),
        spot_market_acc.clone(),
        bids_acc.clone(),
        asks_acc.clone(),
        open_orders_acc.clone(),
        signer_acc.clone(),
        dex_event_queue_acc.clone()
    ];
    solana_program::program::invoke_signed(&instruction, &account_infos, signers_seeds)
}

fn invoke_cancel_orders<'a>(
    open_orders_acc: &AccountInfo<'a>,
    dex_prog_acc: &AccountInfo<'a>,
    spot_market_acc: &AccountInfo<'a>,
    bids_acc: &AccountInfo<'a>,
    asks_acc: &AccountInfo<'a>,
    signer_acc: &AccountInfo<'a>,
    dex_event_queue_acc: &AccountInfo<'a>,
    signers_seeds: &[&[&[u8]]],

    mut limit: u8
) -> MangoResult<()> {
    let mut cancels = vec![];
    {
        let open_orders = load_open_orders(open_orders_acc)?;

        let market = load_market_state(spot_market_acc, dex_prog_acc.key)?;
        let bids = load_bids_mut(&market, bids_acc)?;
        let asks = load_asks_mut(&market, asks_acc)?;

        limit = min(limit, open_orders.free_slot_bits.count_zeros() as u8);
        if limit == 0 {
            return Ok(());
        }
        for j in 0..128 {
            let slot_mask = 1u128 << j;
            if open_orders.free_slot_bits & slot_mask != 0 {  // means slot is free
                continue;
            }
            let order_id = open_orders.orders[j];

            let side = if open_orders.is_bid_bits & slot_mask != 0 {
                match bids.find_by_key(order_id) {
                    None => { continue }
                    Some(_) => serum_dex::matching::Side::Bid
                }
            } else {
                match asks.find_by_key(order_id) {
                    None => { continue }
                    Some(_) => serum_dex::matching::Side::Ask
                }
            };

            let cancel_instruction = serum_dex::instruction::CancelOrderInstructionV2 { side, order_id };

            cancels.push(cancel_instruction);

            limit -= 1;
            if limit == 0 {
                break;
            }
        }
    }

    let mut instruction = Instruction {
        program_id: *dex_prog_acc.key,
        data: vec![],
        accounts: vec![
            AccountMeta::new(*spot_market_acc.key, false),
            AccountMeta::new(*bids_acc.key, false),
            AccountMeta::new(*asks_acc.key, false),
            AccountMeta::new(*open_orders_acc.key, false),
            AccountMeta::new_readonly(*signer_acc.key, true),
            AccountMeta::new(*dex_event_queue_acc.key, false),
        ],
    };

    let account_infos = [
        dex_prog_acc.clone(),
        spot_market_acc.clone(),
        bids_acc.clone(),
        asks_acc.clone(),
        open_orders_acc.clone(),
        signer_acc.clone(),
        dex_event_queue_acc.clone()
    ];

    for cancel in cancels.iter() {
        let cancel_instruction = serum_dex::instruction::MarketInstruction::CancelOrderV2(cancel.clone());
        instruction.data = cancel_instruction.pack();
        solana_program::program::invoke_signed(&instruction, &account_infos, signers_seeds)?;
    }

    Ok(())
}

fn invoke_transfer<'a>(
    token_prog_acc: &AccountInfo<'a>,
    source_acc: &AccountInfo<'a>,
    dest_acc: &AccountInfo<'a>,
    authority_acc: &AccountInfo<'a>,
    signers_seeds: &[&[&[u8]]],
    quantity: u64

) -> ProgramResult {
    let transfer_instruction = spl_token::instruction::transfer(
        &spl_token::ID,
        source_acc.key,
        dest_acc.key,
        authority_acc.key,
        &[],
        quantity
    )?;
    let accs = [
        token_prog_acc.clone(),
        source_acc.clone(),
        dest_acc.clone(),
        authority_acc.clone()
    ];

    solana_program::program::invoke_signed(&transfer_instruction, &accs, signers_seeds)
}

fn get_in_out_quantities(
    mango_group: &mut MangoGroup,
    margin_account: &mut MarginAccount,
    open_orders_accs: &[AccountInfo; NUM_MARKETS],
    prices: &[U64F64; NUM_TOKENS],
    in_token_index: usize,
    out_token_index: usize,
    liqor_max_in: u64
) -> MangoResult<(u64, u64)> {
    let deficit_val = margin_account.get_partial_liq_deficit(&mango_group, &prices, open_orders_accs)? + ONE_U64F64;
    let out_avail: U64F64 = margin_account.deposits[out_token_index].checked_mul(mango_group.indexes[out_token_index].deposit).unwrap();
    let out_avail_val = out_avail * prices[out_token_index];

    // liq incentive is max of 1/2 the dist between

    // Can only deposit as much as it is possible to withdraw out_token
    let max_in_val = out_avail_val / PARTIAL_LIQ_INCENTIVE;
    let max_in_val = min(deficit_val, max_in_val);

    // we know prices are not 0; if they are this will error;
    let max_in: U64F64 = max_in_val / prices[in_token_index];
    let native_borrow = margin_account.borrows[in_token_index].checked_mul(
        mango_group.indexes[in_token_index].borrow).unwrap();

    // Can only deposit as much there is borrows to offset in in_token
    let in_quantity = min(min(max_in, native_borrow), U64F64::from_num(liqor_max_in));
    let deposit: U64F64 = in_quantity / mango_group.indexes[in_token_index].borrow;

    // TODO if borrowed is close to Deposit, just set borrowed == 0
    checked_sub_borrow(mango_group, margin_account, in_token_index, deposit)?;

    // Withdraw incentive funds to liqor
    let in_val: U64F64 = in_quantity.checked_mul(prices[in_token_index]).unwrap();
    let out_val: U64F64 = in_val * PARTIAL_LIQ_INCENTIVE;
    let out_quantity: U64F64 = out_val / prices[out_token_index];

    let withdraw = out_quantity / mango_group.indexes[out_token_index].deposit;

    checked_sub_deposit(mango_group, margin_account, out_token_index, withdraw)?;

    // TODO account for the rounded amounts as deposits -- could be valuable in some tokens

    Ok((in_quantity.checked_ceil().unwrap().to_num(), out_quantity.checked_floor().unwrap().to_num()))
}
