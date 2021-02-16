use std::{thread, time};
use std::collections::HashMap;
use std::fs::File;
use std::io::Write;
use std::mem::size_of;
use std::str::FromStr;

use anyhow::{Result};
use arrayref::array_ref;
use clap::Clap;
use common::{Cluster, convert_assertion_error, create_account_rent_exempt,
             create_signer_key_and_nonce, create_token_account, read_keypair_file, send_instructions};
use fixed::types::U64F64;
use mango::instruction::{borrow, deposit, init_mango_group, init_margin_account, liquidate, withdraw, settle_borrow};
use mango::processor::get_prices;
use mango::state::{Loadable, MangoGroup, MarginAccount, NUM_MARKETS, NUM_TOKENS};
use serde_json::{json, Value};
use solana_client::rpc_client::RpcClient;
use solana_client::rpc_config::{RpcAccountInfoConfig, RpcProgramAccountsConfig};
use solana_client::rpc_filter::{Memcmp, MemcmpEncodedBytes, RpcFilterType};
use solana_client::rpc_request::TokenAccountsFilter;
use solana_client::rpc_response::RpcKeyedAccount;
use solana_sdk::account::{Account, create_account_infos};
use solana_sdk::commitment_config::CommitmentConfig;
use solana_sdk::program_pack::Pack;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::{Keypair, Signer};

#[derive(Clap, Debug)]
pub struct Opts {
    #[clap(default_value = "mainnet")]
    pub cluster: Cluster,
    #[clap(subcommand)]
    pub command: Command,
}

#[derive(Clap, Debug)]
pub enum Command {
    InitMangoGroup {
        #[clap(long, short)]
        payer: String,
        #[clap(long, short)]
        ids_path: String,
        #[clap(long, short)]
        tokens: Vec<String>,
        #[clap(long, short)]
        mango_program_id: Option<String>,
    },
    InitMarginAccount {
        #[clap(long, short)]
        payer: String,
        #[clap(long, short)]
        ids_path: String,
        #[clap(long, short)]
        mango_group_name: String,
    },
    Deposit {
        #[clap(long, short)]
        payer: String,
        #[clap(long, short)]
        ids_path: String,
        #[clap(long, short)]
        mango_group_name: String,
        #[clap(long)]
        token_symbol: String,
        #[clap(long)]
        margin_account: String,
        #[clap(long)]
        quantity: f64
    },
    Withdraw {
        #[clap(long, short)]
        payer: String,
        #[clap(long, short)]
        ids_path: String,
        #[clap(long, short)]
        mango_group_name: String,
        #[clap(long)]
        token_symbol: String,
        #[clap(long)]
        margin_account: String,
        #[clap(long)]
        quantity: f64
    },
    Borrow {
        #[clap(long, short)]
        payer: String,
        #[clap(long, short)]
        ids_path: String,
        #[clap(long)]
        mango_group_name: String,
        #[clap(long)]
        margin_account: String,
        #[clap(long, short)]
        token_symbol: String,
        #[clap(long, short)]
        quantity: f64
    },
    SettleBorrow {
        #[clap(long, short)]
        payer: String,
        #[clap(long, short)]
        ids_path: String,
        #[clap(long)]
        mango_group_name: String,
        #[clap(long)]
        margin_account: String,
        #[clap(long, short)]
        token_symbol: String,
        #[clap(long, short)]
        quantity: Option<f64>
    },
    ConvertAssertionError {
        #[clap(long, short)]
        code: u32,
    },
    PrintBs58 {
        #[clap(long, short)]
        keypair: String,
        #[clap(long, short)]
        filepath: Option<String>,
    },
    RunLiquidator {
        #[clap(long, short)]
        ids_path: String,
        #[clap(long, short)]
        payer: String,
        #[clap(long, short)]
        mango_group_name: String,
    },

    PlaceOrder {

    },
    SettleFunds {

    },
    CancelOrder {

    },

    PrintPrices {
        #[clap(long, short)]
        ids_path: String,
        #[clap(long, short)]
        mango_group_name: String,
    },
    PrintMarginAccountInfo {
        #[clap(long, short)]
        ids_path: String,
        #[clap(long)]
        mango_group_name: String,
        #[clap(long)]
        margin_account: String
    }
}

