use std::cell::{Ref, RefMut};
use std::convert::identity;
use std::mem::size_of;

use bytemuck::{cast_slice, cast_slice_mut, from_bytes, from_bytes_mut, Pod, try_from_bytes, try_from_bytes_mut, Zeroable};
use enumflags2::BitFlags;
use fixed::types::U64F64;
use serum_dex::error::DexResult;
use serum_dex::state::ToAlignedBytes;
use solana_program::account_info::AccountInfo;
use solana_program::clock::Clock;
use solana_program::entrypoint::ProgramResult;
use solana_program::msg;
use solana_program::program_error::ProgramError;
use solana_program::pubkey::Pubkey;

/// Initially launching with BTC/USDC, ETH/USDC, SRM/USDC
pub const NUM_TOKENS: usize = 3;
pub const NUM_MARKETS: usize = NUM_TOKENS - 1;
pub const MAX_LEVERAGE: u64 = 5;
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

    /// interest is in units per second (e.g. 0.01 => 1% interest per second)
    pub fn get_interest_rate(&self, token_index: usize) -> U64F64 {
        if self.total_borrows[token_index] == 0 {
            U64F64::from_num(0)
        } else {
            U64F64::from_num(0.01) / U64F64::from_num(YEAR)  // 1% interest per year
        }
    }

    pub fn update_indexes(&mut self, clock: &Clock) -> ProgramResult {
        let curr_ts = clock.unix_timestamp as u64;
        let fee_adj = U64F64::from_num(19) / U64F64::from_num(20);

        for i in 0..NUM_TOKENS {
            let interest_rate = self.get_interest_rate(i);

            let index: &mut MangoIndex = &mut self.indexes[i];

            if index.last_update == curr_ts {
                continue;
            }

            let native_deposits = self.total_deposits[i] * index.deposit;
            let native_borrows = self.total_borrows[i] * index.borrow;

            assert!(native_deposits > 0);
            assert!(native_borrows <= native_deposits);

            let utilization: U64F64 = native_borrows / native_deposits;
            let borrow_interest = interest_rate * U64F64::from_num(curr_ts - index.last_update);
            let deposit_interest = interest_rate * fee_adj * utilization;
            index.last_update = curr_ts;
            index.borrow += index.borrow * borrow_interest;
            index.deposit += index.deposit * deposit_interest;
        }
        Ok(())
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

impl MarginAccount {
    pub fn get_free_equity(
        &self,
        mango_group: &MangoGroup,
        prices: &[U64F64; NUM_TOKENS],
        open_orders_accs: &[AccountInfo; NUM_MARKETS]
    ) -> Result<U64F64, ProgramError> {
        // TODO weight collateral differently

        let mut assets: U64F64 = U64F64::from_num(0);
        let mut liabs: U64F64 = U64F64::from_num(0);

        /*
            Determine quote currency value of deposits
            Determine quote currency value of borrows
            Determine value of positions
            Add back value locked in open_orders

            equity = val(deposits) - val(borrows) + val(positions) + val(open_orders)

         */


        for i in 0..NUM_MARKETS {
            // TODO check open orders details
            let open_orders = load_open_orders(&open_orders_accs[i])?;
            assets += U64F64::from_num(open_orders.native_coin_total) * prices[i]
                + U64F64::from_num(open_orders.native_pc_total);
        }
        for i in 0..NUM_TOKENS {
            let index: &MangoIndex = &mango_group.indexes[i];
            let native_deposits = index.deposit * self.deposits[i];
            let native_borrows = index.borrow * self.borrows[i];

            assets += (native_deposits + U64F64::from_num(self.positions[i])) * prices[i];
            liabs += native_borrows * prices[i];
        }

        if liabs > assets {
            msg!("This account should be liquidated!");
            Ok(U64F64::from_num(0))
        } else {
            let locked_equity = liabs / U64F64::from_num(MAX_LEVERAGE);
            let equity = assets - liabs;
            if equity < locked_equity {
                Ok(U64F64::from_num(0))
            } else {
                Ok(equity - locked_equity)
            }
        }
    }
}


#[derive(Copy, Clone)]
#[repr(packed)]
pub struct OrderBookStateHeader {
    pub account_flags: u64, // Initialized, (Bids or Asks)
}
unsafe impl Zeroable for OrderBookStateHeader {}
unsafe impl Pod for OrderBookStateHeader {}


#[inline]
#[allow(dead_code)]
fn remove_slop<T: Pod>(bytes: &[u8]) -> &[T] {
    let slop = bytes.len() % size_of::<T>();
    let new_len = bytes.len() - slop;
    cast_slice(&bytes[..new_len])
}


#[inline]
#[allow(dead_code)]
fn remove_slop_mut<T: Pod>(bytes: &mut [u8]) -> &mut [T] {
    let slop = bytes.len() % size_of::<T>();
    let new_len = bytes.len() - slop;
    cast_slice_mut(&mut bytes[..new_len])
}

#[allow(dead_code)]
fn strip_header<'a, H: Pod, D: Pod>(
    account: &'a AccountInfo
) -> Result<(Ref<'a, H>, Ref<'a, [D]>), ProgramError> {
    let (header, inner): (Ref<'a, [H]>, Ref<'a, [D]>) =
        Ref::map_split(account.try_borrow_data()?, |raw_data| {

            let data: &[u8] = *raw_data;
            let (header_bytes, inner_bytes) = data.split_at(size_of::<H>());
            let header: &H;
            let inner: &[D];
            header = try_from_bytes(header_bytes).unwrap();

            inner = remove_slop(inner_bytes);

            (std::slice::from_ref(header), inner)
        });

    let header = Ref::map(header, |s| s.first().unwrap_or_else(|| unreachable!()));
    Ok((header, inner))
}

