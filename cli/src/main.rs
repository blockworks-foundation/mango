use std::str::FromStr;

use anyhow::Result;
use clap::Clap;
use solana_client::rpc_client::RpcClient;
use solana_sdk::commitment_config::CommitmentConfig;
use solana_sdk::pubkey::Pubkey;


#[derive(Clap, Debug)]
pub struct Opts {
    #[clap(default_value = "http://localhost:8899")]
    pub cluster_url: String,
    #[clap(subcommand)]
    pub command: Command,
}

#[derive(Clap, Debug)]
pub enum Command {
    InitMangoGroup {
        #[clap(long)]
        mango_program_id: String,
    }
}

impl Opts {
    fn client(&self) -> RpcClient {
        RpcClient::new_with_commitment(self.cluster_url.clone(), CommitmentConfig::single_gossip())
    }
}

#[allow(unused_variables)]
pub fn start(opts: Opts) -> Result<()> {
    let client = opts.client();

    match opts.command {
        Command::InitMangoGroup {
            mango_program_id,
        } => {
            println!("InitMangoGroup");

            let omega_program_id = Pubkey::from_str(mango_program_id.as_str())?;
        }
    }
    Ok(())
}



fn main() {
    let opts = Opts::parse();
    start(opts).unwrap();
}
