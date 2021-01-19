import {MangoClient} from "./client";
import { Account, Connection, PublicKey } from '@solana/web3.js';
import IDS from "./ids.json";
import * as fs from 'fs';

export { MangoClient, MangoGroup, MarginAccount } from './client';
export { MangoIndexLayout, MarginAccountLayout, MangoGroupLayout } from './layout';

async function main() {
  const cluster = "devnet";
  const client = new MangoClient();
  const clusterIds = IDS[cluster]

  const connection = new Connection(IDS.cluster_urls[cluster], 'singleGossip')
  const mangoGroupPk = new PublicKey(IDS[cluster].mango_groups.BTC_ETH_USDC.mango_group_pk);
  const mangoProgramId = new PublicKey(IDS[cluster].mango_program_id);

  const mangoGroup = await client.getMangoGroup(connection, mangoProgramId, mangoGroupPk);

  const keyPairPath = '/home/dd/.config/solana/id.json'
  const payer = new Account(JSON.parse(fs.readFileSync(keyPairPath, 'utf-8')))

  const marginAccountPk = await client.initMarginAccount(
    connection,
    mangoProgramId,
    new PublicKey(clusterIds.dex_program_id),
    mangoGroup,
    payer
  )
  console.log(marginAccountPk.toBase58())

}

main();