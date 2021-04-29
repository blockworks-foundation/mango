use std::mem::size_of;
use safe_transmute::{self, to_bytes::transmute_one_to_bytes};

use flux_aggregator::borsh_utils;
use flux_aggregator::borsh_state::BorshState;
use flux_aggregator::state::{Aggregator, AggregatorConfig, Answer};
use solana_program::program_option::COption;
use solana_program::program_pack::Pack;
use solana_program::pubkey::Pubkey;

use solana_sdk::account_info::IntoAccountInfo;
use solana_sdk::account::Account;
use solana_sdk::signature::{Keypair, Signer};

use spl_token::state::{Mint, Account as Token, AccountState};

use solana_program_test::ProgramTest;
use mango::processor::srm_token;
use serum_dex::state::MarketState;
use serum_dex::state::ToAlignedBytes;


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


pub struct TestQuoteMint {
    pub pubkey: Pubkey,
    pub authority: Keypair,
    pub decimals: u8,
}


pub fn add_mint(test: &mut ProgramTest, decimals: u8) -> TestQuoteMint {
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
    TestQuoteMint {
        pubkey,
        authority,
        decimals,
    }
}

pub fn add_mint_srm(test: &mut ProgramTest) -> TestQuoteMint {
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
    TestQuoteMint {
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
    let mut acc = Account::new(u32::MAX as u64, size_of::<MarketState>(), &dex_prog_id);
    let ms = MarketState {
        account_flags: 0,
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
    acc.data = transmute_one_to_bytes(&ms).to_vec();
    test.add_account(pubkey, acc);
    TestDex { pubkey }
}

pub struct TestTokenAccount {
    pub pubkey: Pubkey,
}

pub fn add_token_account(test: &mut ProgramTest, owner: Pubkey, mint: Pubkey) -> TestTokenAccount {
    let pubkey = Pubkey::new_unique();
    test.add_packable_account(
        pubkey,
        u32::MAX as u64,
        &Token {
            mint: mint,
            owner: owner,
            amount: 0,
            state: AccountState::Initialized,
            ..Token::default()
        },
        &spl_token::id(),
    );
    TestTokenAccount { pubkey }
}


// pub struct TestDex {
//     pub pubkey: Pubkey,
// }

// pub fn add_dex(test: &mut ProgramTest, base: Pubkey, quote: Pubkey) -> TestDex {
//     let pubkey = Pubkey::new_unique();

// }

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
