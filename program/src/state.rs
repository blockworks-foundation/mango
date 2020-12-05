use std::cell::{Ref, RefMut};

use solana_program::account_info::AccountInfo;
use solana_program::program_error::ProgramError;

use bytemuck::{from_bytes, from_bytes_mut, Pod, Zeroable};
use solana_program::pubkey::Pubkey;
use enumflags2::BitFlags;


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


#[derive(Copy, Clone, BitFlags, Debug, Eq, PartialEq)]
#[repr(u64)]
pub enum AccountFlag {
    Initialized = 1u64 << 0,
    MangoGroup = 1u64 << 1,
}


/// A group of spot markets that can be cross margined together
/// TODO need plans to migrate smart contract, need plans for migration
/// Charge fee on redemption of bond token in proportion to the length of the bond token. 1% per year

#[derive(Copy, Clone)]
#[repr(C)]
pub struct MangoGroup {
    pub account_flags: u64,
    pub quote: Pubkey, // mint of quote currency shared across all markets in this group

}
unsafe impl Zeroable for MangoGroup {}
unsafe impl Pod for MangoGroup {}
impl Loadable for MangoGroup {}


// Track the issuances of bonds by this user
pub struct MangoAccount {
    pub account_flags: u64,
    pub owner: Pubkey,  // solana pubkey of owner
}


/*
** Big Issue -> if bond tokens are traded on serum dex, we will have to pay very large percentage fees per dex transaction

Monetization:
-charge redemption fee in proportion to the length of the bond e.g. 1% per year => 25c on quarterly $100 bond.
    -> to implement this you need to record when the bond token mint was first created
    -> has the problem with intermediate issuances
    -> if no fee on intermediate redemptions, then lenders are incented to close position before expiration
    -> For intermediate issuance, record fees that should be credited back
-charge fee at issuance
    -> problem is intermediate redemptions or burning of tokens
    -> your mango account will record total fees paid, at issuance and
-charge fee on loan volume
    Need to keep this pretty small otherwise spreads will be wide
-charge interest based fee on trade volume

-charge fixed fee per transaction
    -> incents larger transactions over smaller ones, akin to blockchain fee
    -> gives incentive for mango devs to target more users rather than large users
    -> harder for people to lend micro
    -> Have to have own orderbook in order to charge this fee; can't use serum dex
        could be solved by having a large stake in serum and charging no fee

-Serum dex gives a certain amount of the fees to the margin platform that originated the trade
    -> Since we provide

-No trading fee,
 */

/// any object that implements these functions can be the price oracle used for liquidations
pub trait MangoPriceOracle {
    // Look into interfaces for oracles on solidity
}

#[derive(Copy, Clone)]
#[repr(C)]
pub struct SpotDexOracle {
    pub account_flags: u64,
}


#[derive(Copy, Clone)]
#[repr(C)]
/// SPL token that is owned by Mango. Has all the functionality of SPL-tokens
/// Useful for perp bonds
/// Each perp bond token pays 0.10 of the quote token every year (or equiv fraction per hour)
/// every interaction with a particular bond account will pay out interest to this account as long
/// as there is enough quote currency in vault
/// Every interaction with the LBL (liability) version of this token will pay out interest or issue more LBL in order to pay
/// Keep track of last timestamp interest was paid out
pub struct BondAccount {

}


/*
Incentivize people to run the interest cranks by giving over a portion of the interest payment
Every hour, perp bond interest must be paid out

Cranker Staking Collections
-> crankers stake coins in MangoGroup to be able to collect fees from interest
-> If somebody provably does not get paid on time, crankers lose money from stake
-> Perhaps there is an official cranker per account

Lender Collections
Lender himself registers a call to crank it at a certain time

Incentivized Collections
-> Charge 10% of all interest payments.
-> 1% of that goes to the person who runs collections bot
-> Lender himself can run collections and assign fees to himself
-> every holder of the bond token is registered


 */