impl Opts {
    fn client(&self) -> RpcClient {
        RpcClient::new_with_commitment(self.cluster.url().to_string(),
                                       CommitmentConfig::confirmed())
    }
}


fn get_accounts(client: &RpcClient, pks: &[Pubkey]) -> Vec<(Pubkey, Account)> {
    client.get_multiple_accounts(pks)
        .unwrap()
        .iter()
        .enumerate()
        .map(|(i, a)| (pks[i], a.as_ref().unwrap().clone()))
        .collect()
}

fn run_liquidator(
    client: &RpcClient,
    cids: ClusterIds,
    mgids: MangoGroupIds,
    liqor_kp: &Keypair
) -> Result<()> {
    // TODO
    /*
        place_order
        cancel_order
        settle_funds
     */

    let sleep_time = time::Duration::from_secs(2);

    let mut mint_accs = get_accounts(client, &mgids.mint_pks);
    let mint_accs = create_account_infos(mint_accs.as_mut_slice());
    let mint_accs = array_ref![mint_accs.as_slice(), 0, NUM_TOKENS];
    let mango_group_acc = client.get_account(&mgids.mango_group_pk)?;
    let mango_group = MangoGroup::load_from_bytes(mango_group_acc.data.as_slice())?;
    let min_coll_ratio = U64F64::from_num(1.02);

    let liqor_token_accounts: Vec<RpcKeyedAccount> = mgids.mint_pks.iter().map(
        |pk| client.get_token_accounts_by_owner_with_commitment(
            &liqor_kp.pubkey(),
            TokenAccountsFilter::Mint(*pk),
            CommitmentConfig::confirmed()
        ).unwrap().value[0].clone()
    ).collect();
    let liqor_token_account_pks: Vec<Pubkey> = liqor_token_accounts.iter().map(
        |rka| Pubkey::from_str(&rka.pubkey).unwrap()
    ).collect();

    loop {
        let t0 = time::SystemTime::now();
        let mut oracle_accs = get_accounts(client, &mgids.oracle_pks);
        let oracle_accs = create_account_infos(oracle_accs.as_mut_slice());
        let oracle_accs = array_ref![oracle_accs.as_slice(), 0, NUM_MARKETS];

        let prices = get_prices(mango_group, oracle_accs)?;

        // fetch all margin accounts
        let config = RpcProgramAccountsConfig {
            filters: Some(vec![
                RpcFilterType::Memcmp(Memcmp {
                    offset: 8,
                    bytes: MemcmpEncodedBytes::Binary(mgids.mango_group_pk.to_string()),
                    encoding: None
                }),
                RpcFilterType::DataSize(size_of::<MarginAccount>() as u64)
            ]),
            account_config: RpcAccountInfoConfig::default()
        };
        let result = client.get_program_accounts_with_config(&cids.mango_program_id, config)?;

        // Go to each margin account and check collateral
        for (pk, margin_account_acc) in result.iter() {
            let margin_account = MarginAccount::load_from_bytes(margin_account_acc.data.as_slice())?;

            let mut open_orders_accs = get_accounts(client, &margin_account.open_orders);
            let open_orders_accs = create_account_infos(open_orders_accs.as_mut_slice());
            let open_orders_accs = array_ref![open_orders_accs.as_slice(), 0, NUM_MARKETS];


            let coll_ratio = margin_account.get_collateral_ratio(mango_group, &prices, open_orders_accs)?;

            println!("{} {} {}", pk, margin_account.owner, coll_ratio);
            println!("{} {} {}", margin_account.deposits[0], margin_account.deposits[1], margin_account.deposits[2]);
            println!("{} {} {}", margin_account.borrows[0], margin_account.borrows[1], margin_account.borrows[2]);
            // println!("{} {} {}\n", margin_account.positions[0], margin_account.positions[1], margin_account.positions[2]);

            if coll_ratio < mango_group.maint_coll_ratio && coll_ratio >= min_coll_ratio {
                // determine how much to deposit to get the account above init coll ratio
                let deficit = margin_account.get_collateral_deficit(
                    mango_group,
                    &prices,
                    open_orders_accs
                )?;

                println!("Sending liquidation instruction");
                let instruction = liquidate(
                    &cids.mango_program_id,
                    &mgids.mango_group_pk,
                    &liqor_kp.pubkey(),
                    pk,
                    open_orders_accs.iter().map(|a| *a.key).collect::<Vec<Pubkey>>().as_slice(),
                    mgids.oracle_pks.as_slice(),
                    &mango_group.vaults,
                    liqor_token_account_pks.as_slice(),
                    &mgids.mint_pks,
                    [0, 0, deficit * 101 / 100]
                )?;
                let instructions = vec![instruction];
                let signers = vec![liqor_kp];
                match send_instructions(&client, instructions, signers, &liqor_kp.pubkey()) {
                    Ok(()) => {
                        println!("Successfully taken ownership of the MarginAccount");
                        // 1. cancel all outstanding orders and settle funds into MarginAccount
                        //      a. load all outstanding orders
                        //      b. cancel each one using its order id
                        // 2. for each of the borrowed assets, see how to close them
                        // 3. then send closing orders for them

                        /*
                            Load outstanding orders:
                            need to fetch the bids and asks
                            iterate through and filter for orders owned by this user

                         */

                    }
                    Err(e) => {
                        println!("{}", e);
                    }
                }
            }

        }

        let elapsed = time::SystemTime::now().duration_since(t0)?.as_millis();
        println!("{}", elapsed);
        // update prices using oracle
        // calculate collateralization ratio for each of them
        // if coll ratio below maint_coll_ratio
        //  check if you have enough funds to liquidate it
        //  if you do have enough funds, send liquidation instruction
        //  once you own the margin account, now send sell orders in the dex to get rid of them
        // cancel orders in the margin account first
        // then withdraw all funds from the margin account and (delete?)

        println!("sleeping");
        thread::sleep(sleep_time);
    }
}

