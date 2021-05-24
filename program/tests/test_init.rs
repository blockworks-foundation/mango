// Tests related to initializing MangoGroups and MarginAccounts
#![cfg(feature="test-bpf")]

mod helpers;

use std::mem::size_of;
use helpers::*;
use solana_program_test::*;
use solana_sdk::{
    pubkey::Pubkey,
    signature::{Signer, Keypair},
    transaction::Transaction,
    account::Account,
};
use solana_program::account_info::AccountInfo;

use mango::{
    entrypoint::process_instruction,
    instruction::init_margin_account,
    state::MarginAccount,
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
    test.set_bpf_compute_max_units(50_000);

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
async fn test_init_margin_account() {
    // Test that we can create a MarginAccount
    // Also make sure that new accounts start with 0 balance
    let program_id = Pubkey::new_unique();

    let mut test = ProgramTest::new(
        "mango",
        program_id,
        processor!(process_instruction),
    );

    // limit to track compute unit increase
    test.set_bpf_compute_max_units(50_000);

    let mango_group = add_mango_group_prodlike(&mut test, program_id);
    let margin_account_pk = Pubkey::new_unique();
    test.add_account(margin_account_pk, Account::new(u32::MAX as u64, size_of::<MarginAccount>(), &program_id));
    let user = Keypair::new();
    test.add_account(user.pubkey(), Account::new(u32::MAX as u64, 0, &user.pubkey()));

    let (mut banks_client, payer, recent_blockhash) = test.start().await;

    let mut transaction = Transaction::new_with_payer(
        &[
            mango_group.init_mango_group(&payer.pubkey()),
            init_margin_account(
                &program_id,
                &mango_group.mango_group_pk,
                &margin_account_pk,
                &user.pubkey(),
            ).unwrap(),
        ],
        Some(&payer.pubkey()),
    );

    transaction.sign(
        &[&payer, &user],
        recent_blockhash,
    );
    assert!(banks_client.process_transaction(transaction).await.is_ok());

    let mut account = banks_client.get_account(margin_account_pk).await.unwrap().unwrap();
    let account_info: AccountInfo = (&margin_account_pk, &mut account).into();
    let margin_account = MarginAccount::load_mut_checked(
        &program_id,
        &account_info,
        &mango_group.mango_group_pk,
    ).unwrap();
    for dep in &margin_account.deposits {
        assert_eq!(dep.to_bits(), 0);
    }
    for borrow in &margin_account.borrows {
        assert_eq!(borrow.to_bits(), 0);
    }
}
