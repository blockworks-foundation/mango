#![cfg(feature="test-bpf")]

mod helpers;

use std::mem::size_of;
use fixed::types::U64F64;
use helpers::*;
use solana_program::instruction::InstructionError;
use solana_program_test::*;
use solana_sdk::{
    pubkey::Pubkey,
    signature::{Signer, Keypair},
    transaction::{Transaction,TransactionError},
    account::Account,
};
use solana_program::account_info::AccountInfo;

use common::create_signer_key_and_nonce;
use mango::{
    entrypoint::process_instruction,
    error::MangoErrorCode,
    instruction::{init_mango_group, deposit_srm, withdraw_srm},
    state::{MangoGroup, MangoSrmAccount},
};

#[tokio::test]
async fn test_init_mango_group() {
    // Mostly a test to ensure we can successfully create the testing harness
    // Also gives us an alert if the InitMangoGroup tx ends up using too much gas
    let program_id = Pubkey::new_unique();

    let mut test = ProgramTest::new(
        "mango",
        program_id,
        processor!(process_instruction),
    );

    // limit to track compute unit increase
    test.set_bpf_compute_max_units(20_000);

    let mango_group = add_mango_group_prodlike(&mut test, program_id);

    let (mut banks_client, payer, recent_blockhash) = test.start().await;

    let mut transaction = Transaction::new_with_payer(
        &[
            mango_group.init_mango_group(&payer.pubkey()),
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
async fn test_deposit_srm() {
    // Test that the DepositSrm instruction succeeds in the simple case
    let program_id = Pubkey::new_unique();

    let mut test = ProgramTest::new(
        "mango",
        program_id,
        processor!(process_instruction),
    );

    // limit to track compute unit increase
    test.set_bpf_compute_max_units(20_000);

    let initial_amount = 500;
    let deposit_amount = 100;

    let user = Keypair::new();
    let user_pk = user.pubkey();
    let mango_group = add_mango_group_prodlike(&mut test, program_id);
    let mango_srm_account_pk = Pubkey::new_unique();
    test.add_account(mango_srm_account_pk, Account::new(u32::MAX as u64, size_of::<MangoSrmAccount>(), &program_id));
    let user_srm_account = add_token_account(&mut test, user_pk, mango_group.srm_mint.pubkey, initial_amount);

    let (mut banks_client, payer, recent_blockhash) = test.start().await;

    let mut transaction = Transaction::new_with_payer(
        &[
            mango_group.init_mango_group(&payer.pubkey()),

            deposit_srm(
                &program_id,
                &mango_group.mango_group_pk,
                &mango_srm_account_pk,
                &user_pk,
                &user_srm_account.pubkey,
                &mango_group.srm_vault.pubkey,
                deposit_amount,
            ).unwrap(),
        ],
        Some(&payer.pubkey()),
    );

    transaction.sign(
        &[&payer, &user],
        recent_blockhash,
    );
    assert!(banks_client.process_transaction(transaction).await.is_ok());

    let final_user_balance = get_token_balance(&mut banks_client, user_srm_account.pubkey).await;
    assert_eq!(final_user_balance, initial_amount - deposit_amount);
    let mango_vault_srm_balance = get_token_balance(&mut banks_client, mango_group.srm_vault.pubkey).await;
    assert_eq!(mango_vault_srm_balance, deposit_amount);

    let mut mango_srm_account = banks_client.get_account(mango_srm_account_pk).await.unwrap().unwrap();
    let account_info: AccountInfo = (&mango_srm_account_pk, &mut mango_srm_account).into();

    let mango_srm_account = MangoSrmAccount::load_mut_checked(
        &program_id,
        &account_info,
        &mango_group.mango_group_pk,
    ).unwrap();
    assert_eq!(mango_srm_account.amount, deposit_amount);
}


#[tokio::test]
async fn test_deposit_srm_error() {
    // Test that the DepositSrm instruction succeeds in the simple case
    let program_id = Pubkey::new_unique();

    let mut test = ProgramTest::new(
        "mango",
        program_id,
        processor!(process_instruction),
    );

    // limit to track compute unit increase
    test.set_bpf_compute_max_units(20_000);

    let initial_amount = 500;
    let deposit_amount = 100;

    let user = Keypair::new();
    let user_pk = user.pubkey();
    let mango_group = add_mango_group_prodlike(&mut test, program_id);
    let mango_srm_account_pk = Pubkey::new_unique();
    test.add_account(mango_srm_account_pk, Account::new(u32::MAX as u64, size_of::<MangoSrmAccount>(), &program_id));
    let user_srm_account = add_token_account(&mut test, user_pk, mango_group.srm_mint.pubkey, initial_amount);

    let (mut banks_client, payer, recent_blockhash) = test.start().await;

    let mut transaction = Transaction::new_with_payer(
        &[
            mango_group.init_mango_group(&payer.pubkey()),

            deposit_srm(
                &program_id,
                &mango_group.mango_group_pk,
                &mango_srm_account_pk,
                &user_pk,
                &user_srm_account.pubkey,
                &mango_group.srm_vault.pubkey,
                deposit_amount,
            ).unwrap(),
        ],
        Some(&payer.pubkey()),
    );

    transaction.sign(
        &[&payer, &user],
        recent_blockhash,
    );
    assert!(banks_client.process_transaction(transaction).await.is_ok());

    let final_user_balance = get_token_balance(&mut banks_client, user_srm_account.pubkey).await;
    assert_eq!(final_user_balance, initial_amount - deposit_amount);
    let mango_vault_srm_balance = get_token_balance(&mut banks_client, mango_group.srm_vault.pubkey).await;
    assert_eq!(mango_vault_srm_balance, deposit_amount);

    let mut mango_srm_account = banks_client.get_account(mango_srm_account_pk).await.unwrap().unwrap();
    let account_info: AccountInfo = (&mango_srm_account_pk, &mut mango_srm_account).into();

    let mango_srm_account = MangoSrmAccount::load_mut_checked(
        &program_id,
        &account_info,
        &mango_group.mango_group_pk,
    ).unwrap();
    assert_eq!(mango_srm_account.amount, deposit_amount);
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

    let sol_vault = add_token_account(&mut test, signer_pk, sol_mint.pubkey, 0);
    let srm_vault = add_token_account(&mut test, signer_pk, srm_mint.pubkey, 0);
    let usdt_vault = add_token_account(&mut test, signer_pk, usdt_mint.pubkey, 0);
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

