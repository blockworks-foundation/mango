use std::cell::Ref;
use std::mem::size_of;

use arrayref::{array_ref, array_refs};
use bytemuck::from_bytes;
use serum_dex::state::ToAlignedBytes;
use solana_program::account_info::AccountInfo;
use solana_program::clock::Clock;
use solana_program::entrypoint::ProgramResult;
use solana_program::program_error::ProgramError;
use solana_program::program_pack::{IsInitialized, Pack};
use solana_program::pubkey::Pubkey;
use solana_program::rent::Rent;
use solana_program::sysvar::Sysvar;
use spl_token::state::Account;

use crate::instruction::MangoInstruction;
use crate::state::{AccountFlag, Loadable, MangoGroup, MangoIndex, MarginAccount, NUM_MARKETS, NUM_TOKENS};
use crate::utils::gen_signer_key;
use fixed::types::U64F64;


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
        let mut mango_group = MangoGroup::load_mut(mango_group_acc)?;
        let clock = Clock::from_account_info(clock_acc)?;

        assert_eq!(mango_group.account_flags, (AccountFlag::Initialized | AccountFlag::MangoGroup).bits());
        assert_eq!(mango_group_acc.owner, program_id);

        let mut margin_account = MarginAccount::load_mut(margin_account_acc)?;
        let token_index = mango_group.get_token_index(mint_acc.key).unwrap();

        let index: MangoIndex = mango_group.indexes[token_index];
        let native_deposits: U64F64 = mango_group.total_deposits[token_index] * index.deposit;
        let native_borrows: U64F64 = mango_group.total_borrows[token_index] * index.borrow;
        let interest_rate = mango_group.get_interest_rate(token_index) ;
        mango_group.indexes[token_index] = Self::update_index(index, clock, interest_rate, native_deposits, native_borrows);

        assert_eq!(&mango_group.vaults[token_index], vault_acc.key);

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

        margin_account.deposits[token_index] += U64F64::from_num(quantity) / mango_group.indexes[token_index].deposit;

        Ok(())
    }

    /// interest is in units per second (e.g. 0.01 => 1% interest per second)
    fn update_index(
        index: MangoIndex,
        clock: Clock,
        interest_rate: U64F64,
        native_deposits: U64F64,
        native_borrows: U64F64
    ) -> MangoIndex {

        // TODO do checked operations
        let curr_ts = clock.unix_timestamp as u64;
        if curr_ts == index.last_update {
            return index;
        }

        assert!(native_deposits > 0);
        assert!(native_borrows <= native_deposits);

        let utilization: U64F64 = native_borrows / native_deposits;


        let borrow_interest = interest_rate * U64F64::from_num(curr_ts - index.last_update);

        let fee_adj = U64F64::from_num(19) / U64F64::from_num(20);
        let deposit_interest = interest_rate * fee_adj * utilization;

        MangoIndex {
            last_update: curr_ts,
            borrow:  index.borrow + borrow_interest * index.borrow,
            deposit: index.deposit + deposit_interest * index.deposit
        }
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
                Self::init_mango_group(program_id, accounts, signer_nonce)?;
            }
            MangoInstruction::InitMarginAccount => {
                Self::init_margin_account(program_id, accounts)?;
            }
            MangoInstruction::Deposit {
                quantity
            } => {
                Self::deposit(program_id, accounts, quantity)?;
            }
            MangoInstruction::Withdraw => {}
            MangoInstruction::Liquidate => {}
            MangoInstruction::PlaceOrder => {}
            MangoInstruction::SettleFunds => {}
            MangoInstruction::CancelOrder => {}
            MangoInstruction::CancelOrderByClientId => {}
        }
        Ok(())
    }
}


fn strip_dex_padding<'a>(acc: &'a AccountInfo) -> Result<Ref<'a, [u8]>, ProgramError> {
    assert!(acc.data_len() >= 12);
    let unpadded_data: Ref<[u8]> = Ref::map(acc.try_borrow_data()?, |data| {
        let data_len = data.len() - 12;
        let (_, rest) = data.split_at(5);
        let (mid, _) = rest.split_at(data_len);
        mid
    });
    Ok(unpadded_data)
}



fn load_open_orders<'a>(acc: &'a AccountInfo) -> Result<Ref<'a, serum_dex::state::OpenOrders>, ProgramError> {
    Ok(Ref::map(strip_dex_padding(acc)?, from_bytes))
}


/*
TODO
Initial launch
- UI
- funding book
- provide liquidity
- liquidation bot
- cranks
- testing
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