fn print_prices(
    client: &RpcClient,
    cids: ClusterIds,
    mango_group_name: String,
) -> Result<()> {
    let mgids = &cids.mango_groups[&mango_group_name];

    let mango_group_acc = client.get_account(&mgids.mango_group_pk)?;
    let mango_group = MangoGroup::load_from_bytes(mango_group_acc.data.as_slice())?;

    let mut oracle_accs = get_accounts(client, &mgids.oracle_pks);
    let oracle_accs = create_account_infos(oracle_accs.as_mut_slice());
    let oracle_accs = array_ref![oracle_accs.as_slice(), 0, NUM_MARKETS];

    let mut mint_accs = get_accounts(client, &mgids.mint_pks);
    let mint_accs = create_account_infos(mint_accs.as_mut_slice());
    let mint_accs = array_ref![mint_accs.as_slice(), 0, NUM_TOKENS];

    let prices = get_prices(mango_group, oracle_accs)?;
    let names: Vec<&str> = mango_group_name.split("_").collect();
    for i in 0..prices.len() {
        println!("{} {}", names[i], prices[i]);
    }
    Ok(())
}


#[derive(Clone)]
struct ClusterIds {
    pub mango_program_id: Pubkey,
    pub dex_program_id: Pubkey,
    pub mango_groups: HashMap<String, MangoGroupIds>,
    pub oracles: HashMap<String, Pubkey>,
    pub spot_markets: HashMap<String, Pubkey>,
    pub symbols: HashMap<String, Pubkey>
}


impl ClusterIds {
    pub fn load(value: &Value) -> Self {
        let mango_groups: HashMap<String, MangoGroupIds> = value["mango_groups"].as_object().unwrap().iter().map(
            |(k, v)| (k.clone(), MangoGroupIds::load(v))
        ).collect();

        ClusterIds {
            mango_program_id: get_pk(value, "mango_program_id"),
            dex_program_id: get_pk(value, "dex_program_id"),
            mango_groups,
            oracles: get_map_pks(&value["oracles"]),
            spot_markets: get_map_pks(&value["spot_markets"]),
            symbols: get_map_pks(&value["symbols"])
        }
    }

    #[allow(dead_code)]
    pub fn to_json(&self) -> Value {
        json!({"hello": "world"})
    }
}

