import { MangoClient, MangoGroup } from './client';
import { Account, Connection, PublicKey } from '@solana/web3.js';
import IDS from "./ids.json";
import * as fs from 'fs';
import { Market } from '@project-serum/serum';

export { MangoClient, MangoGroup, MarginAccount } from './client';
export { MangoIndexLayout, MarginAccountLayout, MangoGroupLayout } from './layout';

async function main() {
  const cluster = "devnet";
  const client = new MangoClient();
  const clusterIds = IDS[cluster]

  const connection = new Connection(IDS.cluster_urls[cluster], 'singleGossip')
  const mangoGroupPk = new PublicKey(clusterIds.mango_groups.BTC_ETH_USDC.mango_group_pk);
  const mangoProgramId = new PublicKey(clusterIds.mango_program_id);

  const mangoGroup = await client.getMangoGroup(connection, mangoGroupPk);

  for (const pk of mangoGroup.vaults) {
    const x = await connection.getAccountInfo(pk)
    console.log(x?.data.byteLength)
  }

  const keyPairPath = '/home/dd/.config/solana/id.json'
  const payer = new Account(JSON.parse(fs.readFileSync(keyPairPath, 'utf-8')))

  // TODO auto fetch
  const marginAccountPk = new PublicKey("GU9WHjoUoTvmwgKyA4d7nNeUapT1RcxwK2Gc8EGE1Tmi")
  const marginAccount = await client.getMarginAccount(connection, marginAccountPk)

  const marketIndex = 0  // index for BTC/USDC
  const spotMarket = await Market.load(
    connection,
    mangoGroup.spotMarkets[marketIndex],
    {skipPreflight: true, commitment: 'singleGossip'},
    mangoGroup.dexProgramId
  )

  // margin short 0.1 BTC
  const sig = await client.placeOrder(
    connection,
    mangoProgramId,
    mangoGroup,
    marginAccount,
    spotMarket,
    payer,
    'sell',
    38000,
    0.1
  )
  console.log(sig)
}

main();