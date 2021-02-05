use std::cmp;
use std::mem::size_of;

use arrayref::{array_ref, array_refs};
use fixed::types::U64F64;
use serum_dex::matching::Side;
use serum_dex::state::ToAlignedBytes;
use solana_program::account_info::AccountInfo;
use solana_program::clock::Clock;
use solana_program::instruction::{AccountMeta, Instruction};
use solana_program::msg;
use solana_program::program_error::ProgramError;
use solana_program::program_pack::{IsInitialized, Pack};
use solana_program::pubkey::Pubkey;
use solana_program::rent::Rent;
use solana_program::sysvar::Sysvar;
use spl_token::state::{Account, Mint};

use crate::error::{check_assert, MangoResult, SourceFileId};
use crate::instruction::MangoInstruction;
use crate::state::{AccountFlag, check_open_orders, load_market_state, load_open_orders, Loadable, MangoGroup, MangoIndex, MarginAccount, NUM_MARKETS, NUM_TOKENS};
use crate::utils::{gen_signer_key, gen_signer_seeds};

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
    #[inline(never)]
    fn init_mango_group(
        program_id: &Pubkey,
        accounts: &[AccountInfo],
        signer_nonce: u64,
        maint_coll_ratio: U64F64,
        init_coll_ratio: U64F64
    ) -> MangoResult<()> {
        const NUM_FIXED: usize = 5;
        let accounts = array_ref![accounts, 0, NUM_FIXED + 2 * NUM_TOKENS + 2 * NUM_MARKETS];
        let (
            fixed_accs,
            token_mint_accs,
            vault_accs,
            spot_market_accs,
            oracle_accs
        ) = array_refs![accounts, NUM_FIXED, NUM_TOKENS, NUM_TOKENS, NUM_MARKETS, NUM_MARKETS];

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
            // TODO how to verify these are valid oracle acccounts?
            mango_group.oracles[i] = *oracle_accs[i].key;
        }

        Ok(())
    }

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

        MangoGroup::load_checked(mango_group_acc, program_id)?;
        let mut margin_account = MarginAccount::load_mut(margin_account_acc)?;
        let rent = Rent::from_account_info(rent_acc)?;

        prog_assert_eq!(margin_account_acc.owner, program_id)?;
        prog_assert!(rent.is_exempt(margin_account_acc.lamports(), size_of::<MarginAccount>()))?;
        prog_assert_eq!(margin_account.account_flags, 0)?;
        prog_assert!(owner_acc.is_signer)?;

        margin_account.account_flags = (AccountFlag::Initialized | AccountFlag::MarginAccount).bits();
        margin_account.mango_group = *mango_group_acc.key;
        margin_account.owner = *owner_acc.key;

        Ok(())
    }

    fn deposit(
        program_id: &Pubkey,
        accounts: &[AccountInfo],
        quantity: u64
    ) -> MangoResult<()> {
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
        margin_account.checked_add_deposit(token_index, deposit)?;
        mango_group.checked_add_deposit(token_index, deposit)?;

        Ok(())
    }

    fn withdraw(
        program_id: &Pubkey,
        accounts: &[AccountInfo],
        token_index: usize,
        quantity: u64
    ) -> MangoResult<()> {
        const NUM_FIXED: usize = 8;
        let accounts = array_ref![accounts, 0, NUM_FIXED + 2 * NUM_MARKETS + NUM_TOKENS];
        let (
            fixed_accs,
            open_orders_accs,
            oracle_accs,
            mint_accs
        ) = array_refs![accounts, NUM_FIXED, NUM_MARKETS, NUM_MARKETS, NUM_TOKENS];

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

        let mut mango_group = MangoGroup::load_mut_checked(mango_group_acc, program_id)?;
        let mut margin_account = MarginAccount::load_mut_checked(
            margin_account_acc, mango_group_acc.key
        )?;
        let clock = Clock::from_account_info(clock_acc)?;
        mango_group.update_indexes(&clock)?;

        prog_assert!(owner_acc.is_signer)?;
        prog_assert_eq!(&margin_account.owner, owner_acc.key)?;

        for i in 0..NUM_MARKETS {
            prog_assert_eq!(open_orders_accs[i].key, &margin_account.open_orders[i])?;
            check_open_orders(&open_orders_accs[i], signer_acc.key)?;
        }

        prog_assert_eq!(&mango_group.vaults[token_index], vault_acc.key)?;

        let index: &MangoIndex = &mango_group.indexes[token_index];
        let native_deposits: u64 = (margin_account.deposits[token_index] * index.deposit).to_num();
        let available = native_deposits;

        prog_assert!(available >= quantity)?;
        // TODO just borrow (quantity - available)

        let prices = get_prices(&mango_group, mint_accs, oracle_accs)?;

        // Withdraw from deposit
        let withdrew: U64F64 = U64F64::from_num(quantity) / index.deposit;
        margin_account.checked_sub_deposit(token_index, withdrew)?;
        mango_group.checked_sub_deposit(token_index, withdrew)?;

        // Make sure accounts are in valid state after withdrawal
        let coll_ratio = margin_account.get_collateral_ratio(&mango_group, &prices, open_orders_accs)?;
        prog_assert!(coll_ratio >= mango_group.init_coll_ratio)?;
        prog_assert!(mango_group.has_valid_deposits_borrows(token_index))?;

        // Send out withdraw instruction to SPL token program
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

    fn borrow(
        program_id: &Pubkey,
        accounts: &[AccountInfo],
        token_index: usize,
        quantity: u64
    ) -> MangoResult<()> {
        const NUM_FIXED: usize = 4;
        let accounts = array_ref![accounts, 0, NUM_FIXED + 2 * NUM_MARKETS + NUM_TOKENS];
        let (
            fixed_accs,
            open_orders_accs,
            oracle_accs,
            mint_accs
        ) = array_refs![accounts, NUM_FIXED, NUM_MARKETS, NUM_MARKETS, NUM_TOKENS];

        let [
            mango_group_acc,
            margin_account_acc,
            owner_acc,
            clock_acc,
        ] = fixed_accs;

        let mut mango_group = MangoGroup::load_mut_checked(mango_group_acc, program_id)?;
        let mut margin_account = MarginAccount::load_mut_checked(
            margin_account_acc, mango_group_acc.key
        )?;
        prog_assert!(owner_acc.is_signer)?;
        prog_assert_eq!(&margin_account.owner, owner_acc.key)?;

        for i in 0..NUM_MARKETS {
            prog_assert_eq!(open_orders_accs[i].key, &margin_account.open_orders[i])?;
            check_open_orders(&open_orders_accs[i], &mango_group.signer_key)?;
        }
        let clock = Clock::from_account_info(clock_acc)?;
        mango_group.update_indexes(&clock)?;

        let index: &MangoIndex = &mango_group.indexes[token_index];

        let borrow = U64F64::from_num(quantity) / index.borrow;
        let deposit = U64F64::from_num(quantity) / index.deposit;
        margin_account.checked_add_deposit(token_index, deposit)?;
        mango_group.checked_add_deposit(token_index, deposit)?;
        margin_account.checked_add_borrow(token_index, borrow)?;
        mango_group.checked_add_borrow(token_index, borrow)?;

        let prices = get_prices(&mango_group, mint_accs, oracle_accs)?;
        let coll_ratio = margin_account.get_collateral_ratio(&mango_group, &prices, open_orders_accs)?;

        prog_assert!(coll_ratio >= mango_group.init_coll_ratio)?;
        prog_assert!(mango_group.has_valid_deposits_borrows(token_index))?;

        Ok(())
    }

    // Use deposits to offset borrows to 0
    // Client expected to close positions and open ordres first
    // and make sure there is enough funds in positions and deposits to close
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
            margin_account_acc, mango_group_acc.key
        )?;
        let clock = Clock::from_account_info(clock_acc)?;
        mango_group.update_indexes(&clock)?;
        prog_assert!(owner_acc.is_signer)?;
        prog_assert_eq!(&margin_account.owner, owner_acc.key)?;

        let index: &MangoIndex = &mango_group.indexes[token_index];

        let native_borrow = margin_account.get_native_borrow(index, token_index);
        let native_deposit = margin_account.get_native_deposit(index, token_index);

        let quantity = cmp::min(cmp::min(quantity, native_borrow), native_deposit);

        let borr_settle = U64F64::from_num(quantity) / index.borrow;
        let dep_settle = U64F64::from_num(quantity) / index.deposit;
        margin_account.checked_sub_borrow(token_index, borr_settle)?;
        margin_account.checked_sub_deposit(token_index, dep_settle)?;
        mango_group.checked_sub_borrow(token_index, borr_settle)?;
        mango_group.checked_sub_deposit(token_index, dep_settle)?;

        // No need to check collateralization ratio or deposits/borrows validity

        Ok(())
    }

    fn liquidate(
        program_id: &Pubkey,
        accounts: &[AccountInfo],
        deposit_quantities: [u64; NUM_TOKENS]
    ) -> MangoResult<()> {
        const NUM_FIXED: usize = 5;
        let accounts = array_ref![accounts, 0, NUM_FIXED + 2 * NUM_MARKETS + 3 * NUM_TOKENS];
        let (
            fixed_accs,
            open_orders_accs,
            oracle_accs,
            vault_accs,
            liqor_token_account_accs,
            mint_accs
        ) = array_refs![accounts, NUM_FIXED, NUM_MARKETS, NUM_MARKETS, NUM_TOKENS, NUM_TOKENS, NUM_TOKENS];

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
        let mut mango_group = MangoGroup::load_mut_checked(
            mango_group_acc, program_id
        )?;
        let mut liqee_margin_account = MarginAccount::load_mut_checked(
            liqee_margin_account_acc, mango_group_acc.key
        )?;
        let clock = Clock::from_account_info(clock_acc)?;
        mango_group.update_indexes(&clock)?;

        for i in 0..NUM_MARKETS {
            prog_assert_eq!(open_orders_accs[i].key, &liqee_margin_account.open_orders[i])?;
            check_open_orders(&open_orders_accs[i], liqee_margin_account_acc.key)?;
        }

        let prices = get_prices(&mango_group, mint_accs, oracle_accs)?;
        let coll_ratio = liqee_margin_account.get_collateral_ratio(
            &mango_group, &prices, open_orders_accs)?;


        // No liquidations if account above maint collateral ratio
        prog_assert!(coll_ratio < mango_group.maint_coll_ratio)?;

        // TODO need to SettleFunds or SettleBorrow first to make sure user is still liquidatable

        // liquidator may forcefully SettleFunds and SettleBorrow on account with less than maint

        if coll_ratio < U64F64::from_num(1) {
            let liabs = liqee_margin_account.get_total_liabs(&mango_group)?;
            let liabs_val = liqee_margin_account.get_liabs_val(&mango_group, &prices)?;
            let assets_val = liqee_margin_account.get_assets_val(&mango_group, &prices, open_orders_accs)?;

            // reduction_val = amount of quote currency value to reduce liabilities by to get coll_ratio = 1.01
            let reduction_val = liabs_val
                .checked_sub(assets_val / U64F64::from_num(1.01)).unwrap();

            for i in 0..NUM_TOKENS {
                let proportion = U64F64::from_num(liabs[i])
                    .checked_div(liabs_val).unwrap();

                let token_reduce = proportion.checked_mul(reduction_val).unwrap();
                Self::socialize_loss(&mut mango_group, &mut liqee_margin_account, i, token_reduce)?;
                // TODO this will reduce deposits of liqee as well which could put actual value below; way to fix is to SettleBorrow first
                // TODO Can socialize loss cause more liquidations?
            }
        }

        // Pull deposits from liqor's token wallets
        for i in 0..NUM_TOKENS {
            let quantity = deposit_quantities[i];
            if quantity == 0 {
                continue;
            }

            let vault_acc: &AccountInfo = &vault_accs[i];
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
            liqee_margin_account.checked_add_deposit(i, deposit)?;
            mango_group.checked_add_deposit(i, deposit)?;
        }

        // Check to make sure liqor's deposits brought account above init_coll_ratio
        let coll_ratio = liqee_margin_account.get_collateral_ratio(&mango_group, &prices, open_orders_accs)?;
        prog_assert!(coll_ratio >= mango_group.init_coll_ratio)?;

        // If all deposits are good, transfer ownership of margin account to liqor
        liqee_margin_account.owner = *liqor_acc.key;

        Ok(())
    }


    fn socialize_loss(
        mango_group: &mut MangoGroup,
        margin_account: &mut MarginAccount,
        token_index: usize,
        reduce_quantity_native: U64F64
    ) -> MangoResult<()> {

        // reduce borrow for this margin_account by appropriate amount
        // decrease MangoIndex.deposit by appropriate amount

        let quantity: U64F64 = reduce_quantity_native / mango_group.indexes[token_index].borrow;
        margin_account.checked_sub_borrow(token_index, quantity)?;
        mango_group.checked_sub_borrow(token_index, quantity)?;

        let total_deposits = U64F64::from_num(mango_group.get_total_native_deposit(token_index));
        let percentage_loss = reduce_quantity_native.checked_div(total_deposits).unwrap();
        let index: &mut MangoIndex = &mut mango_group.indexes[token_index];
        index.deposit = index.deposit
            .checked_sub(percentage_loss.checked_mul(index.deposit).unwrap()).unwrap();

        Ok(())
    }
    #[inline(never)]
    fn place_order(
        program_id: &Pubkey,
        accounts: &[AccountInfo],
        order: serum_dex::instruction::NewOrderInstructionV2
    ) -> MangoResult<()> {
        // TODO if account is below init_margin_ratio then allow reducing orders
        // what counts as a reducing order?
        // when you are trying to buy an asset in which you have a net borrow position
            // there must necessarily be at least one where you have a net borrow position

        const NUM_FIXED: usize = 13;
        let accounts = array_ref![accounts, 0, NUM_FIXED + 2 * NUM_MARKETS + NUM_TOKENS];
        let (
            fixed_accs,
            open_orders_accs,
            oracle_accs,
            mint_accs
        ) = array_refs![accounts, NUM_FIXED, NUM_MARKETS, NUM_MARKETS, NUM_TOKENS];

        let [
            mango_group_acc,
            owner_acc,
            margin_account_acc,
            clock_acc,
            dex_prog_acc,
            spot_market_acc,
            dex_request_queue_acc,
            vault_acc,
            signer_acc,
            dex_base_acc,
            dex_quote_acc,
            token_prog_acc,
            rent_acc,
        ] = fixed_accs;


        let mut mango_group = MangoGroup::load_mut_checked(mango_group_acc, program_id)?;
        let mut margin_account = MarginAccount::load_mut_checked(
            margin_account_acc, mango_group_acc.key
        )?;

        let clock = Clock::from_account_info(clock_acc)?;
        mango_group.update_indexes(&clock)?;

        prog_assert!(owner_acc.is_signer)?;
        prog_assert_eq!(&margin_account.owner, owner_acc.key)?;

        let market_i = mango_group.get_market_index(spot_market_acc.key).unwrap();
        let token_i = match order.side {
            Side::Bid => NUM_MARKETS,
            Side::Ask => market_i
        };
        prog_assert_eq!(&mango_group.vaults[token_i], vault_acc.key)?;

        let pre_amount = {  // this is to keep track of how much funds were transferred out
            let vault = Account::unpack(&vault_acc.try_borrow_data()?)?;
            vault.amount
        };

        for i in 0..NUM_MARKETS {
            let open_orders_acc = &open_orders_accs[i];
            if i == market_i {  // this one must not be default pubkey
                prog_assert!(*open_orders_acc.key != Pubkey::default())?;
                if margin_account.open_orders[i] == Pubkey::default() {
                    let open_orders = load_open_orders(open_orders_acc)?;
                    prog_assert_eq!(open_orders.account_flags, 0)?;
                    margin_account.open_orders[i] = *open_orders_acc.key;
                }
            } else {
                prog_assert_eq!(open_orders_accs[i].key, &margin_account.open_orders[i])?;
                check_open_orders(&open_orders_accs[i], margin_account_acc.key)?;
            }
        }

        let data = serum_dex::instruction::MarketInstruction::NewOrderV2(order).pack();
        let instruction = Instruction {
            program_id: *dex_prog_acc.key,
            data,
            accounts: vec![
                AccountMeta::new(*spot_market_acc.key, false),
                AccountMeta::new(*open_orders_accs[market_i].key, false),
                AccountMeta::new(*dex_request_queue_acc.key, false),
                AccountMeta::new(*vault_acc.key, false),
                AccountMeta::new_readonly(*signer_acc.key, true),
                AccountMeta::new(*dex_base_acc.key, false),
                AccountMeta::new(*dex_quote_acc.key, false),
                AccountMeta::new_readonly(*token_prog_acc.key, false),
                AccountMeta::new_readonly(*rent_acc.key, false),
            ],
        };
        let account_infos = [
            dex_prog_acc.clone(),  // Have to add account of the program id
            spot_market_acc.clone(),
            open_orders_accs[market_i].clone(),
            dex_request_queue_acc.clone(),
            vault_acc.clone(),
            signer_acc.clone(),
            dex_base_acc.clone(),
            dex_quote_acc.clone(),
            token_prog_acc.clone(),
            rent_acc.clone(),
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
            margin_account.checked_sub_deposit(token_i, spent_deposit)?;
            mango_group.checked_sub_deposit(token_i, spent_deposit)?;
        } else {
            margin_account.deposits[token_i] = U64F64::from_num(0);

            let rem_spend = U64F64::from_num(spent - native_deposit);
            margin_account.checked_add_borrow(token_i, rem_spend / index.borrow)?;
            mango_group.checked_add_borrow(token_i, rem_spend / index.borrow)?;
        }

        // TODO optimize redundant calculations
        let prices = get_prices(&mango_group, mint_accs, oracle_accs)?;
        let coll_ratio = margin_account.get_collateral_ratio(&mango_group, &prices, open_orders_accs)?;

        if coll_ratio < mango_group.init_coll_ratio { // only allow this if it reduces position
            let assets = margin_account.get_total_assets(&mango_group, open_orders_accs)?;
            let liabs = margin_account.get_total_liabs(&mango_group)?;

            if token_i == NUM_MARKETS {  // means this is a bid
                // means we are increasing market_i pos
                // make sure market_i pos is already net borrowed
                // strictest req make sure base currency is net borrowed
                // strictest req make sure quote currency is not net borrowed
                // TODO see if these reqs can be relaxed
                prog_assert!(assets[market_i] < liabs[market_i] && assets[NUM_MARKETS] >= liabs[NUM_MARKETS])?;

            } else {  // means this is offer,
                // strictest req make sure quote currency is net borrowed
                // strictest req make sure base currency not net borrowed
                prog_assert!(assets[NUM_MARKETS] < liabs[NUM_MARKETS] && assets[market_i] >= liabs[market_i])?;
            }
        }

        prog_assert!(mango_group.has_valid_deposits_borrows(token_i))?;
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
            margin_account_acc,
            mango_group_acc.key
        )?;
        let clock = Clock::from_account_info(clock_acc)?;
        mango_group.update_indexes(&clock)?;

        let market_i = mango_group.get_market_index(spot_market_acc.key).unwrap();

        prog_assert!(owner_acc.is_signer)?;
        prog_assert_eq!(owner_acc.key, &margin_account.owner)?;
        prog_assert_eq!(&margin_account.open_orders[market_i], open_orders_acc.key)?;
        prog_assert_eq!(base_vault_acc.key, &mango_group.vaults[market_i])?;
        prog_assert_eq!(quote_vault_acc.key, &mango_group.vaults[NUM_MARKETS])?;

        if *open_orders_acc.key == Pubkey::default() {
            return Ok(());
        }

        let (pre_base, pre_quote) = {
            let open_orders = load_open_orders(open_orders_acc)?;
            (open_orders.native_coin_free, open_orders.native_pc_free)
        };

        if pre_base == 0 && pre_quote == 0 {
            return Ok(());
        }

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
            token_prog_acc.clone()
        ];
        let signer_seeds = gen_signer_seeds(&mango_group.signer_nonce, mango_group_acc.key);
        solana_program::program::invoke_signed(&instruction, &account_infos, &[&signer_seeds])?;

        let (post_base, post_quote) = {
            let open_orders = load_open_orders(open_orders_acc)?;
            (open_orders.native_coin_free, open_orders.native_pc_free)
        };

        prog_assert!(post_base <= pre_base)?;
        prog_assert!(post_quote <= pre_quote)?;

        margin_account.checked_add_deposit(
            market_i,
            U64F64::from_num(pre_base - post_base) / mango_group.indexes[market_i].deposit
        )?;
        margin_account.checked_add_deposit(
            NUM_MARKETS,
            U64F64::from_num(pre_quote - post_quote) / mango_group.indexes[NUM_MARKETS].deposit
        )?;

        Ok(())
    }

    // Once it is settled build back
    fn cancel_order(
        program_id: &Pubkey,
        accounts: &[AccountInfo],
        data: Vec<u8>
    ) -> MangoResult<()> {
        const NUM_FIXED: usize = 9;
        let accounts = array_ref![accounts, 0, NUM_FIXED];

        let [
            mango_group_acc,
            owner_acc,  // signer
            margin_account_acc,
            clock_acc,
            dex_prog_acc,
            spot_market_acc,
            open_orders_acc,
            dex_request_queue_acc,
            signer_acc,
        ] = accounts;

        let mut mango_group = MangoGroup::load_mut_checked(mango_group_acc, program_id)?;
        let margin_account = MarginAccount::load_checked(
            margin_account_acc,
            mango_group_acc.key
        )?;
        let clock = Clock::from_account_info(clock_acc)?;
        mango_group.update_indexes(&clock)?;

        prog_assert!(owner_acc.is_signer)?;
        prog_assert_eq!(&margin_account.owner, owner_acc.key)?;
        let market_i = mango_group.get_market_index(spot_market_acc.key).unwrap();
        prog_assert_eq!(&margin_account.open_orders[market_i], open_orders_acc.key)?;

        let instruction = Instruction {
            program_id: *dex_prog_acc.key,
            data,
            accounts: vec![
                AccountMeta::new_readonly(*spot_market_acc.key, false),
                AccountMeta::new(*open_orders_acc.key, false),
                AccountMeta::new(*dex_request_queue_acc.key, false),
                AccountMeta::new_readonly(*signer_acc.key, true),
            ],
        };

        let account_infos = [
            dex_prog_acc.clone(),
            spot_market_acc.clone(),
            open_orders_acc.clone(),
            dex_request_queue_acc.clone(),
            signer_acc.clone()
        ];
        let signer_seeds = gen_signer_seeds(&mango_group.signer_nonce, mango_group_acc.key);
        solana_program::program::invoke_signed(&instruction, &account_infos, &[&signer_seeds])?;

        Ok(())
    }

    pub fn process(
        program_id: &Pubkey,
        accounts: &[AccountInfo],
        data: &[u8]
    ) -> MangoResult<()> {
        msg!("Mango Processor");
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
                token_index,
                quantity
            } => {
                msg!("Withdraw");
                Self::withdraw(program_id, accounts, token_index, quantity)?;
            }
            MangoInstruction::Borrow {
                token_index,
                quantity
            } => {
                msg!("Borrow");
                Self::borrow(program_id, accounts, token_index, quantity)?;
            }
            MangoInstruction::SettleBorrow {
                token_index,
                quantity
            } => {
                msg!("SettleBorrow");
                Self::settle_borrow(program_id, accounts, token_index, quantity)?;
            }
            MangoInstruction::Liquidate {
                deposit_quantities
            } => {
                // Either user takes the position
                // Or the program can liquidate on the serum dex (in case no liquidator wants to take pos)
                msg!("Liquidate");
                Self::liquidate(program_id, accounts, deposit_quantities)?;
            }
            MangoInstruction::PlaceOrder {
                order
            } => {
                msg!("PlaceOrder");
                Self::place_order(program_id, accounts, order)?;
            }
            MangoInstruction::SettleFunds => {
                msg!("SettleFunds");
                Self::settle_funds(program_id, accounts)?;
            }
            MangoInstruction::CancelOrder {
                order
            } => {
                msg!("CancelOrder");
                let data =  serum_dex::instruction::MarketInstruction::CancelOrder(order).pack();
                Self::cancel_order(program_id, accounts, data)?;
            }
            MangoInstruction::CancelOrderByClientId {
                client_id
            } => {
                msg!("CancelOrderByClientId");
                Self::cancel_order(program_id, accounts, client_id.to_le_bytes().to_vec())?;
            }
        }
        Ok(())
    }

}

