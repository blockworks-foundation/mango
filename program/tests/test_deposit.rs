// Tests related to depositing in a MangoGroup
#![cfg(feature="test-bpf")]

mod helpers;

use std::mem::size_of;
use helpers::*;
use solana_program::account_info::AccountInfo;
use solana_program_test::*;
use solana_sdk::{
    pubkey::Pubkey,
    signature::{Signer, Keypair},
    transaction::Transaction,
    account::Account,
};

use mango::{
    entrypoint::process_instruction,
    instruction::{deposit, init_margin_account},
    state::MarginAccount,
};

#[tokio::test]
async fn test_deposit_succeeds() {
    // Test that the deposit instruction succeeds and the expected side effects occurr
    let program_id = Pubkey::new_unique();

    let mut test = ProgramTest::new(
        "mango",
        program_id,
        processor!(process_instruction),
    );

    // limit to track compute unit increase
    test.set_bpf_compute_max_units(50_000);

    let initial_amount = 2;
    let deposit_amount = 1;

    // setup mango group
    let mango_group = add_mango_group_prodlike(&mut test, program_id);

    // setup user account
    let user = Keypair::new();
    test.add_account(user.pubkey(), Account::new(u32::MAX as u64, 0, &user.pubkey()));

    // setup user token accounts
    let user_account = add_token_account(
        &mut test,
        user.pubkey(),
        mango_group.mints[0].pubkey,
        initial_amount,
    );    

    // setup marginaccount account
    let margin_account_pk = Pubkey::new_unique();
    test.add_account(margin_account_pk, Account::new(u32::MAX as u64, size_of::<MarginAccount>(), &program_id));

    // setup test harness
    let (mut banks_client, payer, recent_blockhash) = test.start().await;

    {
        let mut transaction = Transaction::new_with_payer(
            &[
                mango_group.init_mango_group(&payer.pubkey()),
                init_margin_account(
                    &program_id,
                    &mango_group.mango_group_pk,
                    &margin_account_pk,
                    &user.pubkey(),
                ).unwrap(),
                deposit(
                    &program_id,
                    &mango_group.mango_group_pk,
                    &margin_account_pk,
                    &user.pubkey(),
                    &user_account.pubkey,
                    &mango_group.vaults[0].pubkey,
                    deposit_amount,
                ).unwrap(),
            ],
            Some(&payer.pubkey()),
        );

        transaction.sign(
            &[&payer, &user],
            recent_blockhash,
        );

        // Test transaction succeeded
        assert!(banks_client.process_transaction(transaction).await.is_ok());

        // Test expected amount is deducted from user wallet
        let final_user_balance = get_token_balance(&mut banks_client, user_account.pubkey).await;
        assert_eq!(final_user_balance, initial_amount - deposit_amount);

        // Test expected amount is added to the vault
        let mango_vault_balance = get_token_balance(&mut banks_client, mango_group.vaults[0].pubkey).await;
        assert_eq!(mango_vault_balance, deposit_amount);

        // Test expected amount is in margin account
        let mut margin_account = banks_client
            .get_account(margin_account_pk)
            .await
            .unwrap()
            .unwrap();
        let account_info: AccountInfo = (&margin_account_pk, &mut margin_account).into();

        let margin_account = MarginAccount::load_mut_checked(
            &program_id,
            &account_info,
            &mango_group.mango_group_pk,
        )
        .unwrap();
        assert_eq!(margin_account.deposits[0], deposit_amount);
    }
}

#[tokio::test]
async fn test_deposit_fails_invalid_margin_account_owner() {
    // Test that the deposit instruction fails if the margin account owner is not the payer
    let program_id = Pubkey::new_unique();

    let mut test = ProgramTest::new(
        "mango",
        program_id,
        processor!(process_instruction),
    );

    // limit to track compute unit increase
    test.set_bpf_compute_max_units(50_000);

    let initial_amount = 2;
    let deposit_amount = 3;

    // setup mango group
    let mango_group = add_mango_group_prodlike(&mut test, program_id);

    // setup user accounts
    let user = Keypair::new();
    test.add_account(user.pubkey(), Account::new(u32::MAX as u64, 0, &user.pubkey()));
    let other_user = Keypair::new();
    test.add_account(other_user.pubkey(), Account::new(u32::MAX as u64, 0, &other_user.pubkey()));

    // setup user token accounts
    let user_account = add_token_account(
        &mut test,
        user.pubkey(),
        mango_group.mints[0].pubkey,
        initial_amount,
    );    

    // setup marginaccount account
    let margin_account_pk = Pubkey::new_unique();
    test.add_account(margin_account_pk, Account::new(u32::MAX as u64, size_of::<MarginAccount>(), &program_id));

    // setup test harness
    let (mut banks_client, payer, recent_blockhash) = test.start().await;

    {
        let mut transaction = Transaction::new_with_payer(
            &[
                mango_group.init_mango_group(&payer.pubkey()),
                init_margin_account(
                    &program_id,
                    &mango_group.mango_group_pk,
                    &margin_account_pk,
                    &other_user.pubkey(),
                ).unwrap(),
            ],
            Some(&payer.pubkey()),
        );

        transaction.sign(
            &[&payer, &other_user],
            recent_blockhash,
        );

        // Test transaction succeeded
        assert!(banks_client.process_transaction(transaction).await.is_ok());
    }

    {
        let mut transaction = Transaction::new_with_payer(
            &[
                deposit(
                    &program_id,
                    &mango_group.mango_group_pk,
                    &margin_account_pk,
                    &user.pubkey(),
                    &user_account.pubkey,
                    &mango_group.vaults[0].pubkey,
                    deposit_amount,
                ).unwrap(),
            ],
            Some(&payer.pubkey()),
        );

        transaction.sign(
            &[&payer, &user],
            recent_blockhash,
        );

        // Test transaction failed
        assert!(banks_client.process_transaction(transaction).await.is_err());

        // Test no deductions from user wallet
        let final_user_balance = get_token_balance(&mut banks_client, user_account.pubkey).await;
        assert_eq!(final_user_balance, initial_amount);

        // Test nothing is added to the vault
        let mango_vault_balance = get_token_balance(&mut banks_client, mango_group.vaults[0].pubkey).await;
        assert_eq!(mango_vault_balance, 0);

        // Test nothing is in margin account
        let mut margin_account = banks_client
            .get_account(margin_account_pk)
            .await
            .unwrap()
            .unwrap();
        let account_info: AccountInfo = (&margin_account_pk, &mut margin_account).into();

        let margin_account = MarginAccount::load_mut_checked(
            &program_id,
            &account_info,
            &mango_group.mango_group_pk,
        )
        .unwrap();
        assert_eq!(margin_account.deposits[0], 0);
    }
}