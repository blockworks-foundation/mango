mod helpers;

use helpers::*;
use solana_program_test::*;
use solana_sdk::{
    pubkey::Pubkey,
    signature::Signer,
    transaction::Transaction,
};

use mango::entrypoint::process_instruction;

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