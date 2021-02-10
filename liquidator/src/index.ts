import { IDS, MangoClient } from '@mango/client';

import { Account, Connection, PublicKey } from '@solana/web3.js';
import * as fs from 'fs';
import { Market } from '@project-serum/serum';

async function main() {
  const client = new MangoClient()
  const cluster = 'devnet'
  const connection = new Connection(IDS.cluster_urls[cluster], 'singleGossip')
  const programId = new PublicKey(IDS[cluster].mango_program_id)
  const dexProgramId = new PublicKey(IDS[cluster].dex_program_id)
  const mangoGroupPk = new PublicKey(IDS[cluster].mango_groups.BTC_ETH_USDC.mango_group_pk)

  const mangoGroup = await client.getMangoGroup(connection, mangoGroupPk)
  const marginAccounts = await client.getAllMarginAccounts(connection, programId, mangoGroupPk)

  // TODO fetch these automatically
  const keyPairPath = '/home/dd/.config/solana/id.json'
  const payer = new Account(JSON.parse(fs.readFileSync(keyPairPath, 'utf-8')))
  const tokenWallets = [
    new PublicKey("HLoPtihB8oETm1kkTpx17FEnXm7afQdS4hojTNvbg3Rg"),
    new PublicKey("8ASVNBAo94RnJCABYybnkJnXGpBHan2svW3pRsKdbn7s"),
    new PublicKey("GBBtcVE7WA8qdrHyhWTZkYDaz71EVHsg7wVaca9iq9xs")
  ]

  // fetch open orders
  for (const ma of marginAccounts) {  // TODO load with websocket
    await ma.loadOpenOrders(connection, dexProgramId)
  }
  const prices = await mangoGroup.getPrices(connection)  // TODO put this on websocket as well
  for (const ma of marginAccounts) {
    console.log(ma.toPrettyString(mangoGroup), ma.owner.toBase58())
    const assetsVal = ma.getAssetsVal(mangoGroup, prices)
    const liabsVal = ma.getLiabsVal(mangoGroup, prices)
    if (liabsVal === 0) {
      continue
    }
    const collRatio = assetsVal / liabsVal
    console.log(assetsVal, liabsVal, collRatio)

    if (collRatio < mangoGroup.maintCollRatio) {

      const deficit = liabsVal * mangoGroup.initCollRatio - assetsVal
      console.log('liqdatable', deficit)

      // handle undercoll case separately
      if (collRatio < 1) {
        // Need to make sure there are enough funds in MangoGroup to be compensated fully
      }


      // determine how much to deposit to get the account above init coll ratio

      await client.liquidate(connection, programId, mangoGroup, ma, payer, tokenWallets, [0, 0, deficit * 1.01])

      const updatedMa = await client.getMarginAccount(connection, ma.publicKey)
      await updatedMa.loadOpenOrders(connection, dexProgramId)
      console.log('liquidation success')
      console.log(updatedMa.toPrettyString(mangoGroup))
      // after depositing and receiving success, cancel outstanding open orders
      // place new orders to liquidate all positions into USDC
      // call settleBorrow on every open borrow
      // transfer assets out to own marginAccount

    }

  }

}


async function setupMarginAccounts() {
  const keyPairPath = '/home/dd/.config/solana/id.json'
  const payer = new Account(JSON.parse(fs.readFileSync(keyPairPath, 'utf-8')))
  const cluster = "devnet";
  const client = new MangoClient();
  const clusterIds = IDS[cluster]

  const connection = new Connection(IDS.cluster_urls[cluster], 'singleGossip')
  const mangoGroupPk = new PublicKey(clusterIds.mango_groups.BTC_ETH_USDC.mango_group_pk);
  const mangoProgramId = new PublicKey(clusterIds.mango_program_id);
  const dexProgramId = new PublicKey(clusterIds.dex_program_id)

  const mangoGroup = await client.getMangoGroup(connection, mangoGroupPk);

  // TODO auto fetch
  const marginAccountPk = new PublicKey("3axUjRCrUtFaLeZuZ7obPENf3mWgA1LJVByWR7jbXqBR")
  let marginAccount = await client.getMarginAccount(connection, marginAccountPk)
  await marginAccount.loadOpenOrders(connection, dexProgramId)

  console.log(marginAccount.toPrettyString(mangoGroup))

  const marketIndex = 0  // index for BTC/USDC
  const spotMarket = await Market.load(
    connection,
    mangoGroup.spotMarkets[marketIndex],
    {skipPreflight: true, commitment: 'singleGossip'},
    mangoGroup.dexProgramId
  )
  const prices = await mangoGroup.getPrices(connection)
  console.log(prices)

  // // margin short 0.1 BTC
  // await client.placeOrder(
  //   connection,
  //   mangoProgramId,
  //   mangoGroup,
  //   marginAccount,
  //   spotMarket,
  //   payer,
  //   'sell',
  //   30000,
  //   0.1
  // )

  await spotMarket.matchOrders(connection, payer, 10)

  await client.settleFunds(
    connection,
    mangoProgramId,
    mangoGroup,
    marginAccount,
    payer,
    spotMarket
  )
  //
  // await client.settleBorrow(connection, mangoProgramId, mangoGroup, marginAccount, payer, mangoGroup.tokens[2], 5000)
  // await client.settleBorrow(connection, mangoProgramId, mangoGroup, marginAccount, payer, mangoGroup.tokens[0], 1.0)

  marginAccount = await client.getMarginAccount(connection, marginAccountPk)
  await marginAccount.loadOpenOrders(connection, dexProgramId)

  console.log(marginAccount.toPrettyString(mangoGroup))
}

// setupMarginAccounts()
main()