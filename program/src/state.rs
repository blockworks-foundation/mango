use std::cell::{Ref, RefMut};
use std::convert::identity;
use std::mem::size_of;

use bytemuck::{cast_slice, cast_slice_mut, from_bytes, from_bytes_mut, Pod, try_from_bytes, try_from_bytes_mut, Zeroable};
use enumflags2::BitFlags;
use fixed::types::U64F64;
use serum_dex::state::ToAlignedBytes;
use solana_program::account_info::AccountInfo;
use solana_program::clock::Clock;
use solana_program::msg;
use solana_program::program_error::ProgramError;
use solana_program::pubkey::Pubkey;

use crate::error::{check_assert, MangoResult, SourceFileId};

/// Initially launching with BTC/USDC, ETH/USDC, SRM/USDC
pub const NUM_TOKENS: usize = 3;
pub const NUM_MARKETS: usize = NUM_TOKENS - 1;
pub const MINUTE: u64 = 60;
pub const HOUR: u64 = 3600;
pub const DAY: u64 = 86400;
pub const YEAR: u64 = 31536000;

macro_rules! prog_assert {
    ($cond:expr) => {
        check_assert($cond, line!() as u16, SourceFileId::State)
    }
}
macro_rules! prog_assert_eq {
    ($x:expr, $y:expr) => {
        check_assert($x == $y, line!() as u16, SourceFileId::State)
    }
}

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
    pub total_borrows: [U64F64; NUM_TOKENS],

    pub maint_coll_ratio: U64F64,  // 1.10
    pub init_coll_ratio: U64F64  //  1.20
}
impl_loadable!(MangoGroup);

