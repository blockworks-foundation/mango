#![cfg(feature="test-bpf")]

use std::mem::size_of;
use std::convert::TryInto;
use safe_transmute::{self, to_bytes::transmute_one_to_bytes};

use fixed::types::U64F64;
use common::create_signer_key_and_nonce;
use flux_aggregator::borsh_utils;
use flux_aggregator::borsh_state::BorshState;
use flux_aggregator::state::{Aggregator, AggregatorConfig, Answer};
use solana_program::program_option::COption;
use solana_program::program_pack::Pack;
use solana_program::pubkey::Pubkey;
use solana_program_test::{ProgramTest, BanksClient};

use solana_sdk::{
    account_info::IntoAccountInfo,
    account::Account,
    instruction::Instruction,
    signature::{Keypair, Signer}
};

use spl_token::state::{Mint, Account as Token, AccountState};
use serum_dex::state::{MarketState, AccountFlag, ToAlignedBytes};

use mango::processor::srm_token;
use mango::instruction::init_mango_group;
use mango::state::MangoGroup;

pub const PRICE_BTC: u64 = 50000;
pub const PRICE_ETH: u64 = 2000;

trait AddPacked {
    fn add_packable_account<T: Pack>(
        &mut self,
        pubkey: Pubkey,
        amount: u64,
        data: &T,
        owner: &Pubkey,
    );
}

impl AddPacked for ProgramTest {
    fn add_packable_account<T: Pack>(
        &mut self,
        pubkey: Pubkey,
        amount: u64,
        data: &T,
        owner: &Pubkey,
    ) {
        let mut account = Account::new(amount, T::get_packed_len(), owner);
        data.pack_into_slice(&mut account.data);
        self.add_account(pubkey, account);
    }
}


pub struct TestMint {
    pub pubkey: Pubkey,
    pub authority: Keypair,
    pub decimals: u8,
}


pub fn add_mint(test: &mut ProgramTest, decimals: u8) -> TestMint {
    let authority = Keypair::new();
    let pubkey = Pubkey::new_unique();
    test.add_packable_account(
        pubkey,
        u32::MAX as u64,
        &Mint {
            is_initialized: true,
            mint_authority: COption::Some(authority.pubkey()),
            decimals,
            ..Mint::default()
        },
        &spl_token::id(),
    );
    TestMint {
        pubkey,
        authority,
        decimals,
    }
}

pub fn add_mint_srm(test: &mut ProgramTest) -> TestMint {
    let authority = Keypair::new();
    let pubkey = srm_token::ID;
    let decimals = 6;
    test.add_packable_account(
        pubkey,
        u32::MAX as u64,
        &Mint {
            is_initialized: true,
            mint_authority: COption::Some(authority.pubkey()),
            decimals,
            ..Mint::default()
        },
        &spl_token::id(),
    );
    TestMint {
        pubkey,
        authority,
        decimals,
    }
}

pub struct TestDex {
    pub pubkey: Pubkey,
}

pub fn add_dex_empty(test: &mut ProgramTest, base_mint: Pubkey, quote_mint: Pubkey, dex_prog_id: Pubkey) -> TestDex {
    let pubkey = Pubkey::new_unique();
    let mut acc = Account::new(u32::MAX as u64, 0, &dex_prog_id);
    let ms = MarketState {
        account_flags: (AccountFlag::Initialized | AccountFlag::Market).bits(),
        own_address: pubkey.to_aligned_bytes(),
        vault_signer_nonce: 0,
        coin_mint: base_mint.to_aligned_bytes(),
        pc_mint: quote_mint.to_aligned_bytes(),

        coin_vault: Pubkey::new_unique().to_aligned_bytes(),
        coin_deposits_total: 0,
        coin_fees_accrued: 0,

        pc_vault: Pubkey::new_unique().to_aligned_bytes(),
        pc_deposits_total: 0,
        pc_fees_accrued: 0,
        pc_dust_threshold: 0,

        req_q: Pubkey::new_unique().to_aligned_bytes(),
        event_q: Pubkey::new_unique().to_aligned_bytes(),
        bids: Pubkey::new_unique().to_aligned_bytes(),
        asks: Pubkey::new_unique().to_aligned_bytes(),

        coin_lot_size: 1,
        pc_lot_size: 1,

        fee_rate_bps: 1,
        referrer_rebates_accrued: 0,
    };
    let head: &[u8; 5] = b"serum";
    let tail: &[u8; 7] = b"padding";
    let data = transmute_one_to_bytes(&ms);
    let mut accdata = vec![];
    accdata.extend(head);
    accdata.extend(data);
    accdata.extend(tail);
    acc.data = accdata;

    test.add_account(pubkey, acc);
    TestDex { pubkey }
}

pub struct TestTokenAccount {
    pub pubkey: Pubkey,
}

pub fn add_token_account(test: &mut ProgramTest, owner: Pubkey, mint: Pubkey, initial_balance: u64) -> TestTokenAccount {
    let pubkey = Pubkey::new_unique();
    test.add_packable_account(
        pubkey,
        u32::MAX as u64,
        &Token {
            mint: mint,
            owner: owner,
            amount: initial_balance,
            state: AccountState::Initialized,
            ..Token::default()
        },
        &spl_token::id(),
    );
    TestTokenAccount { pubkey }
}

pub struct TestAggregator {
    pub name: String,
    pub pubkey: Pubkey,
    pub price: u64,
}

