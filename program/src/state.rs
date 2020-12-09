use std::cell::{Ref, RefMut};

use solana_program::account_info::AccountInfo;
use solana_program::program_error::ProgramError;

use bytemuck::{from_bytes, from_bytes_mut, Pod, Zeroable};
use solana_program::pubkey::Pubkey;
use enumflags2::BitFlags;
use fixed::types::U64F64;


/// Initially launching with BTC/USDC, ETH/USDC, SRM/USDC
pub const NUM_TOKENS: usize = 3;
pub const NUM_MARKETS: usize = NUM_TOKENS - 1;
pub const MINUTE: u64 = 60;
pub const HOUR: u64 = 3600;
pub const DAY: u64 = 86400;
pub const YEAR: u64 = 31536000;


pub trait Loadable: Pod {
    fn load_mut<'a>(account: &'a AccountInfo) -> Result<RefMut<'a, Self>, ProgramError> {
        Ok(RefMut::map(account.try_borrow_mut_data()?, |data| from_bytes_mut(data)))
    }
    fn load<'a>(account: &'a AccountInfo) -> Result<Ref<'a, Self>, ProgramError> {
        Ok(Ref::map(account.try_borrow_data()?, |data| from_bytes(data)))
    }

    fn load_from_bytes(data: &[u8]) -> Result<&Self, ProgramError> {
        Ok(from_bytes(data))
    }
}

macro_rules! impl_loadable {
    ($type_name:ident) => {
        unsafe impl Zeroable for $type_name {}
        unsafe impl Pod for $type_name {}
        impl Loadable for $type_name {}
    }
}


#[derive(Copy, Clone, BitFlags, Debug, Eq, PartialEq)]
#[repr(u64)]
pub enum AccountFlag {
    Initialized = 1u64 << 0,
    MangoGroup = 1u64 << 1,
    MarginAccount = 1u64 << 2,
}


#[derive(Copy, Clone)]
#[repr(C)]
pub struct MangoIndex {
    pub last_update: u64,
    pub borrow: U64F64,
    pub deposit: U64F64
}
unsafe impl Zeroable for MangoIndex {}
unsafe impl Pod for MangoIndex {}

/// A group of spot markets that can be cross margined together
/// TODO need plans to migrate smart contract
/// TODO add in fees for devs and UI hosters
#[derive(Copy, Clone)]
#[repr(C)]
pub struct MangoGroup {
    pub account_flags: u64,
    pub tokens: [Pubkey; NUM_TOKENS],  // Last token is shared quote currency
    pub vaults: [Pubkey; NUM_TOKENS],  // where funds are stored
    pub indexes: [MangoIndex; NUM_TOKENS],  // to keep track of interest
    pub spot_markets: [Pubkey; NUM_MARKETS],  // pubkeys to MarketState of serum dex
    pub signer_nonce: u64,
    pub signer_key: Pubkey,
    pub dex_program_id: Pubkey,  // serum dex program id

    // denominated in Mango index adjusted terms
    pub total_deposits: [U64F64; NUM_TOKENS],
    pub total_borrows: [U64F64; NUM_TOKENS]
}
impl_loadable!(MangoGroup);

impl MangoGroup {
    pub fn get_token_index(&self, mint_pk: &Pubkey) -> Option<usize> {
        self.tokens.iter().position(|token| token == mint_pk)
    }

    pub fn get_interest_rate(&self, token_index: usize) -> U64F64 {
        if self.total_borrows[token_index] == 0 {
            U64F64::from_num(0)
        } else {
            U64F64::from_num(0.01) / U64F64::from_num(YEAR)  // 1% interest per year
        }
    }
}



// Track the issuances of bonds by this user
#[derive(Copy, Clone)]
#[repr(C)]
pub struct MarginAccount {
    pub account_flags: u64,
    pub mango_group: Pubkey,
    pub owner: Pubkey,  // solana pubkey of owner

    // assets and borrows are denominated in Mango adjusted terms
    pub deposits: [U64F64; NUM_TOKENS],  // assets being lent out and gaining interest, including collateral

    // this will be incremented every time an order is opened and decremented when order is closed
    pub borrows: [U64F64; NUM_TOKENS],  // multiply by current index to get actual value

    pub positions: [u64; NUM_TOKENS],  // the positions held by the user
    pub open_orders: [Pubkey; NUM_MARKETS],  // owned by Mango

}
impl_loadable!(MarginAccount);

