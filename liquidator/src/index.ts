import { MangoClient } from "@mango/client-ts";

import {
  Connection,
  PublicKey
} from "@solana/web3.js";
import { BN } from 'bn.js';
import IDS from "./ids.json";
import { MangoGroupLayout } from '@mango/client-ts/lib/layouts';

async function main() {
  const client = new MangoClient();
  const running = true;
  const cluster = "devnet";

  const clusterUrl = IDS.cluster_urls.devnet;
  const connection = new Connection(clusterUrl, 'singleGossip')
  const mangoGroupPk = new PublicKey(IDS.devnet.mango_groups.BTC_ETH_USDC.mango_group_pk);
  const mangoProgramId = new PublicKey(IDS.devnet.mango_program_id);

  const mangoGroupInfo = await connection.getAccountInfo(mangoGroupPk);
  // @ts-ignore
  const mangoGroup = MangoGroupLayout.decode(mangoGroupInfo.data);

  console.log(mangoGroup.indexes[0].borrow);
  console.log(mangoGroup.maint_coll_ratio);
  console.log(mangoGroup.tokens[0].toBase58());
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