#[allow(dead_code)]
fn strip_header_mut<'a, H: Pod, D: Pod>(
    account: &'a AccountInfo
) -> Result<(RefMut<'a, H>, RefMut<'a, [D]>), ProgramError> {
    let (header, inner): (RefMut<'a, [H]>, RefMut<'a, [D]>) =
        RefMut::map_split(account.try_borrow_mut_data()?, |raw_data| {

            let data: &mut [u8] = *raw_data;
            let (header_bytes, inner_bytes) = data.split_at_mut(size_of::<H>());
            let header: &mut H;
            let inner: &mut [D];
            header = try_from_bytes_mut(header_bytes).unwrap();

            inner = remove_slop_mut(inner_bytes);

            (std::slice::from_mut(header), inner)
        });

    let header = RefMut::map(header, |s| s.first_mut().unwrap_or_else(|| unreachable!()));
    Ok((header, inner))
}


fn strip_data_header_mut<'a, H: Pod, D: Pod>(
    orig_data: RefMut<'a, [u8]>,
) -> Result<(RefMut<'a, H>, RefMut<'a, [D]>), ProgramError> {
    let (header, inner): (RefMut<'a, [H]>, RefMut<'a, [D]>) =
        RefMut::map_split(orig_data, |data| {

            let (header_bytes, inner_bytes) = data.split_at_mut(size_of::<H>());
            let header: &mut H;
            let inner: &mut [D];
            header = try_from_bytes_mut(header_bytes).unwrap();
            inner = remove_slop_mut(inner_bytes);
            (std::slice::from_mut(header), inner)
        });
    let header = RefMut::map(header, |s| s.first_mut().unwrap_or_else(|| unreachable!()));
    Ok((header, inner))
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

fn strip_dex_padding_mut<'a>(acc: &'a AccountInfo) -> Result<RefMut<'a, [u8]>, ProgramError> {
    assert!(acc.data_len() >= 12);
    let unpadded_data: RefMut<[u8]> = RefMut::map(acc.try_borrow_mut_data()?, |data| {
        let data_len = data.len() - 12;
        let (_, rest) = data.split_at_mut(5);
        let (mid, _) = rest.split_at_mut(data_len);
        mid
    });
    Ok(unpadded_data)
}



pub fn load_bids_mut<'a>(
    sm: &RefMut<serum_dex::state::MarketState>,
    bids: &'a AccountInfo
) -> DexResult<RefMut<'a, serum_dex::critbit::Slab>> {
    assert_eq!(&bids.key.to_aligned_bytes(), &identity(sm.bids));

    let orig_data = strip_dex_padding_mut(bids)?;
    let (header, buf) = strip_data_header_mut::<OrderBookStateHeader, u8>(orig_data)?;
    let flags = BitFlags::from_bits(header.account_flags).unwrap();
    assert!(&flags == &(serum_dex::state::AccountFlag::Initialized | serum_dex::state::AccountFlag::Bids));
    Ok(RefMut::map(buf, serum_dex::critbit::Slab::new))
}

pub fn load_asks_mut<'a>(
    sm: &RefMut<serum_dex::state::MarketState>,
    asks: &'a AccountInfo
) -> Result<RefMut<'a, serum_dex::critbit::Slab>, ProgramError> {
    assert_eq!(&asks.key.to_aligned_bytes(), &identity(sm.asks));
    let orig_data = strip_dex_padding_mut(asks)?;
    let (header, buf) = strip_data_header_mut::<OrderBookStateHeader, u8>(orig_data)?;
    let flags = BitFlags::from_bits(header.account_flags).unwrap();
    assert!(&flags == &(serum_dex::state::AccountFlag::Initialized | serum_dex::state::AccountFlag::Asks));
    Ok(RefMut::map(buf, serum_dex::critbit::Slab::new))
}

pub fn load_open_orders<'a>(acc: &'a AccountInfo) -> Result<Ref<'a, serum_dex::state::OpenOrders>, ProgramError> {
    Ok(Ref::map(strip_dex_padding(acc)?, from_bytes))
}