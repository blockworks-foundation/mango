import { MangoClient } from "@mango/client-ts";

import {
  Connection,
  PublicKey
} from "@solana/web3.js";
import IDS from "./ids.json";

async function main() {
  const client = new MangoClient();
  const running = true;
  const cluster = "devnet";

  const clusterUrl = IDS.cluster_urls.devnet;
  const connection = new Connection(clusterUrl, 'singleGossip')
  const mangoGroupPk = new PublicKey(IDS.devnet.mango_groups.BTC_ETH_USDC.mango_group_pk);
  const mangoProgramId = new PublicKey(IDS.devnet.mango_program_id);

  const mangoGroup = await client.getMangoGroup(connection, mangoProgramId, mangoGroupPk);
  const marginAccounts = await client.getAllMarginAccounts(connection, mangoProgramId, mangoGroupPk);
  const prices = await mangoGroup.getPrices(connection);
  console.log(prices);

  console.log(mangoGroup.accountFlags);


  // get all outstanding margin accounts
  // const marginAccounts = await client.getAllMarginAccounts(connection, mangoProgramId, mangoGroupPk);
  //
  // // get current prices
  // const prices = client.getPrices(connection, mangoProgramId, null);
  // for (const account of marginAccounts) {
  //   // go to each margin account
  //
  //
  //   // get collateral ratio
  //
  // }
}

main();