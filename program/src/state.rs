use std::cell::{Ref, RefMut};
use std::convert::identity;
use std::mem::size_of;

use bytemuck::{cast_slice, cast_slice_mut, from_bytes, from_bytes_mut, Pod, try_from_bytes, try_from_bytes_mut, Zeroable};
use enumflags2::BitFlags;
use fixed::types::U64F64;
use serum_dex::state::ToAlignedBytes;
use solana_program::account_info::AccountInfo;
use solana_program::clock::Clock;
use solana_program::program_error::ProgramError;
use solana_program::pubkey::Pubkey;

use fixed_macro::types::U64F64;

use crate::error::{check_assert, MangoResult, SourceFileId, MangoErrorCode, MangoError};

/// Initially launching with BTC/USDT, ETH/USDT
pub const NUM_TOKENS: usize = 5;
pub const NUM_MARKETS: usize = NUM_TOKENS - 1;
pub const MANGO_GROUP_PADDING: usize = 8 - (NUM_TOKENS + NUM_MARKETS) % 8;
pub const MINUTE: u64 = 60;
pub const HOUR: u64 = 3600;
pub const DAY: u64 = 86400;
pub const YEAR: U64F64 = U64F64!(31536000);
const OPTIMAL_UTIL: U64F64 = U64F64!(0.7);
const OPTIMAL_R: U64F64 = U64F64!(1.9025875190258751902587e-09);  // 6% APR -> 0.06 / YEAR
const MAX_R: U64F64 = U64F64!(4.7564687975646879756468e-08); // max 150% APR -> 2 / YEAR

pub const ONE_U64F64: U64F64 = U64F64!(1);
pub const ZERO_U64F64: U64F64 = U64F64!(0);
pub const PARTIAL_LIQ_INCENTIVE: U64F64 = U64F64!(1.05);
pub const DUST_THRESHOLD: U64F64 = U64F64!(1);  // TODO make this part of MangoGroup state
pub const EPSILON: U64F64 = U64F64!(1.0e-17);
pub const INFO_LEN: usize = 32;


macro_rules! check_default {
    ($cond:expr) => {
        check_assert($cond, MangoErrorCode::Default, line!(), SourceFileId::State)
    }
}
macro_rules! check_eq_default {
    ($x:expr, $y:expr) => {
        check_assert($x == $y, MangoErrorCode::Default, line!(), SourceFileId::State)
    }
}

macro_rules! throw {
    () => {
        MangoError::MangoErrorCode {
            mango_error_code: MangoErrorCode::Default,
            line: line!(),
            source_file_id: SourceFileId::State
        }
    }
}


pub trait Loadable: Pod {
    fn load_mut<'a>(account: &'a AccountInfo) -> Result<RefMut<'a, Self>, ProgramError> {
        // TODO verify if this checks for size
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
    MangoSrmAccount = 1u64 << 3
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
    pub oracles: [Pubkey; NUM_MARKETS],  // oracles that give price of each base currency in quote currency
    pub signer_nonce: u64,
    pub signer_key: Pubkey,
    pub dex_program_id: Pubkey,  // serum dex program id

    // denominated in Mango index adjusted terms
    pub total_deposits: [U64F64; NUM_TOKENS],
    pub total_borrows: [U64F64; NUM_TOKENS],

    pub maint_coll_ratio: U64F64,  // 1.10
    pub init_coll_ratio: U64F64,  //  1.20

    pub srm_vault: Pubkey,  // holds users SRM for fee reduction

    /// This admin key is only for alpha release and the only power it has is to amend borrow limits
    /// If users borrow too much too quickly before liquidators are able to handle the volume,
    /// lender funds will be at risk. Hence these borrow limits will be raised slowly
    /// UPDATE: 4/15/2021 - this admin key is now useless, borrow limits are removed
    pub admin: Pubkey,
    pub borrow_limits: [u64; NUM_TOKENS],

    pub mint_decimals: [u8; NUM_TOKENS],
    pub oracle_decimals: [u8; NUM_MARKETS],
    pub padding: [u8; MANGO_GROUP_PADDING]
}
impl_loadable!(MangoGroup);



