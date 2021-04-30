mod helpers;

use std::mem::size_of;

use fixed::types::U64F64;
use helpers::*;
use solana_program::instruction::InstructionError;
use solana_program_test::*;
use solana_sdk::{
    pubkey::Pubkey,
    signature::Signer,
    transaction::{Transaction,TransactionError},
    account::Account,
};

use mango::entrypoint::process_instruction;
use mango::error::MangoErrorCode;
use mango::instruction::{init_mango_group, deposit_srm, withdraw_srm};
use mango::state::{MangoGroup, MangoSrmAccount};
use common::create_signer_key_and_nonce;

#[tokio::test]
async fn test_init_btc_eth_usdt() {
    let program_id = Pubkey::new_unique();

    let mut test = ProgramTest::new(
        "mango",
        program_id,
        processor!(process_instruction),
    );

    // limit to track compute unit increase
    test.set_bpf_compute_max_units(51_000);

    let mango_group_pk = Pubkey::new_unique();
    let (signer_pk, signer_nonce) = create_signer_key_and_nonce(&program_id, &mango_group_pk);
    test.add_account(mango_group_pk, Account::new(u32::MAX as u64, size_of::<MangoGroup>(), &program_id));

    let btc_mint = add_mint(&mut test, 6);
    let eth_mint = add_mint(&mut test, 6);
    let usdt_mint = add_mint(&mut test, 6);
    let mints = vec![btc_mint.pubkey, eth_mint.pubkey, usdt_mint.pubkey];

    let btc_vault = add_token_account(&mut test, signer_pk, btc_mint.pubkey);
    let eth_vault = add_token_account(&mut test, signer_pk, eth_mint.pubkey);
    let usdt_vault = add_token_account(&mut test, signer_pk, usdt_mint.pubkey);
    let vaults = vec![btc_vault.pubkey, eth_vault.pubkey, usdt_vault.pubkey];

    let srm_mint = add_mint_srm(&mut test);
    let srm_vault = add_token_account(&mut test, signer_pk, srm_mint.pubkey);

    let dex_prog_id = Pubkey::new_unique();
    let btc_usdt_dex = add_dex_empty(&mut test, btc_mint.pubkey, usdt_mint.pubkey, dex_prog_id);
    let eth_usdt_dex = add_dex_empty(&mut test, eth_mint.pubkey, usdt_mint.pubkey, dex_prog_id);
    let dexes = vec![btc_usdt_dex.pubkey, eth_usdt_dex.pubkey];

    let unit = 10u64.pow(6);
    let btc_usdt = add_aggregator(&mut test, "BTC:USDT", 6, 50_000 * unit, &program_id);
    let eth_usdt = add_aggregator(&mut test, "ETH:USDT", 6, 2_000 * unit, &program_id);
    let oracles = vec![btc_usdt.pubkey, eth_usdt.pubkey];

    let (mut banks_client, payer, recent_blockhash) = test.start().await;

    let borrow_limits = [100, 100, 100];

    let mut transaction = Transaction::new_with_payer(
        &[
            init_mango_group(
                &program_id,
                &mango_group_pk,
                &signer_pk,
                &dex_prog_id,
                &srm_vault.pubkey,
                &payer.pubkey(),
                &mints,
                &vaults,
                &dexes,
                &oracles,
                signer_nonce,
                U64F64::from_num(1.1),
                U64F64::from_num(1.2),
                borrow_limits,
            ).unwrap(),
        ],
        Some(&payer.pubkey()),
    );

    transaction.sign(
        &[&payer],
        recent_blockhash,
    );
    assert!(banks_client.process_transaction(transaction).await.is_ok());
}