pub fn get_prices(
    mango_group: &MangoGroup,
    mint_accs: &[AccountInfo],
    oracle_accs: &[AccountInfo],
) -> MangoResult<[U64F64; NUM_TOKENS]> {
    let mut prices = [U64F64::from_num(0); NUM_TOKENS];
    prices[NUM_MARKETS] = U64F64::from_num(1);  // quote currency is 1
    prog_assert_eq!(mint_accs[NUM_MARKETS].key, &mango_group.tokens[NUM_MARKETS])?;
    let quote_mint = Mint::unpack(&mint_accs[NUM_MARKETS].try_borrow_data()?)?;

    // TODO: assumes oracle multiplied by 100 to represent cents; remove assumption
    let quote_adj = U64F64::from_num(10u64.pow(quote_mint.decimals.checked_sub(2).unwrap() as u32));

    for i in 0..NUM_MARKETS {
        let value = flux_aggregator::get_median(&oracle_accs[i])?; // this is in USD cents
        let value = U64F64::from_num(value);

        prog_assert_eq!(mint_accs[i].key, &mango_group.tokens[i])?;
        let mint = Mint::unpack(&mint_accs[i].try_borrow_data()?)?;

        let base_adj = U64F64::from_num(10u64.pow(mint.decimals as u32));
        prices[i] = quote_adj
            .checked_div(base_adj).unwrap()
            .checked_mul(value).unwrap();

        // TODO: checked mul, checked div
        // n UI USDC / 1 UI coin
        // mul 10 ^ (quote decimals - oracle decimals) / 10 ^ (base decimals)
    }
    Ok(prices)
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