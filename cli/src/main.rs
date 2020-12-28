use std::str::FromStr;

use anyhow::Result;
use clap::Clap;
use solana_client::rpc_client::RpcClient;
use solana_sdk::commitment_config::CommitmentConfig;
use solana_sdk::pubkey::{Pubkey};
use common::{convert_assertion_error, read_keypair_file, create_account_rent_exempt,
             create_signer_key_and_nonce, create_token_account, send_instructions, Cluster};
use std::mem::size_of;
use mango::state::{MangoGroup, NUM_TOKENS, MarginAccount, Loadable};
use solana_sdk::signature::Signer;
use mango::instruction::{init_mango_group, init_margin_account, deposit, borrow};
use serde_json::{Value, json};
use std::fs::File;
use solana_client::rpc_request::TokenAccountsFilter;
use solana_sdk::program_pack::Pack;
use fixed::types::U64F64;
use std::io::Write;

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
        token_symbol: String,
        #[clap(long, short)]
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
    ConvertAssertionError {
        #[clap(long, short)]
        code: u32,
    },
    PrintBs58 {
        #[clap(long, short)]
        keypair: String,
        #[clap(long, short)]
        filepath: String,
    }
}

impl Opts {
    fn client(&self) -> RpcClient {
        RpcClient::new_with_commitment(self.cluster.url().to_string(),
                                       CommitmentConfig::single_gossip())
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
            let group_keys = json!({
                "mango_group_pk": mango_group_pk.to_string(),
                "vault_pks": vault_pk_strs,
                "mint_pks": mint_pk_strs,
                "spot_market_pks": spot_market_pk_strs,

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
            let dex_program_id = cluster_ids["dex_program_id"].as_str().unwrap();

            let mango_program_id = Pubkey::from_str(mango_program_id)?;
            let dex_program_id = Pubkey::from_str(dex_program_id)?;

            let group_ids = &cluster_ids["mango_groups"][mango_group_name.as_str()];
            let mango_group_pk = Pubkey::from_str(group_ids["mango_group_pk"].as_str().unwrap())?;
            let spot_market_pks = get_vec_pks(&group_ids["spot_market_pks"]);

            let margin_account_pk = create_account_rent_exempt(
                &client, &payer, size_of::<MarginAccount>(), &mango_program_id
            )?.pubkey();

            let mut open_orders_pks = vec![];
            for _ in 0..spot_market_pks.len() {
                let open_orders_pk = create_account_rent_exempt(
                    &client,
                    &payer,
                    size_of::<serum_dex::state::OpenOrders>() + 12,  // add size of padding
                    &dex_program_id
                )?.pubkey();

                open_orders_pks.push(open_orders_pk);
            }


            // Send out instruction
            let instruction = init_margin_account(
                &mango_program_id,
                &mango_group_pk,
                &margin_account_pk,
                &payer.pubkey(),
                &open_orders_pks
            )?;
            let instructions = vec![instruction];
            let signers = vec![&payer];
            send_instructions(&client, instructions, signers, &payer.pubkey())?;

            println!("MarginAccount created");
            println!("{}", margin_account_pk.to_string());
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
                CommitmentConfig::single_gossip()
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
        Command::Withdraw {
            ..
        } => {
            unimplemented!()
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

            let o = client.get_account(&oracle_pks[0])?;
            println!("{} {:?}", o.executable, o.data.len());

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

        Command::PrintBs58 {
            keypair,
            filepath
        } => {

            let keypair = read_keypair_file(keypair.as_str())?;
            let mut f = File::create(filepath.as_str()).unwrap();
            write!(&mut f, "{}", keypair.to_base58_string())?;
            // println!("{}", keypair.to_base58_string())
        }
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

fn main() {
    let opts = Opts::parse();
    start(opts).unwrap();
}
