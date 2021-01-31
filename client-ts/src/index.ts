import { MangoClient, MangoGroup } from './client';
import { Account, Connection, PublicKey } from '@solana/web3.js';
import * as fs from 'fs';
import { Market } from '@project-serum/serum';
import { NUM_TOKENS } from './layout';

export { MangoClient, MangoGroup, MarginAccount } from './client';
export { MangoIndexLayout, MarginAccountLayout, MangoGroupLayout } from './layout';
export { NUM_TOKENS } from './layout';

import IDS from "./ids.json";
export { IDS }

//
// async function main() {
//   const cluster = "devnet";
//   const client = new MangoClient();
//   const clusterIds = IDS[cluster]
//
//   const connection = new Connection(IDS.cluster_urls[cluster], 'singleGossip')
//   const mangoGroupPk = new PublicKey(clusterIds.mango_groups.BTC_ETH_USDC.mango_group_pk);
//   const mangoProgramId = new PublicKey(clusterIds.mango_program_id);
//
//   const mangoGroup = await client.getMangoGroup(connection, mangoGroupPk);
//
//   const keyPairPath = '/home/dd/.config/solana/id.json'
//   const payer = new Account(JSON.parse(fs.readFileSync(keyPairPath, 'utf-8')))
//
//   // TODO auto fetch
//   const marginAccountPk = new PublicKey("58hhPAgRgk1BHM1UkvYnJfxpMcoyi3DSoKnkwxuFe47")
//   let marginAccount = await client.getMarginAccount(connection, marginAccountPk)
//
//   console.log(marginAccount.toPrettyString(mangoGroup))
//
//   const marketIndex = 0  // index for BTC/USDC
//   const spotMarket = await Market.load(
//     connection,
//     mangoGroup.spotMarkets[marketIndex],
//     {skipPreflight: true, commitment: 'singleGossip'},
//     mangoGroup.dexProgramId
//   )
//
//   // margin short 0.1 BTC
//   await client.placeOrder(
//     connection,
//     mangoProgramId,
//     mangoGroup,
//     marginAccount,
//     spotMarket,
//     payer,
//     'sell',
//     30000,
//     0.1
//   )
//
//   await spotMarket.matchOrders(connection, payer, 10)
//
//   await client.settleFunds(
//     connection,
//     mangoProgramId,
//     mangoGroup,
//     marginAccount,
//     payer,
//     spotMarket
//   )
//
//   await client.settleBorrow(connection, mangoProgramId, mangoGroup, marginAccount, payer, mangoGroup.tokens[2], 5000)
//   await client.settleBorrow(connection, mangoProgramId, mangoGroup, marginAccount, payer, mangoGroup.tokens[0], 1.0)
//
//   marginAccount = await client.getMarginAccount(connection, marginAccount.publicKey)
//   console.log(marginAccount.toPrettyString(mangoGroup))
// }
//
// async function testAll() {
//   const cluster = "devnet"
//   const client = new MangoClient()
//   const clusterIds = IDS[cluster]
//
//   const connection = new Connection(IDS.cluster_urls[cluster], 'singleGossip')
//   const mangoGroupPk = new PublicKey(clusterIds.mango_groups.BTC_ETH_USDC.mango_group_pk);
//   const mangoProgramId = new PublicKey(clusterIds.mango_program_id);
//
//   const mangoGroup = await client.getMangoGroup(connection, mangoGroupPk);
//
//   const keyPairPath = '/home/dd/.config/solana/id.json'
//   const payer = new Account(JSON.parse(fs.readFileSync(keyPairPath, 'utf-8')))
//
//   // TODO auto fetch
//   const marginAccounts = await client.getMarginAccountsForOwner(connection, mangoProgramId, mangoGroup, payer)
//   for (const x of marginAccounts) {
//     // get value of each margin account and select highest
//
//     console.log(x.publicKey.toBase58())
//   }
//
// }
//
//
//
//
// testAll()