impl MangoGroup {
    pub fn load_mut_checked<'a>(
        account: &'a AccountInfo,
        program_id: &Pubkey
    ) -> MangoResult<RefMut<'a, Self>> {

        check_eq_default!(account.data_len(), size_of::<Self>())?;
        check_eq_default!(account.owner, program_id)?;

        let mango_group = Self::load_mut(account)?;
        check_eq_default!(mango_group.account_flags, (AccountFlag::Initialized | AccountFlag::MangoGroup).bits())?;

        Ok(mango_group)
    }
    pub fn load_checked<'a>(
        account: &'a AccountInfo,
        program_id: &Pubkey
    ) -> MangoResult<Ref<'a, Self>> {
        check_eq_default!(account.data_len(), size_of::<Self>())?;  // TODO not necessary check
        check_eq_default!(account.owner, program_id)?;

        let mango_group = Self::load(account)?;
        check_eq_default!(mango_group.account_flags, (AccountFlag::Initialized | AccountFlag::MangoGroup).bits())?;

        Ok(mango_group)
    }
    pub fn get_token_index(&self, mint_pk: &Pubkey) -> Option<usize> {
        self.tokens.iter().position(|token| token == mint_pk)
    }
    pub fn get_token_index_with_vault(&self, vault: &Pubkey) -> Option<usize> {
        self.vaults.iter().position(|pk| pk == vault)
    }
    /// interest is in units per second (e.g. 0.01 => 1% interest per second)
    pub fn get_interest_rate(&self, token_index: usize) -> U64F64 {
        let index: &MangoIndex = &self.indexes[token_index];
        let native_deposits = index.deposit.checked_mul(self.total_deposits[token_index]).unwrap();
        let native_borrows = index.borrow.checked_mul(self.total_borrows[token_index]).unwrap();
        if native_deposits <= native_borrows {  // if deps == 0, this is always true
            return MAX_R;  // kind of an error state
        }

        let utilization = native_borrows.checked_div(native_deposits).unwrap();
        if utilization > OPTIMAL_UTIL {
            let extra_util = utilization - OPTIMAL_UTIL;
            let slope = (MAX_R - OPTIMAL_R) / (ONE_U64F64 - OPTIMAL_UTIL);
            OPTIMAL_R + slope * extra_util
        } else {
            let slope = OPTIMAL_R / OPTIMAL_UTIL;
            slope * utilization
        }
    }


    pub fn update_indexes(&mut self, clock: &Clock) -> MangoResult<()> {
        // TODO verify what happens if total_deposits < total_borrows
        // TODO verify what happens if total_deposits == 0 && total_borrows > 0
        // TODO What are cases where borrows is greater than deposits?
        // TODO total_borrows may be greater than total_deposits if rounding error

        let curr_ts = clock.unix_timestamp as u64;

        for i in 0..NUM_TOKENS {
            let interest_rate = self.get_interest_rate(i);
            let index: &mut MangoIndex = &mut self.indexes[i];
            if index.last_update == curr_ts || self.total_deposits[i] == ZERO_U64F64 {
                // TODO is skipping when total_deposits == 0 correct move?
                continue;
            }

            // don't need to check here because this check already happens in get interest rate
            let native_deposits: U64F64 = self.total_deposits[i].checked_mul(index.deposit).unwrap();
            let native_borrows: U64F64 = self.total_borrows[i].checked_mul(index.borrow).unwrap();
            check_default!(native_borrows <= native_deposits + EPSILON)?;  // to account for rounding errors

            let utilization = native_borrows.checked_div(native_deposits).unwrap();
            let borrow_interest = interest_rate
                .checked_mul(U64F64::from_num(curr_ts - index.last_update)).unwrap();

            let deposit_interest = borrow_interest
                .checked_mul(utilization).unwrap();

            index.last_update = curr_ts;
            index.borrow = index.borrow.checked_mul(borrow_interest).unwrap()
                .checked_add(index.borrow).unwrap();

            index.deposit = index.deposit.checked_mul(deposit_interest).unwrap()
                .checked_add(index.deposit).unwrap();
        }
        Ok(())
    }

    pub fn has_valid_deposits_borrows(&self, token_i: usize) -> bool {
        self.get_total_native_deposit(token_i) >= self.get_total_native_borrow(token_i)
    }

    pub fn get_total_native_borrow(&self, token_i: usize) -> u64 {
        let native: U64F64 = self.total_borrows[token_i] * self.indexes[token_i].borrow;
        native.checked_ceil().unwrap().to_num()  // rounds toward +inf
    }

    pub fn get_total_native_deposit(&self, token_i: usize) -> u64 {
        let native: U64F64 = self.total_deposits[token_i] * self.indexes[token_i].deposit;
        native.checked_floor().unwrap().to_num()  // rounds toward -inf
    }

    pub fn get_market_index(&self, spot_market_pk: &Pubkey) -> Option<usize> {
        self.spot_markets.iter().position(|market| market == spot_market_pk)
    }

    pub fn checked_add_borrow(&mut self, token_i: usize, v: U64F64) -> MangoResult<()> {
        Ok(self.total_borrows[token_i] = self.total_borrows[token_i].checked_add(v).ok_or(throw!())?)
    }

    pub fn checked_sub_borrow(&mut self, token_i: usize, v: U64F64) -> MangoResult<()> {
        Ok(self.total_borrows[token_i] = self.total_borrows[token_i].checked_sub(v).ok_or(throw!())?)
    }

    pub fn checked_add_deposit(&mut self, token_i: usize, v: U64F64) -> MangoResult<()> {
        Ok(self.total_deposits[token_i] = self.total_deposits[token_i].checked_add(v).ok_or(throw!())?)
    }

    pub fn checked_sub_deposit(&mut self, token_i: usize, v: U64F64) -> MangoResult<()> {
        Ok(self.total_deposits[token_i] = self.total_deposits[token_i].checked_sub(v).ok_or(throw!())?)
    }
}



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

    pub open_orders: [Pubkey; NUM_MARKETS],  // owned by Mango
    pub being_liquidated: bool,
    pub has_borrows: bool, // does the account have any open borrows? set by checked_add_borrow and checked_sub_borrow
    pub info: [u8; INFO_LEN],
    pub padding: [u8; 38] // padding for future expansion
}
impl_loadable!(MarginAccount);