impl MangoGroup {
    pub fn load_mut_checked<'a>(
        account: &'a AccountInfo,
        program_id: &Pubkey
    ) -> MangoResult<RefMut<'a, Self>> {

        prog_assert_eq!(account.data_len(), size_of::<Self>())?;
        prog_assert_eq!(account.owner, program_id)?;

        let mango_group = Self::load_mut(account)?;
        prog_assert_eq!(mango_group.account_flags, (AccountFlag::Initialized | AccountFlag::MangoGroup).bits())?;

        Ok(mango_group)
    }
    #[allow(dead_code)]
    fn load_checked<'a>(
        account: &'a AccountInfo,
        program_id: &Pubkey
    ) -> MangoResult<Ref<'a, Self>> {
        prog_assert_eq!(account.data_len(), size_of::<Self>())?;
        prog_assert_eq!(account.owner, program_id)?;

        let mango_group = Self::load(account)?;
        prog_assert_eq!(mango_group.account_flags, (AccountFlag::Initialized | AccountFlag::MangoGroup).bits())?;

        Ok(mango_group)
    }
    pub fn get_token_index(&self, mint_pk: &Pubkey) -> Option<usize> {
        self.tokens.iter().position(|token| token == mint_pk)
    }

    /// interest is in units per second (e.g. 0.01 => 1% interest per second)
    pub fn get_interest_rate(&self, token_index: usize) -> U64F64 {

        let optimal_util = U64F64::from_num(0.7);
        let optimal_r = U64F64::from_num(0.10) / U64F64::from_num(YEAR);  // opt 10%
        let max_r = U64F64::from_num(1) / U64F64::from_num(YEAR);  // max 100%
        let index: &MangoIndex = &self.indexes[token_index];
        let native_deposits = index.deposit * self.total_deposits[token_index];
        let native_borrows = index.borrow * self.total_borrows[token_index];
        if native_deposits < native_borrows || native_deposits == 0 {
            return max_r;  // kind of an error state
        }
        let utilization = native_borrows / native_deposits;

        if utilization > optimal_util {
            let extra_util = utilization - optimal_util;
            let slope = (max_r - optimal_r) / (U64F64::from_num(1) - optimal_util);
            optimal_r + slope * extra_util
        } else {
            let slope = optimal_r / optimal_util;
            slope * utilization
        }
    }

    pub fn update_indexes(&mut self, clock: &Clock) -> MangoResult<()> {
        let curr_ts = clock.unix_timestamp as u64;
        let fee_adj = U64F64::from_num(19) / U64F64::from_num(20);

        for i in 0..NUM_TOKENS {

            let interest_rate = self.get_interest_rate(i);
            let index: &mut MangoIndex = &mut self.indexes[i];
            if index.last_update == curr_ts || self.total_deposits[i] == 0 {
                continue;
            }

            let native_deposits = self.total_deposits[i] * index.deposit;
            let native_borrows = self.total_borrows[i] * index.borrow;
            prog_assert!(native_borrows <= native_deposits)?;

            let utilization: U64F64 = native_borrows / native_deposits;
            let borrow_interest = interest_rate * U64F64::from_num(curr_ts - index.last_update);
            let deposit_interest = interest_rate * fee_adj * utilization;

            index.last_update = curr_ts;
            index.borrow += index.borrow * borrow_interest;
            index.deposit += index.deposit * deposit_interest;

        }
        Ok(())
    }

    pub fn get_total_borrows_native(&self, token_i: usize) -> u64 {
        let native: U64F64 = self.total_borrows[token_i] * self.indexes[token_i].borrow;
        native.checked_ceil().unwrap().to_num()  // rounds toward +inf
    }
    pub fn get_total_deposits_native(&self, token_i: usize) -> u64 {
        let native: U64F64 = self.total_deposits[token_i] * self.indexes[token_i].deposit;
        native.checked_floor().unwrap().to_num()  // rounds toward -inf
    }
    pub fn get_market_index(&self, spot_market_pk: &Pubkey) -> Option<usize> {
        self.spot_markets.iter().position(|market| market == spot_market_pk)
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
    pub fn load_mut_checked<'a>(
        account: &'a AccountInfo,
        mango_group_pk: &Pubkey
    ) -> MangoResult<RefMut<'a, Self>> {
        prog_assert_eq!(account.data_len(), size_of::<MarginAccount>())?;

        let margin_account = Self::load_mut(account)?;
        prog_assert_eq!(margin_account.account_flags, (AccountFlag::Initialized | AccountFlag::MarginAccount).bits())?;
        // prog_assert_eq!(&margin_account.owner, owner_pk)?; // not necessary
        prog_assert_eq!(&margin_account.mango_group, mango_group_pk)?;

        Ok(margin_account)
    }
    pub fn load_checked<'a>(
        account: &'a AccountInfo,
        mango_group_pk: &Pubkey
    ) -> MangoResult<Ref<'a, Self>> {
        prog_assert_eq!(account.data_len(), size_of::<MarginAccount>())?;

        let margin_account = Self::load(account)?;
        prog_assert_eq!(margin_account.account_flags, (AccountFlag::Initialized | AccountFlag::MarginAccount).bits())?;
        // prog_assert_eq!(&margin_account.owner, owner_pk)?;  // not necessary
        prog_assert_eq!(&margin_account.mango_group, mango_group_pk)?;

        Ok(margin_account)
    }
    pub fn get_equity(
        &self,
        mango_group: &MangoGroup,
        prices: &[U64F64; NUM_TOKENS],
        open_orders_accs: &[AccountInfo; NUM_MARKETS]
    ) -> MangoResult<U64F64> {
        // TODO weight collateral differently
        // equity = val(deposits) + val(positions) + val(open_orders) - val(borrows)
        let assets = self.get_assets_val(mango_group, prices, open_orders_accs)?;
        let liabs = self.get_liabs_val(mango_group, prices)?;
        if liabs > assets {
            msg!("This account should be liquidated!");
            Ok(U64F64::from_num(0))
        } else {
            Ok(assets - liabs)
        }
    }
    pub fn get_free_equity(
        &self,
        mango_group: &MangoGroup,
        prices: &[U64F64; NUM_TOKENS],
        open_orders_accs: &[AccountInfo; NUM_MARKETS]
    ) -> MangoResult<U64F64> {
        let liabs = self.get_liabs_val(mango_group, prices)?;
        let assets = self.get_assets_val(mango_group, prices, open_orders_accs)?;
        if liabs > assets {
            msg!("This account should be liquidated!");
            Ok(U64F64::from_num(0))
        } else {
            let locked_assets = liabs * mango_group.init_coll_ratio;
            if assets < locked_assets {
                Ok(U64F64::from_num(0))
            } else {
                Ok(assets - locked_assets)
            }
        }
    }
    pub fn get_collateral_ratio(
        &self,
        mango_group: &MangoGroup,
        prices: &[U64F64; NUM_TOKENS],
        open_orders_accs: &[AccountInfo; NUM_MARKETS]
    ) -> MangoResult<U64F64> {
        // assets / liabs
        let assets = self.get_assets_val(mango_group, prices, open_orders_accs)?;
        let liabs = self.get_liabs_val(mango_group, prices)?;
        if liabs == U64F64::from_num(0) {
            Ok(U64F64::MAX)
        } else {
            Ok(assets / liabs)
        }
    }
    pub fn get_assets_val(
        &self,
        mango_group: &MangoGroup,
        prices: &[U64F64; NUM_TOKENS],
        open_orders_accs: &[AccountInfo; NUM_MARKETS]
    ) -> MangoResult<U64F64> {
        // TODO weight collateral differently
        // equity = val(deposits) + val(positions) + val(open_orders) - val(borrows)
        let mut assets: U64F64 = U64F64::from_num(0);
        for i in 0..NUM_MARKETS {
            // TODO check open orders details
            let open_orders = load_open_orders(&open_orders_accs[i])?;
            assets += U64F64::from_num(open_orders.native_coin_total) * prices[i]
                + U64F64::from_num(open_orders.native_pc_total);
        }
        for i in 0..NUM_TOKENS {
            let index: &MangoIndex = &mango_group.indexes[i];
            let native_deposits = index.deposit * self.deposits[i];
            assets += (native_deposits + U64F64::from_num(self.positions[i])) * prices[i];
        }
        Ok(assets)

    }
    pub fn get_liabs_val(
        &self,
        mango_group: &MangoGroup,
        prices: &[U64F64; NUM_TOKENS],
    ) -> MangoResult<U64F64> {
        let mut liabs: U64F64 = U64F64::from_num(0);
        for i in 0..NUM_TOKENS {
            let index: &MangoIndex = &mango_group.indexes[i];
            let native_borrows = index.borrow * self.borrows[i];
            liabs += native_borrows * prices[i];
        }
        Ok(liabs)
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
) -> MangoResult<(Ref<'a, H>, Ref<'a, [D]>)> {
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
) -> MangoResult<(RefMut<'a, H>, RefMut<'a, [D]>)> {
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
) -> MangoResult<(RefMut<'a, H>, RefMut<'a, [D]>)> {
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


fn strip_dex_padding<'a>(acc: &'a AccountInfo) -> MangoResult<Ref<'a, [u8]>> {
    prog_assert!(acc.data_len() >= 12)?;
    let unpadded_data: Ref<[u8]> = Ref::map(acc.try_borrow_data()?, |data| {
        let data_len = data.len() - 12;
        let (_, rest) = data.split_at(5);
        let (mid, _) = rest.split_at(data_len);
        mid
    });
    Ok(unpadded_data)
}

