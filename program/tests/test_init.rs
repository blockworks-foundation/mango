mod helpers;

use fixed::types::U64F64;
use helpers::*;
use solana_program_test::*;
use solana_sdk::{
    pubkey::Pubkey,
    signature::{Signer, Keypair},
    transaction::Transaction,
    account::Account,
};

use mango::entrypoint::process_instruction;
use mango::instruction::init_mango_group;

#[tokio::test]
async fn test_success() {
    let main_keypair = Keypair::new();
    let program_id = main_keypair.pubkey();

    let mut test = ProgramTest::new(
        "mango",
        program_id,
        processor!(process_instruction),
    );

    let mango_group = Keypair::new();
    test.add_account(mango_group.pubkey(), Account::new(u32::MAX as u64, 0, &program_id));

    // limit to track compute unit increase
    // test.set_bpf_compute_max_units(51_000);

    let usdt_mint = add_mint(&mut test, 6);
    let btc_mint = add_mint(&mut test, 6);
    let eth_mint = add_mint(&mut test, 6);
    let mints = vec![btc_mint.pubkey, eth_mint.pubkey, usdt_mint.pubkey];

    let vaults = vec![];
    let dexes = vec![];

    let unit = 10u64.pow(6);
    let btc_usdt = add_aggregator(&mut test, "BTC:USDT", 6, 50_000 * unit, &program_id);
    let eth_usdt = add_aggregator(&mut test, "ETH:USDT", 6, 2_000 * unit, &program_id);
    let oracles = vec![btc_usdt.pubkey, eth_usdt.pubkey];

    let (mut banks_client, payer, recent_blockhash) = test.start().await;

    let signer_pk = Pubkey::new_unique();
    let signer_nonce = 0;
    let dex_prog_id = Pubkey::new_unique();
    let srm_vault_pk = Pubkey::new_unique();
    let admin_pk = Pubkey::new_unique();
    let borrow_limits = [100, 100, 100];

    let mut transaction = Transaction::new_with_payer(
        &[
            init_mango_group(
                &program_id,
                &mango_group.pubkey(),
                &signer_pk,
                &dex_prog_id,
                &srm_vault_pk,
                &admin_pk,
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