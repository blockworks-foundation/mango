import {MangoClient} from "./client";
import {Connection, PublicKey} from "@solana/web3.js";

export { MangoClient, MangoGroup, MarginAccount } from './client';
export { MangoIndexLayout, MarginAccountLayout, MangoGroupLayout } from './layout';

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

  console.log(mangoGroup.accountFlags);
  console.log(prices);
}

main();