import {MangoClient} from "./client";
import {Connection, PublicKey} from "@solana/web3.js";
import IDS from "./ids.json";

export { MangoClient, MangoGroup, MarginAccount } from './client';
export { MangoIndexLayout, MarginAccountLayout, MangoGroupLayout } from './layout';

async function main() {
  const cluster = "devnet";
  const client = new MangoClient();

  const connection = new Connection(IDS.cluster_urls[cluster], 'singleGossip')
  const mangoGroupPk = new PublicKey(IDS[cluster].mango_groups.BTC_ETH_USDC.mango_group_pk);
  const mangoProgramId = new PublicKey(IDS[cluster].mango_program_id);

  const mangoGroup = await client.getMangoGroup(connection, mangoProgramId, mangoGroupPk);
  const prices = await mangoGroup.getPrices(connection);

  console.log(prices);
}

main();