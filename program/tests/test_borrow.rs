// Tests related to borrowing on a MangoGroup
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
    instruction::{deposit, borrow, init_margin_account},
    state::MarginAccount,
    state::MangoGroup,
};

#[tokio::test]
async fn test_borrow_succeeds() {
    // Test that the borrow instruction succeeds and the expected side effects occurr
    let program_id = Pubkey::new_unique();

    let mut test = ProgramTest::new(
        "mango",
        program_id,
        processor!(process_instruction),
    );

    // limit to track compute unit increase
    test.set_bpf_compute_max_units(50_000);

    let deposit_token_index = 0;
    let borrow_token_index = 1;
    let initial_amount = 2;
    let deposit_amount = 1;
    // 5x leverage
    let borrow_amount = (deposit_amount * PRICE_BTC * 5) / PRICE_ETH;

    let mango_group = add_mango_group_prodlike(&mut test, program_id);
    let mango_group_pk = mango_group.mango_group_pk;

    let user = Keypair::new();
    test.add_account(user.pubkey(), Account::new(u32::MAX as u64, 0, &user.pubkey()));

    let user_account = add_token_account(
        &mut test,
        user.pubkey(),
        mango_group.mints[deposit_token_index].pubkey,
        initial_amount,
    );    

    let margin_account_pk = Pubkey::new_unique();
    test.add_account(margin_account_pk, Account::new(u32::MAX as u64, size_of::<MarginAccount>(), &program_id));

    let (mut banks_client, payer, recent_blockhash) = test.start().await;

    // setup mango group and make a deposit
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
                    &mango_group.vaults[deposit_token_index].pubkey,
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
    }

    // make a borrow
    {
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

        let mut transaction = Transaction::new_with_payer(
            &[
                borrow(
                    &program_id,
                    &mango_group.mango_group_pk,
                    &margin_account_pk,
                    &user.pubkey(),
                    &margin_account.open_orders,
                    mango_group.oracles.iter().map(|m| m.pubkey).collect::<Vec<Pubkey>>().as_slice(),
                    borrow_token_index,
                    borrow_amount,
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
        // Test expected borrow is in margin account       
        assert_eq!(margin_account.borrows[borrow_token_index], borrow_amount);
      
        let mut mango_group = banks_client
            .get_account(mango_group_pk)
            .await
            .unwrap()
            .unwrap();
        let account_info: AccountInfo = (&mango_group_pk, &mut mango_group).into();

        let mango_group = MangoGroup::load_mut_checked(
            &account_info,
            &program_id,
        )
        .unwrap();
        // Test expected borrow is added to total in mango group
        assert_eq!(mango_group.total_borrows[borrow_token_index], borrow_amount);
    }
}

#[tokio::test]
async fn test_borrow_fails_overleveraged() {
    // Test that the deposit instruction fails when a user exceeds their leverage limit
    let program_id = Pubkey::new_unique();

    let mut test = ProgramTest::new(
        "mango",
        program_id,
        processor!(process_instruction),
    );

    // limit to track compute unit increase
    test.set_bpf_compute_max_units(50_000);

    let deposit_token_index = 0;
    let borrow_token_index = 1;
    let initial_amount = 2;
    let deposit_amount = 1;
    // try to go for 6x leverage
    let borrow_amount = (deposit_amount * PRICE_BTC * 6) / PRICE_ETH;

    let mango_group = add_mango_group_prodlike(&mut test, program_id);
    let mango_group_pk = mango_group.mango_group_pk;

    let user = Keypair::new();
    test.add_account(user.pubkey(), Account::new(u32::MAX as u64, 0, &user.pubkey()));

    let user_account = add_token_account(
        &mut test,
        user.pubkey(),
        mango_group.mints[deposit_token_index].pubkey,
        initial_amount,
    );    

    let margin_account_pk = Pubkey::new_unique();
    test.add_account(margin_account_pk, Account::new(u32::MAX as u64, size_of::<MarginAccount>(), &program_id));

    let (mut banks_client, payer, recent_blockhash) = test.start().await;

    // setup mango group and make a deposit
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
                    &mango_group.vaults[deposit_token_index].pubkey,
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
    }

    // make a borrow
    {
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

        let mut transaction = Transaction::new_with_payer(
            &[
                borrow(
                    &program_id,
                    &mango_group.mango_group_pk,
                    &margin_account_pk,
                    &user.pubkey(),
                    &margin_account.open_orders,
                    mango_group.oracles.iter().map(|m| m.pubkey).collect::<Vec<Pubkey>>().as_slice(),
                    borrow_token_index,
                    borrow_amount,
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
        // Test no borrow is in margin account       
        assert_eq!(margin_account.borrows[borrow_token_index], 0);
      
        let mut mango_group = banks_client
            .get_account(mango_group_pk)
            .await
            .unwrap()
            .unwrap();
        let account_info: AccountInfo = (&mango_group_pk, &mut mango_group).into();

        let mango_group = MangoGroup::load_mut_checked(
            &account_info,
            &program_id,
        )
        .unwrap();
        // Test nothing is added to total in mango group
        assert_eq!(mango_group.total_borrows[borrow_token_index], 0);
    }
}