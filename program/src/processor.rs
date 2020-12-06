use solana_program::account_info::AccountInfo;
use solana_program::entrypoint::ProgramResult;
use solana_program::pubkey::Pubkey;

pub struct Processor {}


impl Processor {
    #[allow(unused_variables)]
    fn init_mango_group(
        program_id: &Pubkey,
        accounts: &[AccountInfo],
        signer_nonce: u64
    ) -> ProgramResult {
        unimplemented!()
    }

    #[allow(unused_variables)]
    pub fn process(
        program_id: &Pubkey,
        accounts: &[AccountInfo],
        data: &[u8]
    ) -> ProgramResult {
        Ok(())
    }
}

/*
TODO
Initial launch
- UI
- funding book
- we market make on the book
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