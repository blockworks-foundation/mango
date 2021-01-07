const mango = require('@mango/client-ts');
import {
  Connection,
  PublicKey
} from "@solana/web3.js";

// @ts-ignore
import IDS from "./ids.json";

async function main() {
  const m = new mango.MangoClient();
  m.greet();
  let running = true;
  let cluster = "devnet";

  let clusterUrl = IDS.cluster_urls.devnet;
  let connection = new Connection(clusterUrl, 'singleGossip')
  let mangoGroupPk = new PublicKey(IDS.devnet.mango_groups.BTC_ETH_USDC.mango_group_pk);
  let mangoProgramId = new PublicKey(IDS.devnet.mango_program_id);

  // get all outstanding margin accounts
  let marginAccounts = await m.getAllMarginAccounts(connection, mangoProgramId, mangoGroupPk);

  // get current prices
  let prices = m.getPrices(connection, mangoProgramId, null);
  for (let account of marginAccounts) {
    // go to each margin account
    let marginAccount = account.account;

    // get collateral ratio
    
  }
}

main();