#[tokio::test]
async fn test_init_srm_sol_usdt() {
    let program_id = Pubkey::new_unique();

    let mut test = ProgramTest::new(
        "mango",
        program_id,
        processor!(process_instruction),
    );

    // limit to track compute unit increase
    test.set_bpf_compute_max_units(51_000);

    let mango_group_pk = Pubkey::new_unique();
    let (signer_pk, signer_nonce) = create_signer_key_and_nonce(&program_id, &mango_group_pk);
    test.add_account(mango_group_pk, Account::new(u32::MAX as u64, size_of::<MangoGroup>(), &program_id));

    let sol_mint = add_mint(&mut test, 6);
    let srm_mint = add_mint_srm(&mut test);
    let usdt_mint = add_mint(&mut test, 6);
    let mints = vec![sol_mint.pubkey, srm_mint.pubkey, usdt_mint.pubkey];

    let sol_vault = add_token_account(&mut test, signer_pk, sol_mint.pubkey);
    let srm_vault = add_token_account(&mut test, signer_pk, srm_mint.pubkey);
    let usdt_vault = add_token_account(&mut test, signer_pk, usdt_mint.pubkey);
    let vaults = vec![sol_vault.pubkey, srm_vault.pubkey, usdt_vault.pubkey];

    let dex_prog_id = Pubkey::new_unique();
    let sol_usdt_dex = add_dex_empty(&mut test, sol_mint.pubkey, usdt_mint.pubkey, dex_prog_id);
    let srm_usdt_dex = add_dex_empty(&mut test, srm_mint.pubkey, usdt_mint.pubkey, dex_prog_id);
    let dexes = vec![sol_usdt_dex.pubkey, srm_usdt_dex.pubkey];

    let unit = 10u64.pow(6);
    let sol_usdt = add_aggregator(&mut test, "SOL:USDT", 6, 50_000 * unit, &program_id);
    let srm_usdt = add_aggregator(&mut test, "SRM:USDT", 6, 2_000 * unit, &program_id);
    let oracles = vec![sol_usdt.pubkey, srm_usdt.pubkey];


    let mango_srm_account_pk = Pubkey::new_unique();
    test.add_account(mango_srm_account_pk, Account::new(u32::MAX as u64, size_of::<MangoSrmAccount>(), &program_id));


    let (mut banks_client, payer, recent_blockhash) = test.start().await;

    let borrow_limits = [100, 100, 100];

    let mut transaction = Transaction::new_with_payer(
        &[
            init_mango_group(
                &program_id,
                &mango_group_pk,
                &signer_pk,
                &dex_prog_id,
                &srm_vault.pubkey,
                &payer.pubkey(),
                &mints,
                &vaults,
                &dexes,
                &oracles,
                signer_nonce,
                U64F64::from_num(1.1),
                U64F64::from_num(1.2),
                borrow_limits,
            ).unwrap(),
        ],
        Some(&payer.pubkey()),
    );

    transaction.sign(
        &[&payer],
        recent_blockhash,
    );
    assert!(banks_client.process_transaction(transaction).await.is_ok());

    let srm_account_pk = Pubkey::new_unique();

    let mut transaction_deposit = Transaction::new_with_payer(
        &[
            deposit_srm(
                &program_id,
                &mango_group_pk,
                &mango_srm_account_pk,
                &payer.pubkey(),
                &srm_account_pk,
                &srm_vault.pubkey,
                100
            ).unwrap(),
        ],
        Some(&payer.pubkey()),
    );

    transaction_deposit.sign(
        &[&payer],
        recent_blockhash,
    );

    assert_eq!(
        banks_client
            .process_transaction(transaction_deposit)
            .await
            .unwrap_err()
            .unwrap(),
        TransactionError::InstructionError(
            0,
            InstructionError::Custom(
                MangoErrorCode::FeeDiscountFunctionality.into()))
    );

   let mut transaction_withdraw = Transaction::new_with_payer(
        &[
            withdraw_srm(
                &program_id,
                &mango_group_pk,
                &mango_srm_account_pk,
                &payer.pubkey(),
                &srm_account_pk,
                &srm_vault.pubkey,
                &signer_pk,
                100
            ).unwrap(),
        ],
        Some(&payer.pubkey()),
    );

    transaction_withdraw.sign(
        &[&payer],
        recent_blockhash,
    );

    assert_eq!(
        banks_client
            .process_transaction(transaction_withdraw)
            .await
            .unwrap_err()
            .unwrap(),
        TransactionError::InstructionError(
            0,
            InstructionError::Custom(
                MangoErrorCode::FeeDiscountFunctionality.into()))
    );

}
