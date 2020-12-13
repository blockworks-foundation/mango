use std::str::FromStr;

use anyhow::Result;
use clap::Clap;
use solana_client::rpc_client::RpcClient;
use solana_sdk::commitment_config::CommitmentConfig;
use solana_sdk::pubkey::Pubkey;
use common::{convert_assertion_error, read_keypair_file, create_account_rent_exempt,
             create_signer_key_and_nonce, create_token_account, send_instructions, Cluster};
use std::mem::size_of;
use mango::state::{MangoGroup, NUM_TOKENS};
use solana_sdk::signature::Signer;
use mango::instruction::init_mango_group;
use serde_json::{Value, json};
use std::fs::File;


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
    },
    ConvertAssertionError {
        #[clap(long, short)]
        code: u32,
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
            tokens
        } => {
            println!("InitMangoGroup");
            let payer = read_keypair_file(payer.as_str())?;
            let mut ids: Value = serde_json::from_reader(File::open(&ids_path)?)?;
            let cluster_name = opts.cluster.name();
            let cluster_ids = &ids[cluster_name];
            let mango_program_id = cluster_ids["mango_program_id"].as_str().unwrap();
            let dex_program_id = cluster_ids["dex_program_id"].as_str().unwrap();

            let mango_program_id = Pubkey::from_str(mango_program_id)?;
            let mango_group_pk = create_account_rent_exempt(
                &client, &payer, size_of::<MangoGroup>(), &mango_program_id
            )?.pubkey();
            let (signer_key, signer_nonce) = create_signer_key_and_nonce(&mango_program_id, &mango_group_pk);
            let dex_program_id = Pubkey::from_str(dex_program_id)?;
            assert!(tokens.len() <= NUM_TOKENS && tokens.len() >= 2);

            let mut mint_pks = vec![];
            let symbols = &cluster_ids["symbols"];
            for token in tokens.iter() {  // Find the mint address of each token
                mint_pks.push(Pubkey::from_str(
                    symbols.get(token.as_str()).unwrap().as_str().unwrap())?
                );
            }

            // Create vaults owned by mango program id
            let mut vault_pks = vec![];
            for i in 0..mint_pks.len() {
                let vault_pk = create_token_account(&client, &mint_pks[i],
                                                    &signer_key, &payer)?.pubkey();
                vault_pks.push(vault_pk);
            }

            // Find corresponding spot markets
            let mut spot_market_pks = vec![];
            let spot_markets = &cluster_ids["spot_markets"];
            let quote_symbol = &tokens[tokens.len() - 1].as_str();
            for i in 0..(tokens.len() - 1) {
                let base_symbol = &tokens[i].as_str();
                let market_symbol = format!("{}/{}", base_symbol, quote_symbol);
                let market_pk_str = spot_markets[market_symbol.as_str()].as_str().unwrap();
                spot_market_pks.push(Pubkey::from_str(market_pk_str)?);
            }

            let instruction = init_mango_group(
                &mango_program_id,
                &mango_group_pk,
                &signer_key,
                &dex_program_id,
                mint_pks.as_slice(),
                vault_pks.as_slice(),
                spot_market_pks.as_slice(),
                signer_nonce
            )?;
            let instructions = vec![instruction];
            let signers = vec![&payer];
            send_instructions(&client, instructions, signers, &payer.pubkey())?;

            // Edit the json file and add this mango group
            let group_name: String = tokens.join("_");
            let vault_pk_strs: Vec<String> = vault_pks.iter().map(|pk| pk.to_string()).collect();
            let group_keys = json!({
                "mango_group": mango_group_pk.to_string(),
                "vaults": vault_pk_strs
            });

            let ids = ids.as_object_mut().unwrap();
            let cluster_ids = ids.get_mut(cluster_name).unwrap().as_object_mut().unwrap();
            let mango_groups = cluster_ids.get_mut("mango_groups").unwrap().as_object_mut().unwrap();
            mango_groups.insert(group_name, group_keys);
            let f = File::create(ids_path.as_str()).unwrap();
            serde_json::to_writer_pretty(&f, &ids).unwrap();

        }
        Command::ConvertAssertionError {
            code
        } => {
            println!("ConvertAssertionError");
            let (line, file_id) = convert_assertion_error(code);
            println!("file {} line {}", file_id, line);
        }

    }
    Ok(())
}



fn main() {
    let opts = Opts::parse();
    start(opts).unwrap();
}