pub fn add_aggregator(test: &mut ProgramTest, name: &str, decimals: u8, price: u64, owner: &Pubkey) -> TestAggregator {
    let pubkey = Pubkey::new_unique();

    let mut description = [0u8; 32];
    let size = name.len().min(description.len());
    description[0..size].copy_from_slice(&name.as_bytes()[0..size]);

    let aggregator = Aggregator {
        config: AggregatorConfig {
            description,
            decimals,
            ..AggregatorConfig::default()
        },
        is_initialized: true,
        answer: Answer {
            median: price,
            created_at: 1, // set to > 0 to initialize
            ..Answer::default()
        },
        ..Aggregator::default()
    };

    let mut account = Account::new(
        u32::MAX as u64,
        borsh_utils::get_packed_len::<Aggregator>(),
        &owner,
    );
    let account_info = (&pubkey, false, &mut account).into_account_info();
    aggregator.save(&account_info).unwrap();
    test.add_account(pubkey, account);

    TestAggregator {
        name: name.to_string(),
        pubkey,
        price,
    }
}

// Holds all of the dependencies for a MangoGroup
pub struct TestMangoGroup {
    pub program_id: Pubkey,
    pub mango_group_pk: Pubkey,
    pub signer_pk: Pubkey,
    pub signer_nonce: u64,
    
    // Mints and Vaults must ordered with base assets first, quote asset last
    // They must be ordered in the same way
    pub mints: Vec<TestMint>,
    pub vaults: Vec<TestTokenAccount>,

    pub srm_mint: TestMint,
    pub srm_vault: TestTokenAccount,

    pub dex_prog_id: Pubkey,
    // Dexes and Oracles must be sorted in the same way as the first n-1 mints
    // mints[x] / mints[-1]
    pub dexes: Vec<TestDex>,
    pub oracles: Vec<TestAggregator>,

    pub borrow_limits: Vec<u64>,
}


// This should probably go into the main code at some point when we remove the hard-coded market sizes
fn to_fixed_array<T, const N: usize>(v: Vec<T>) -> [T; N] {
    v.try_into().unwrap_or_else(|v: Vec<T>| panic!("Expected a Vec of length {} but it was {}", N, v.len()))
}

impl TestMangoGroup {
    pub fn init_mango_group(&self, payer: &Pubkey) -> Instruction {
        init_mango_group(
            &self.program_id,
            &self.mango_group_pk,
            &self.signer_pk,
            &self.dex_prog_id,
            &self.srm_vault.pubkey,
            payer,
            self.mints.iter().map(|m| m.pubkey).collect::<Vec<Pubkey>>().as_slice(),
            self.vaults.iter().map(|m| m.pubkey).collect::<Vec<Pubkey>>().as_slice(),
            self.dexes.iter().map(|m| m.pubkey).collect::<Vec<Pubkey>>().as_slice(),
            self.oracles.iter().map(|m| m.pubkey).collect::<Vec<Pubkey>>().as_slice(),
            self.signer_nonce,
            U64F64::from_num(1.1),
            U64F64::from_num(1.2),
            to_fixed_array(self.borrow_limits.clone()),
        ).unwrap()
    }
}

pub fn add_mango_group_prodlike(test: &mut ProgramTest, program_id: Pubkey) -> TestMangoGroup {
    let mango_group_pk = Pubkey::new_unique();
    let (signer_pk, signer_nonce) = create_signer_key_and_nonce(&program_id, &mango_group_pk);
    test.add_account(mango_group_pk, Account::new(u32::MAX as u64, size_of::<MangoGroup>(), &program_id));

    let btc_mint = add_mint(test, 6);
    let eth_mint = add_mint(test, 6);
    let usdt_mint = add_mint(test, 6);

    let btc_vault = add_token_account(test, signer_pk, btc_mint.pubkey, 0);
    let eth_vault = add_token_account(test, signer_pk, eth_mint.pubkey, 0);
    let usdt_vault = add_token_account(test, signer_pk, usdt_mint.pubkey, 0);

    let srm_mint = add_mint_srm(test);
    let srm_vault = add_token_account(test, signer_pk, srm_mint.pubkey, 0);

    let dex_prog_id = Pubkey::new_unique();
    let btc_usdt_dex = add_dex_empty(test, btc_mint.pubkey, usdt_mint.pubkey, dex_prog_id);
    let eth_usdt_dex = add_dex_empty(test, eth_mint.pubkey, usdt_mint.pubkey, dex_prog_id);

    let unit = 10u64.pow(6);
    let btc_usdt = add_aggregator(test, "BTC:USDT", 6, PRICE_BTC * unit, &program_id);
    let eth_usdt = add_aggregator(test, "ETH:USDT", 6, PRICE_ETH * unit, &program_id);

    let mints = vec![btc_mint, eth_mint, usdt_mint];
    let vaults = vec![btc_vault, eth_vault, usdt_vault];
    let dexes = vec![btc_usdt_dex, eth_usdt_dex];
    let oracles = vec![btc_usdt, eth_usdt];
    let borrow_limits = vec![100, 100, 100];

    TestMangoGroup {
        program_id,
        mango_group_pk,
        signer_pk,
        signer_nonce,
        mints,
        vaults,
        srm_mint,
        srm_vault,
        dex_prog_id,
        dexes,
        oracles,
        borrow_limits,
    }
}

#[allow(dead_code)]  // Compiler complains about this even tho it is used
pub async fn get_token_balance(banks_client: &mut BanksClient, pubkey: Pubkey) -> u64 {
    let token: Account = banks_client.get_account(pubkey).await.unwrap().unwrap();

    spl_token::state::Account::unpack(&token.data[..])
        .unwrap()
        .amount
}