impl MarginAccount {
    pub fn load_mut_checked<'a>(
        program_id: &Pubkey,
        account: &'a AccountInfo,
        mango_group_pk: &Pubkey
    ) -> MangoResult<RefMut<'a, Self>> {
        check_eq_default!(account.owner, program_id)?;  // this is probably not necessary
        check_eq_default!(account.data_len(), size_of::<MarginAccount>())?;

        let margin_account = Self::load_mut(account)?;
        check_eq_default!(margin_account.account_flags, (AccountFlag::Initialized | AccountFlag::MarginAccount).bits())?;
        // prog_assert_eq!(&margin_account.owner, owner_pk)?; // not necessary
        check_eq_default!(&margin_account.mango_group, mango_group_pk)?;

        Ok(margin_account)
    }
    pub fn load_checked<'a>(
        program_id: &Pubkey,
        account: &'a AccountInfo,
        mango_group_pk: &Pubkey
    ) -> MangoResult<Ref<'a, Self>> {
        check_eq_default!(account.owner, program_id)?;  // This is probably not necessary
        check_eq_default!(account.data_len(), size_of::<MarginAccount>())?;

        let margin_account = Self::load(account)?;
        check_eq_default!(margin_account.account_flags, (AccountFlag::Initialized | AccountFlag::MarginAccount).bits())?;
        // prog_assert_eq!(&margin_account.owner, owner_pk)?;  // not necessary
        check_eq_default!(&margin_account.mango_group, mango_group_pk)?;

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
            Ok(ZERO_U64F64)
        } else {
            Ok(assets - liabs)
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
        if liabs == ZERO_U64F64 {
            Ok(U64F64::MAX)
        } else {
            Ok(assets.checked_div( liabs).unwrap())
        }
    }

    pub fn coll_ratio_from_assets_liabs(
        &self,
        prices: &[U64F64; NUM_TOKENS],
        assets: &[U64F64; NUM_TOKENS],
        liabs: &[U64F64; NUM_TOKENS]
    ) -> MangoResult<U64F64> {
        let mut assets_val: U64F64 = ZERO_U64F64;
        let mut liabs_val: U64F64 = ZERO_U64F64;
        for i in 0..NUM_TOKENS {
            liabs_val = liabs[i].checked_mul(prices[i]).unwrap().checked_add(liabs_val).unwrap();
            assets_val = assets[i].checked_mul(prices[i]).unwrap().checked_add(assets_val).unwrap();
        }

        if liabs_val == ZERO_U64F64 {
            Ok(U64F64::MAX)
        } else {
            Ok(assets_val.checked_div(liabs_val).unwrap())
        }
    }

    pub fn get_assets(
        &self,
        mango_group: &MangoGroup,
        open_orders_accs: &[AccountInfo; NUM_MARKETS]
    ) -> MangoResult<[U64F64; NUM_TOKENS]> {
        let mut assets = [ZERO_U64F64; NUM_TOKENS];

        for i in 0..NUM_TOKENS {
            assets[i] = mango_group.indexes[i].deposit.checked_mul(self.deposits[i]).unwrap()
                .checked_add(assets[i]).unwrap();
        }

        for i in 0..NUM_MARKETS {
            if *open_orders_accs[i].key == Pubkey::default() {
                continue;
            }
            let open_orders = load_open_orders(&open_orders_accs[i])?;

            assets[i] = U64F64::from_num(open_orders.native_coin_total).checked_add(assets[i]).unwrap();
            assets[NUM_TOKENS-1] = U64F64::from_num(open_orders.native_pc_total + open_orders.referrer_rebates_accrued).checked_add(assets[NUM_TOKENS-1]).unwrap();
        }
        Ok(assets)
    }


    pub fn get_liabs(
        &self,
        mango_group: &MangoGroup,
    ) -> MangoResult<[U64F64; NUM_TOKENS]> {
        let mut liabs = [ZERO_U64F64; NUM_TOKENS];

        for i in 0..NUM_TOKENS {
            liabs[i] = mango_group.indexes[i].borrow.checked_mul(self.borrows[i]).unwrap()
                .checked_add(liabs[i]).unwrap();
        }

        Ok(liabs)
    }


    pub fn get_assets_val(
        &self,
        mango_group: &MangoGroup,
        prices: &[U64F64; NUM_TOKENS],
        open_orders_accs: &[AccountInfo; NUM_MARKETS]
    ) -> MangoResult<U64F64> {
        // TODO weight collateral differently
        // equity = val(deposits) + val(positions) + val(open_orders) - val(borrows)
        let mut assets: U64F64 = ZERO_U64F64;
        for i in 0..NUM_MARKETS {  // Add up all the value in open orders
            // TODO check open orders details
            if *open_orders_accs[i].key == Pubkey::default() {
                continue;
            }

            let open_orders = load_open_orders(&open_orders_accs[i])?;
            assets = U64F64::from_num(open_orders.native_coin_total)
                .checked_mul(prices[i]).unwrap()
                .checked_add(U64F64::from_num(open_orders.native_pc_total + open_orders.referrer_rebates_accrued)).unwrap()
                .checked_add(assets).unwrap();
        }
        for i in 0..NUM_TOKENS {  // add up the value in margin account deposits and positions
            let index: &MangoIndex = &mango_group.indexes[i];
            let native_deposits = index.deposit.checked_mul(self.deposits[i]).unwrap();
            assets = native_deposits
                .checked_mul(prices[i]).unwrap()
                .checked_add(assets).unwrap()
        }
        Ok(assets)

    }


    pub fn get_liabs_val(
        &self,
        mango_group: &MangoGroup,
        prices: &[U64F64; NUM_TOKENS],
    ) -> MangoResult<U64F64> {
        let mut liabs: U64F64 = ZERO_U64F64;
        for i in 0..NUM_TOKENS {
            let index: &MangoIndex = &mango_group.indexes[i];
            let native_borrows = index.borrow * self.borrows[i];
            liabs += native_borrows * prices[i];
        }
        Ok(liabs)
    }
    /// Return amount of quote currency to deposit to get account above init_coll_ratio

    pub fn get_collateral_deficit(
        &self,
        mango_group: &MangoGroup,
        prices: &[U64F64; NUM_TOKENS],
        open_orders_accs: &[AccountInfo; NUM_MARKETS]
    ) -> MangoResult<u64> {
        let assets = self.get_assets_val(mango_group, prices, open_orders_accs)?;
        let liabs = self.get_liabs_val(mango_group, prices)?;

        if liabs == ZERO_U64F64 || assets >= liabs * mango_group.init_coll_ratio {
            Ok(0)
        } else {
            Ok((liabs * mango_group.init_coll_ratio - assets).to_num())
        }
    }


    pub fn get_partial_liq_deficit(
        &self,
        mango_group: &MangoGroup,
        prices: &[U64F64; NUM_TOKENS],
        open_orders_accs: &[AccountInfo; NUM_MARKETS]
    ) -> MangoResult<U64F64> {
        let assets = self.get_assets_val(mango_group, prices, open_orders_accs)?;
        let liabs = self.get_liabs_val(mango_group, prices)?;

        if liabs == ZERO_U64F64 || assets >= liabs * mango_group.init_coll_ratio {
            Ok(ZERO_U64F64)
        } else {
            // TODO make this checked
            Ok((liabs * mango_group.init_coll_ratio - assets) / (mango_group.init_coll_ratio - PARTIAL_LIQ_INCENTIVE))
        }
    }

    pub fn get_native_borrow(&self, index: &MangoIndex, token_i: usize) -> u64 {
        (self.borrows[token_i] * index.borrow).to_num()
    }

    pub fn get_native_deposit(&self, index: &MangoIndex, token_i: usize) -> u64 {
        (self.deposits[token_i] * index.deposit).to_num()
    }

    pub fn checked_add_borrow(&mut self, token_i: usize, v: U64F64) -> MangoResult<()> {
        Ok(self.borrows[token_i] = self.borrows[token_i].checked_add(v).ok_or(throw!())?)
    }

    pub fn checked_sub_borrow(&mut self, token_i: usize, v: U64F64) -> MangoResult<()> {
        Ok(self.borrows[token_i] = self.borrows[token_i].checked_sub(v).ok_or(throw!())?)
    }

    pub fn checked_add_deposit(&mut self, token_i: usize, v: U64F64) -> MangoResult<()> {
        Ok(self.deposits[token_i] = self.deposits[token_i].checked_add(v).ok_or(throw!())?)
    }

    pub fn checked_sub_deposit(&mut self, token_i: usize, v: U64F64) -> MangoResult<()> {
        Ok(self.deposits[token_i] = self.deposits[token_i].checked_sub(v).ok_or(throw!())?)
    }
}