#[derive(Clone)]
struct MangoGroupIds {
    pub mango_group_pk: Pubkey,
    pub mint_pks: Vec<Pubkey>,
    pub spot_market_pks: Vec<Pubkey>,
    pub vault_pks: Vec<Pubkey>,
    pub oracle_pks: Vec<Pubkey>
}

impl MangoGroupIds {
    pub fn load(value: &Value) -> Self {
        MangoGroupIds {
            mango_group_pk: get_pk(value, "mango_group_pk"),
            mint_pks: get_vec_pks(&value["mint_pks"]),
            spot_market_pks: get_vec_pks(&value["spot_market_pks"]),
            vault_pks: get_vec_pks(&value["vault_pks"]),
            oracle_pks: get_vec_pks(&value["oracle_pks"])
        }
    }
    pub fn get_token_index(&self, token_pk: &Pubkey) -> Option<usize> {
        self.mint_pks.iter().position(|pk| pk == token_pk)
    }
}


pub fn start(opts: Opts) -> Result<()> {
    let client = opts.client();

    match opts.command {
        Command::InitMangoGroup {
            payer,
            ids_path,
            tokens,
            mango_program_id
        } => {
            println!("InitMangoGroup");
            let payer = read_keypair_file(payer.as_str())?;
            let mut ids: Value = serde_json::from_reader(File::open(&ids_path)?)?;
            let cluster_name = opts.cluster.name();
            let cluster_ids = &ids[cluster_name];

            let mango_program_id = if let Some(pk_str) = mango_program_id {
                Pubkey::from_str(pk_str.as_str())?
            } else {
                let mango_program_id = cluster_ids["mango_program_id"].as_str().unwrap();
                Pubkey::from_str(mango_program_id)?
            };

            let dex_program_id = cluster_ids["dex_program_id"].as_str().unwrap();

            let mango_group_pk = create_account_rent_exempt(
                &client, &payer, size_of::<MangoGroup>(), &mango_program_id
            )?.pubkey();
            let (signer_key, signer_nonce) = create_signer_key_and_nonce(&mango_program_id, &mango_group_pk);
            let dex_program_id = Pubkey::from_str(dex_program_id)?;
            assert!(tokens.len() <= NUM_TOKENS && tokens.len() >= 2);

            let symbols = &cluster_ids["symbols"];
            let mint_pks: Vec<Pubkey> = tokens.iter().map(
                |token| get_symbol_pk(symbols, token.as_str())
            ).collect();

            let symbol_pks: HashMap<String, String> = tokens.iter().map(
                |token| (token.clone(), get_symbol_pk(symbols, token.as_str()).to_string())
            ).collect();

            // Create vaults owned by mango program id
            let mut vault_pks = vec![];
            for i in 0..mint_pks.len() {
                let vault_pk = create_token_account(
                    &client, &mint_pks[i], &signer_key, &payer
                )?.pubkey();
                vault_pks.push(vault_pk);
            }

            // Find corresponding spot markets
            let mut spot_market_pks = vec![];
            let mut oracle_pks = vec![];
            let spot_markets = &cluster_ids["spot_markets"];
            let oracles = &cluster_ids["oracles"];
            let quote_symbol = &tokens[tokens.len() - 1].as_str();
            for i in 0..(tokens.len() - 1) {
                let base_symbol = &tokens[i].as_str();
                let market_symbol = format!("{}/{}", base_symbol, quote_symbol);
                spot_market_pks.push(get_symbol_pk(spot_markets, market_symbol.as_str()));
                oracle_pks.push(get_symbol_pk(oracles, market_symbol.as_str()));
            }

            // Send out instruction
            let instruction = init_mango_group(
                &mango_program_id,
                &mango_group_pk,
                &signer_key,
                &dex_program_id,
                mint_pks.as_slice(),
                vault_pks.as_slice(),
                spot_market_pks.as_slice(),
                oracle_pks.as_slice(),
                signer_nonce,
                U64F64::from_num(1.1),
                U64F64::from_num(1.2)
            )?;
            let instructions = vec![instruction];
            let signers = vec![&payer];
            send_instructions(&client, instructions, signers, &payer.pubkey())?;

            // Edit the json file and add the keys associated with this mango group
            let group_name: String = tokens.join("_");
            let vault_pk_strs: Vec<String> = vault_pks.iter().map(|pk| pk.to_string()).collect();
            let spot_market_pk_strs: Vec<String> = spot_market_pks.iter().map(|pk| pk.to_string()).collect();
            let mint_pk_strs: Vec<String> = mint_pks.iter().map(|pk| pk.to_string()).collect();
            let oracle_pk_strs: Vec<String> = oracle_pks.iter().map(|pk| pk.to_string()).collect();

            let group_keys = json!({
                "mango_group_pk": mango_group_pk.to_string(),
                "vault_pks": vault_pk_strs,
                "mint_pks": mint_pk_strs,
                "spot_market_pks": spot_market_pk_strs,
                "oracle_pks": oracle_pk_strs,
                "symbols": symbol_pks
            });

            let ids = ids.as_object_mut().unwrap();
            let cluster_ids = ids.get_mut(cluster_name).unwrap().as_object_mut().unwrap();
            cluster_ids.insert("mango_program_id".to_string(), Value::from(mango_program_id.to_string()));
            let mango_groups = cluster_ids.get_mut("mango_groups").unwrap().as_object_mut().unwrap();
            mango_groups.insert(group_name, group_keys);
            let f = File::create(ids_path.as_str()).unwrap();
            serde_json::to_writer_pretty(&f, &ids).unwrap();
        }
        Command::InitMarginAccount {
            payer,
            ids_path,
            mango_group_name
        } => {

            println!("InitMarginAccount");
            let payer = read_keypair_file(payer.as_str())?;
            let ids: Value = serde_json::from_reader(File::open(&ids_path)?)?;
            let cluster_name = opts.cluster.name();
            let cluster_ids = &ids[cluster_name];
            let mango_program_id = cluster_ids["mango_program_id"].as_str().unwrap();
            let mango_program_id = Pubkey::from_str(mango_program_id)?;

            let group_ids = &cluster_ids["mango_groups"][mango_group_name.as_str()];
            let mango_group_pk = Pubkey::from_str(group_ids["mango_group_pk"].as_str().unwrap())?;

            let margin_account_pk = create_account_rent_exempt(
                &client, &payer, size_of::<MarginAccount>(), &mango_program_id
            )?.pubkey();


            // Send out instruction
            let instruction = init_margin_account(
                &mango_program_id,
                &mango_group_pk,
                &margin_account_pk,
                &payer.pubkey(),
            )?;
            let instructions = vec![instruction];
            let signers = vec![&payer];
            send_instructions(&client, instructions, signers, &payer.pubkey())?;

            println!("MarginAccount created");
            println!("{}", margin_account_pk.to_string());
        }
        Command::Withdraw {
            payer,
            ids_path,
            mango_group_name,
            token_symbol,
            margin_account,
            quantity
        } => {
            println!("Withdraw");
            let payer = read_keypair_file(payer.as_str())?;
            let ids: Value = serde_json::from_reader(File::open(&ids_path)?)?;
            let cluster_name = opts.cluster.name();
            let cluster_ids = &ids[cluster_name];
            let cids = ClusterIds::load(cluster_ids);
            let mgids = &cids.mango_groups[&mango_group_name];

            let mint_pk = cids.symbols[&token_symbol].clone();
            let token_accounts = client.get_token_accounts_by_owner_with_commitment(
                &payer.pubkey(),
                TokenAccountsFilter::Mint(mint_pk),
                CommitmentConfig::confirmed()
            )?.value;
            assert!(token_accounts.len() > 0);
            // Take first token account
            let rka = &token_accounts[0];
            let token_account_pk = Pubkey::from_str(rka.pubkey.as_str())?;

            let mint_acc = client.get_account(&mint_pk)?;
            let mint = spl_token::state::Mint::unpack(mint_acc.data.as_slice())?;

            let margin_account_pk = Pubkey::from_str(margin_account.as_str())?;
            let margin_account = client.get_account(&margin_account_pk)?;
            let margin_account = MarginAccount::load_from_bytes(margin_account.data.as_slice())?;

            let mango_group_acc = client.get_account(&mgids.mango_group_pk)?;
            let mango_group = MangoGroup::load_from_bytes(mango_group_acc.data.as_slice())?;
            let token_index = mango_group.get_token_index(&mint_pk).unwrap();
            let vault_pk: &Pubkey = &mgids.vault_pks[token_index];

            let instruction = withdraw(
                &cids.mango_program_id,
                &mgids.mango_group_pk,
                &margin_account_pk,
                &payer.pubkey(),  // TODO fetch margin account and determine owner
                &token_account_pk,
                vault_pk,
                &mango_group.signer_key,
                &margin_account.open_orders,
                mgids.oracle_pks.as_slice(),
                mgids.mint_pks.as_slice(),
                token_index,
                spl_token::ui_amount_to_amount(quantity, mint.decimals)
            )?;

            let instructions = vec![instruction];
            let signers = vec![&payer];
            send_instructions(&client, instructions, signers, &payer.pubkey())?;

        }
        Command::Borrow {
            payer,
            ids_path,
            mango_group_name,
            margin_account,
            token_symbol,
            quantity
        } => {
            let payer = read_keypair_file(payer.as_str())?;
            let ids: Value = serde_json::from_reader(File::open(&ids_path)?)?;
            let cluster_name = opts.cluster.name();

            let cluster_ids = &ids[cluster_name];
            let mango_group_ids = &cluster_ids["mango_groups"][mango_group_name.as_str()];

            let mango_program_id = get_pk(cluster_ids, "mango_program_id");
            let mango_group_pk = get_pk(mango_group_ids, "mango_group_pk");
            let margin_account_pk = Pubkey::from_str(margin_account.as_str())?;

            let margin_account = client.get_account(&margin_account_pk)?;
            let margin_account = MarginAccount::load_from_bytes(margin_account.data.as_slice())?;
            assert_eq!(margin_account.owner, payer.pubkey());
            let open_orders_pks = margin_account.open_orders;

            let tokens: Vec<&str> = mango_group_name.split("_").collect();
            let quote_symbol = tokens.last().unwrap();
            let oracles = &cluster_ids["oracles"];
            let mut oracle_pks = vec![];
            for i in 0..(tokens.len() - 1) {
                let market_symbol = format!("{}/{}", tokens[i], quote_symbol);
                oracle_pks.push(get_pk(oracles, market_symbol.as_str()));
            }

            let mint_pks = get_vec_pks(&mango_group_ids["mint_pks"]);
            let token_index = tokens.iter().position(|t| *t == token_symbol.as_str()).unwrap();
            let mint_acc = client.get_account(&mint_pks[token_index])?;
            let mint = spl_token::state::Mint::unpack(mint_acc.data.as_slice())?;

            let instruction = borrow(
                &mango_program_id,
                &mango_group_pk,
                &margin_account_pk,
                &margin_account.owner,
                &open_orders_pks,
                oracle_pks.as_slice(),
                mint_pks.as_slice(),
                token_index,
                spl_token::ui_amount_to_amount(quantity, mint.decimals)
            )?;

            let instructions = vec![instruction];
            let signers = vec![&payer];
            send_instructions(&client, instructions, signers, &payer.pubkey())?;

        }
        Command::ConvertAssertionError {
            code
        } => {
            println!("ConvertAssertionError");
            let (line, file_id) = convert_assertion_error(code);
            println!("file {} line {}", file_id, line);
        }
        Command::Deposit {
            payer,
            ids_path,
            mango_group_name,
            token_symbol,
            margin_account,
            quantity
        } => {
            let payer = read_keypair_file(payer.as_str())?;
            let ids: Value = serde_json::from_reader(File::open(&ids_path)?)?;
            let cluster_name = opts.cluster.name();
            let cluster_ids = &ids[cluster_name];
            let mango_group_ids = &cluster_ids["mango_groups"][mango_group_name.as_str()];

            let mango_program_id = cluster_ids["mango_program_id"].as_str().unwrap();
            let mango_program_id = Pubkey::from_str(mango_program_id)?;
            let symbols = &cluster_ids["symbols"];
            let mint_pk = get_symbol_pk(symbols, token_symbol.as_str());

            // Fetch the token wallet for this user
            let token_accounts = client.get_token_accounts_by_owner_with_commitment(
                &payer.pubkey(),
                TokenAccountsFilter::Mint(mint_pk),
                CommitmentConfig::confirmed()
            )?.value;
            assert!(token_accounts.len() > 0);
            // Take first token account
            let rka = &token_accounts[0];
            let token_account_pk = Pubkey::from_str(rka.pubkey.as_str())?;

            let mint_acc = client.get_account(&mint_pk)?;
            let mint = spl_token::state::Mint::unpack(mint_acc.data.as_slice())?;
            let margin_account_pk = Pubkey::from_str(margin_account.as_str())?;

            let mango_group_pk = Pubkey::from_str(mango_group_ids["mango_group_pk"].as_str().unwrap())?;
            let mango_group_acc = client.get_account(&mango_group_pk)?;
            let mango_group = MangoGroup::load_from_bytes(mango_group_acc.data.as_slice())?;
            let token_index = mango_group.get_token_index(&mint_pk).unwrap();
            let vault_pk: &Pubkey = &mango_group.vaults[token_index];

            // Send out instruction
            let instruction = deposit(
                &mango_program_id,
                &mango_group_pk,
                &margin_account_pk,
                &payer.pubkey(),
                &mint_pk,
                &token_account_pk,
                vault_pk,
                spl_token::ui_amount_to_amount(quantity, mint.decimals)
            )?;
            let instructions = vec![instruction];
            let signers = vec![&payer];
            send_instructions(&client, instructions, signers, &payer.pubkey())?;

            println!("Deposited");
            let margin_account_acc = client.get_account(&margin_account_pk)?;
            let margin_account = MarginAccount::load_from_bytes(margin_account_acc.data.as_slice())?;
            let mval: u64 = margin_account.deposits[token_index].to_num();
            println!("{}", mval);
        }

        Command::PrintBs58 {
            keypair,
            filepath
        } => {

            let keypair = read_keypair_file(keypair.as_str())?;
            match filepath {
                None => {
                    println!("{}", keypair.to_base58_string());
                }
                Some(filepath) => {
                    let mut f = File::create(filepath.as_str()).unwrap();
                    write!(&mut f, "{}", keypair.to_base58_string())?;
                }
            }
        }
        Command::RunLiquidator {
            payer,
            ids_path ,
            mango_group_name
        } => {
            println!("RunLiquidator");
            let payer = read_keypair_file(payer.as_str())?;
            let ids: Value = serde_json::from_reader(File::open(&ids_path)?)?;
            let cluster_name = opts.cluster.name();
            let cluster_ids = &ids[cluster_name];
            let cids = ClusterIds::load(cluster_ids);
            let mgids = cids.mango_groups[&mango_group_name].clone();
            run_liquidator(&client, cids, mgids, &payer)?;
        }
        Command::PrintPrices {
            ids_path,
            mango_group_name
        } => {
            println!("PrintPrices");
            let ids: Value = serde_json::from_reader(File::open(&ids_path)?)?;
            let cluster_name = opts.cluster.name();
            let cluster_ids = &ids[cluster_name];
            let cids = ClusterIds::load(cluster_ids);
            print_prices(&client, cids, mango_group_name)?;
        }
        Command::PrintMarginAccountInfo {
            ids_path,
            mango_group_name,
            margin_account
        } => {
            println!("PrintMarginAccountInfo");
            let ids: Value = serde_json::from_reader(File::open(&ids_path)?)?;
            let cluster_name = opts.cluster.name();
            let cluster_ids = &ids[cluster_name];
            let cids = ClusterIds::load(cluster_ids);
            let margin_account_pk = Pubkey::from_str(margin_account.as_str())?;
            let margin_account = client.get_account(&margin_account_pk)?;
            let margin_account = MarginAccount::load_from_bytes(margin_account.data.as_slice())?;

            let mgids = &cids.mango_groups[&mango_group_name];
            let mango_group_acc = client.get_account(&mgids.mango_group_pk)?;
            let mango_group = MangoGroup::load_from_bytes(mango_group_acc.data.as_slice())?;
            let tokens: Vec<&str> = mango_group_name.split("_").collect();

            let mut mint_accs = get_accounts(&client, &mgids.mint_pks);
            let mint_accs = create_account_infos(mint_accs.as_mut_slice());
            let mint_accs = array_ref![mint_accs.as_slice(), 0, NUM_TOKENS];

            let mut oracle_accs = get_accounts(&client, &mgids.oracle_pks);
            let oracle_accs = create_account_infos(oracle_accs.as_mut_slice());
            let oracle_accs = array_ref![oracle_accs.as_slice(), 0, NUM_MARKETS];

            let mut open_orders_accs = get_accounts(&client, &margin_account.open_orders);
            let open_orders_accs = create_account_infos(open_orders_accs.as_mut_slice());
            let open_orders_accs = array_ref![open_orders_accs.as_slice(), 0, NUM_MARKETS];

            let prices = get_prices(mango_group, oracle_accs)?;

            let equity = margin_account.get_equity(mango_group, &prices, open_orders_accs)?;
            println!("MarginAccount: {} | equity: {}", margin_account_pk, equity);
            println!("deposits");
            for i in 0..NUM_TOKENS {
                println!("{} {}", tokens[i], margin_account.deposits[i] * mango_group.indexes[i].deposit);
            }

            // println!("positions");
            // for i in 0..NUM_TOKENS {
            //     println!("{} {}", tokens[i], margin_account.positions[i]);
            // }

            println!("borrows");
            for i in 0..NUM_TOKENS {
                println!("{} {}", tokens[i], margin_account.borrows[i] * mango_group.indexes[i].borrow);
            }

            // total value in quote currency
            // deposits
            // positions
            // borrows
            // val in open orders
        }
        Command::SettleBorrow {
            payer,
            ids_path,
            mango_group_name,
            margin_account,
            token_symbol,
            quantity
        } => {
            let payer = read_keypair_file(payer.as_str())?;
            let ids: Value = serde_json::from_reader(File::open(&ids_path)?)?;
            let cluster_name = opts.cluster.name();
            let cluster_ids = &ids[cluster_name];
            let cids = ClusterIds::load(cluster_ids);
            let mgids = cids.mango_groups[&mango_group_name].clone();

            let margin_account_pk = Pubkey::from_str(margin_account.as_str())?;
            let margin_account = client.get_account(&margin_account_pk)?;
            let margin_account = MarginAccount::load_from_bytes(margin_account.data.as_slice())?;
            assert_eq!(margin_account.owner, payer.pubkey());

            let token_pk = &cids.symbols[&token_symbol];
            let token_i = mgids.get_token_index(token_pk).unwrap();

            let mint_acc = client.get_account(token_pk)?;
            let mint = spl_token::state::Mint::unpack(mint_acc.data.as_slice())?;

            let quantity = match quantity {
                None => unimplemented!(),
                Some(q) => spl_token::ui_amount_to_amount(q, mint.decimals)
            };
            let instruction = settle_borrow(
                &cids.mango_program_id,
                &mgids.mango_group_pk,
                &margin_account_pk,
                &margin_account.owner,
                token_i,
                quantity
            )?;
            let instructions = vec![instruction];
            let signers = vec![&payer];
            send_instructions(&client, instructions, signers, &payer.pubkey())?;

        }
        Command::PlaceOrder { .. } => {}
        Command::SettleFunds { .. } => {}
        Command::CancelOrder { .. } => {}
    }
    Ok(())
}

fn get_pk(json: &Value, name: &str) -> Pubkey {
    Pubkey::from_str(json[name].as_str().unwrap()).unwrap()
}

fn get_symbol_pk(symbols: &Value, symbol: &str) -> Pubkey {
    Pubkey::from_str(symbols[symbol].as_str().unwrap()).unwrap()
}

fn get_vec_pks(value: &Value) -> Vec<Pubkey> {
    value.as_array().unwrap().iter().map(|s| Pubkey::from_str(s.as_str().unwrap()).unwrap()).collect()
}

fn get_map_pks(value: &Value) -> HashMap<String, Pubkey> {
    value.as_object().unwrap().iter().map(
        |(k, v)| (k.clone(), Pubkey::from_str(v.as_str().unwrap()).unwrap())
    ).collect()
}

#[allow(dead_code)]
fn map_of_pks_to_strs(map: HashMap<String, Pubkey>) -> HashMap<String, String> {
    map.iter().map(|(k, v)| (k.clone(), v.to_string())).collect()
}

fn main() {
    let opts = Opts::parse();
    start(opts).unwrap();
}