fn strip_dex_padding_mut<'a>(acc: &'a AccountInfo) -> MangoResult<RefMut<'a, [u8]>> {
    prog_assert!(acc.data_len() >= 12)?;
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
) -> MangoResult<RefMut<'a, serum_dex::critbit::Slab>> {
    prog_assert_eq!(&bids.key.to_aligned_bytes(), &identity(sm.bids))?;

    let orig_data = strip_dex_padding_mut(bids)?;
    let (header, buf) = strip_data_header_mut::<OrderBookStateHeader, u8>(orig_data)?;
    let flags = BitFlags::from_bits(header.account_flags).unwrap();
    prog_assert!(&flags == &(serum_dex::state::AccountFlag::Initialized | serum_dex::state::AccountFlag::Bids))?;
    Ok(RefMut::map(buf, serum_dex::critbit::Slab::new))
}

pub fn load_asks_mut<'a>(
    sm: &RefMut<serum_dex::state::MarketState>,
    asks: &'a AccountInfo
) -> MangoResult<RefMut<'a, serum_dex::critbit::Slab>> {
    prog_assert_eq!(&asks.key.to_aligned_bytes(), &identity(sm.asks))?;
    let orig_data = strip_dex_padding_mut(asks)?;
    let (header, buf) = strip_data_header_mut::<OrderBookStateHeader, u8>(orig_data)?;
    let flags = BitFlags::from_bits(header.account_flags).unwrap();
    prog_assert!(&flags == &(serum_dex::state::AccountFlag::Initialized | serum_dex::state::AccountFlag::Asks))?;
    Ok(RefMut::map(buf, serum_dex::critbit::Slab::new))
}

pub fn load_open_orders<'a>(
    acc: &'a AccountInfo
) -> Result<Ref<'a, serum_dex::state::OpenOrders>, ProgramError> {
    Ok(Ref::map(strip_dex_padding(acc)?, from_bytes))
}

pub fn load_market_state<'a>(
    market_account: &'a AccountInfo,
    program_id: &Pubkey,
) -> MangoResult<RefMut<'a, serum_dex::state::MarketState>> {
    prog_assert_eq!(market_account.owner, program_id)?;

    let state: RefMut<'a, serum_dex::state::MarketState>;
    state = RefMut::map(market_account.try_borrow_mut_data()?, |data| {
        let data_len = data.len() - 12;
        let (_, rest) = data.split_at_mut(5);
        let (mid, _) = rest.split_at_mut(data_len);
        from_bytes_mut(mid)
    });

    state.check_flags()?;
    Ok(state)

}