// The SRM contributed to the pool by this user
// These SRM are not at risk and have no effect on any margin calculations.
// Depositing srm is a strictly altruistic act with no upside and no downside
#[derive(Copy, Clone)]
#[repr(C)]
pub struct MangoSrmAccount {
    pub account_flags: u64,
    pub mango_group: Pubkey,
    pub owner: Pubkey,
    pub amount: u64
}
impl_loadable!(MangoSrmAccount);

impl MangoSrmAccount {
    pub fn load_mut_checked<'a>(
        program_id: &Pubkey,
        account: &'a AccountInfo,
        mango_group_pk: &Pubkey
    ) -> MangoResult<RefMut<'a, Self>> {
        check_eq_default!(account.owner, program_id)?;
        check_eq_default!(account.data_len(), size_of::<MangoSrmAccount>())?;
        let srm_account = Self::load_mut(account)?;
        check_eq_default!(srm_account.account_flags, (AccountFlag::Initialized | AccountFlag::MangoSrmAccount).bits())?;
        check_eq_default!(&srm_account.mango_group, mango_group_pk)?;

        Ok(srm_account)
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

#[allow(dead_code)]
fn strip_data_header<'a, H: Pod, D: Pod>(
    orig_data: Ref<'a, [u8]>,
) -> MangoResult<(Ref<'a, H>, Ref<'a, [D]>)> {
    let (header, inner): (Ref<'a, [H]>, Ref<'a, [D]>) =
        Ref::map_split(orig_data, |data| {

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

fn strip_dex_padding<'a>(acc: &'a AccountInfo) -> MangoResult<Ref<'a, [u8]>> {
    check_default!(acc.data_len() >= 12)?;
    let unpadded_data: Ref<[u8]> = Ref::map(acc.try_borrow_data()?, |data| {
        let data_len = data.len() - 12;
        let (_, rest) = data.split_at(5);
        let (mid, _) = rest.split_at(data_len);
        mid
    });
    Ok(unpadded_data)
}

fn strip_dex_padding_mut<'a>(acc: &'a AccountInfo) -> MangoResult<RefMut<'a, [u8]>> {
    check_default!(acc.data_len() >= 12)?;
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
    check_eq_default!(&bids.key.to_aligned_bytes(), &identity(sm.bids))?;

    let orig_data = strip_dex_padding_mut(bids)?;
    let (header, buf) = strip_data_header_mut::<OrderBookStateHeader, u8>(orig_data)?;
    let flags = BitFlags::from_bits(header.account_flags).unwrap();
    check_default!(&flags == &(serum_dex::state::AccountFlag::Initialized | serum_dex::state::AccountFlag::Bids))?;
    Ok(RefMut::map(buf, serum_dex::critbit::Slab::new))
}

