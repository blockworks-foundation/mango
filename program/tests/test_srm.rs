// Tests related to the SRM vault of a MangoGroup
#![cfg(feature = "test-bpf")]

mod helpers;

use helpers::*;
use solana_program::account_info::AccountInfo;
use solana_program_test::*;
use solana_sdk::{
    account::Account,
    pubkey::Pubkey,
    signature::{Keypair, Signer},
    transaction::Transaction,
};
use std::mem::size_of;

use mango::{
    entrypoint::process_instruction,
    instruction::{deposit_srm, withdraw_srm},
    state::MangoSrmAccount,
};

#[tokio::test]
async fn test_deposit_srm() {
    // Test that the DepositSrm instruction succeeds in the simple case
    let program_id = Pubkey::new_unique();

    let mut test = ProgramTest::new("mango", program_id, processor!(process_instruction));

    // limit to track compute unit increase
    test.set_bpf_compute_max_units(50_000);

    let initial_amount = 500;
    let deposit_amount = 100;
    let withdraw_amount = 10;

    let user = Keypair::new();
    let user_pk = user.pubkey();
    let mango_group = add_mango_group_prodlike(&mut test, program_id);
    let mango_srm_account_pk = Pubkey::new_unique();
    test.add_account(
        mango_srm_account_pk,
        Account::new(u32::MAX as u64, size_of::<MangoSrmAccount>(), &program_id),
    );
    let user_srm_account = add_token_account(
        &mut test,
        user_pk,
        mango_group.srm_mint.pubkey,
        initial_amount,
    );

    let (mut banks_client, payer, recent_blockhash) = test.start().await;

    {
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
                )
                .unwrap(),
            ],
            Some(&payer.pubkey()),
        );

        transaction.sign(&[&payer, &user], recent_blockhash);
        assert!(banks_client.process_transaction(transaction).await.is_ok());

        let final_user_balance =
            get_token_balance(&mut banks_client, user_srm_account.pubkey).await;
        assert_eq!(final_user_balance, initial_amount - deposit_amount);
        let mango_vault_srm_balance =
            get_token_balance(&mut banks_client, mango_group.srm_vault.pubkey).await;
        assert_eq!(mango_vault_srm_balance, deposit_amount);

        let mut mango_srm_account = banks_client
            .get_account(mango_srm_account_pk)
            .await
            .unwrap()
            .unwrap();
        let account_info: AccountInfo = (&mango_srm_account_pk, &mut mango_srm_account).into();

        let mango_srm_account = MangoSrmAccount::load_mut_checked(
            &program_id,
            &account_info,
            &mango_group.mango_group_pk,
        )
        .unwrap();
        assert_eq!(mango_srm_account.amount, deposit_amount);
    }

    {
        let mut transaction = Transaction::new_with_payer(
            &[withdraw_srm(
                &program_id,
                &mango_group.mango_group_pk,
                &mango_srm_account_pk,
                &user_pk,
                &user_srm_account.pubkey,
                &mango_group.srm_vault.pubkey,
                &mango_group.signer_pk,
                withdraw_amount,
            )
            .unwrap()],
            Some(&payer.pubkey()),
        );
        transaction.sign(&[&user, &payer], recent_blockhash);
        assert!(banks_client.process_transaction(transaction).await.is_ok());

        let final_user_balance =
            get_token_balance(&mut banks_client, user_srm_account.pubkey).await;
        assert_eq!(
            final_user_balance,
            initial_amount - deposit_amount + withdraw_amount
        );
        let mango_vault_srm_balance =
            get_token_balance(&mut banks_client, mango_group.srm_vault.pubkey).await;
        assert_eq!(mango_vault_srm_balance, deposit_amount - withdraw_amount);

        let mut mango_srm_account = banks_client
            .get_account(mango_srm_account_pk)
            .await
            .unwrap()
            .unwrap();
        let account_info: AccountInfo = (&mango_srm_account_pk, &mut mango_srm_account).into();

        let mango_srm_account = MangoSrmAccount::load_mut_checked(
            &program_id,
            &account_info,
            &mango_group.mango_group_pk,
        )
        .unwrap();
        assert_eq!(mango_srm_account.amount, deposit_amount - withdraw_amount);
    }
}