pub fn load_asks_mut<'a>(
    sm: &RefMut<serum_dex::state::MarketState>,
    asks: &'a AccountInfo
) -> MangoResult<RefMut<'a, serum_dex::critbit::Slab>> {
    check_eq_default!(&asks.key.to_aligned_bytes(), &identity(sm.asks))?;
    let orig_data = strip_dex_padding_mut(asks)?;
    let (header, buf) = strip_data_header_mut::<OrderBookStateHeader, u8>(orig_data)?;
    let flags = BitFlags::from_bits(header.account_flags).unwrap();
    check_default!(&flags == &(serum_dex::state::AccountFlag::Initialized | serum_dex::state::AccountFlag::Asks))?;
    Ok(RefMut::map(buf, serum_dex::critbit::Slab::new))
}

pub fn load_open_orders<'a>(
    acc: &'a AccountInfo
) -> Result<Ref<'a, serum_dex::state::OpenOrders>, ProgramError> {
    Ok(Ref::map(strip_dex_padding(acc)?, from_bytes))
}


pub fn check_open_orders(
    acc: &AccountInfo,
    owner: &Pubkey
) -> MangoResult<()> {

    if *acc.key == Pubkey::default() {
        return Ok(());
    }
    // if it's not default, it must be initialized
    let open_orders = load_open_orders(acc)?;
    let valid_flags = (serum_dex::state::AccountFlag::Initialized | serum_dex::state::AccountFlag::OpenOrders).bits();
    check_eq_default!(open_orders.account_flags, valid_flags)?;
    let oos_owner = open_orders.owner;
    check_eq_default!(oos_owner, owner.to_aligned_bytes())?;

    Ok(())
}


pub fn load_market_state<'a>(
    market_account: &'a AccountInfo,
    program_id: &Pubkey,
) -> MangoResult<RefMut<'a, serum_dex::state::MarketState>> {
    check_eq_default!(market_account.owner, program_id)?;

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


pub fn load_event_queue_mut<'a>(
    queue_acc: &'a AccountInfo
) -> MangoResult<serum_dex::state::EventQueue<'a>> {

    let orig_data = strip_dex_padding_mut(queue_acc)?;
    let (header, buf) = strip_data_header_mut::<serum_dex::state::EventQueueHeader, serum_dex::state::Event>(orig_data)?;


    Ok(serum_dex::state::Queue { header, buf })
}
