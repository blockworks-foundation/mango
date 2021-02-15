import { IDS, MangoClient, NUM_TOKENS } from '@mango/client';

import { Account, Connection, PublicKey, TransactionSignature } from '@solana/web3.js';
import * as fs from 'fs';
import { Market } from '@project-serum/serum';
import { NUM_MARKETS } from '@mango/client/lib/layout';
import { nativeToUi } from '@mango/client/lib/utils';

async function main() {
  const client = new MangoClient()
  const cluster = 'devnet'
  const connection = new Connection(IDS.cluster_urls[cluster], 'singleGossip')

  // The address of the Mango Program on the blockchain
  const programId = new PublicKey(IDS[cluster].mango_program_id)
  // The address of the serum dex program on the blockchain: https://github.com/project-serum/serum-dex
  const dexProgramId = new PublicKey(IDS[cluster].dex_program_id)

  // Address of the MangoGroup
  const mangoGroupPk = new PublicKey(IDS[cluster].mango_groups.BTC_ETH_USDC.mango_group_pk)


  // TODO fetch these automatically
  const keyPairPath = '/home/dd/.config/solana/id.json'
  const payer = new Account(JSON.parse(fs.readFileSync(keyPairPath, 'utf-8')))
  const tokenWallets = [
    new PublicKey("HLoPtihB8oETm1kkTpx17FEnXm7afQdS4hojTNvbg3Rg"),
    new PublicKey("8ASVNBAo94RnJCABYybnkJnXGpBHan2svW3pRsKdbn7s"),
    new PublicKey("GBBtcVE7WA8qdrHyhWTZkYDaz71EVHsg7wVaca9iq9xs")
  ]

  let mangoGroup = await client.getMangoGroup(connection, mangoGroupPk)

  // load all markets
  const markets = await Promise.all(mangoGroup.spotMarkets.map(
    (pk) => Market.load(connection, pk, {skipPreflight: true, commitment: 'singleGossip'}, dexProgramId)
  ))
  const sleepTime = 10000
  // TODO handle failures in any of the steps
  while (true) {
    mangoGroup = await client.getMangoGroup(connection, mangoGroupPk)
    const marginAccounts = await client.getAllMarginAccounts(connection, programId, mangoGroupPk)

    await Promise.all(marginAccounts.map((ma) => (ma.loadOpenOrders(connection, dexProgramId))))
    const prices = await mangoGroup.getPrices(connection)  // TODO put this on websocket as well

    for (let ma of marginAccounts) {  // parallelize this if possible
      const assetsVal = ma.getAssetsVal(mangoGroup, prices)
      const liabsVal = ma.getLiabsVal(mangoGroup, prices)
      console.log(ma.toPrettyString(mangoGroup), ma.owner.toBase58())

      if (liabsVal === 0) {
        continue
      }
      const collRatio = assetsVal / liabsVal
      console.log(assetsVal, liabsVal, collRatio)


      // if (collRatio >= mangoGroup.maintCollRatio) {
      //   continue
      // }
      //
      // const deficit = liabsVal * mangoGroup.initCollRatio - assetsVal
      // console.log('liquidatable', deficit)
      //
      // // handle undercoll case separately
      // if (collRatio < 1) {
      //   // Need to make sure there are enough funds in MangoGroup to be compensated fully
      // }
      //
      // // determine how much to deposit to get the account above init coll ratio
      // await client.liquidate(connection, programId, mangoGroup, ma, payer, tokenWallets, [0, 0, deficit * 1.01])
      // ma = await client.getCompleteMarginAccount(connection, ma.publicKey, dexProgramId)
      //
      // console.log('liquidation success')
      // console.log(ma.toPrettyString(mangoGroup))

      // Cancel all open orders
      const bidsPromises = markets.map((market) => market.loadBids(connection))
      const asksPromises = markets.map((market) => market.loadAsks(connection))
      const books = await Promise.all(bidsPromises.concat(asksPromises))
      const bids = books.slice(0, books.length / 2)
      const asks = books.slice(books.length / 2, books.length)

      const cancelProms: Promise<TransactionSignature[]>[] = []
      for (let i = 0; i < NUM_MARKETS; i++) {
        cancelProms.push(ma.cancelAllOrdersByMarket(connection, client, programId, mangoGroup, markets[i], bids[i], asks[i], payer))
      }

      await Promise.all(cancelProms)
      console.log('all orders cancelled')

      await client.settleAll(connection, programId, mangoGroup, ma, markets, payer)
      console.log('settleAll complete')
      ma = await client.getCompleteMarginAccount(connection, ma.publicKey, dexProgramId)

      // sort non-quote currency assets by value
      const assets = ma.getAssets(mangoGroup)
      const liabs = ma.getLiabs(mangoGroup)

      const netValues: [number, number][] = []

      for (let i = 0; i < NUM_TOKENS - 1; i++) {
        netValues.push([i, (assets[i] - liabs[i]) * prices[i]])
      }
      netValues.sort((a, b) => (b[1] - a[1]))

      // A neg, B neg, C pos
      // buy A, buy B
      // A pos, B pos, C pos
      // sell A, sell B
      // A neg, B pos, C neg
      // sell B (C should now be pos), buy A
      // A pos, B neg, C pos
      // sell A (C should have enough to buy back B)
      // A pos, B neg, C neg
      for (let i = 0; i < NUM_TOKENS - 1; i++) {
        const marketIndex = netValues[i][0]
        const market = markets[marketIndex]

        if (netValues[i][1] > 0) { // sell to close
          const price = prices[marketIndex] * 0.95
          const size = assets[marketIndex]
          console.log(`Sell to close ${marketIndex} ${size}`)
          await client.placeOrder(connection, programId, mangoGroup, ma, market, payer, 'sell', price, size, 'limit')

        } else if (netValues[i][1] < 0) { // buy to close
          const price = prices[marketIndex] * 1.05  // buy at up to 5% higher than oracle price
          const size = liabs[marketIndex]
          console.log(mangoGroup.getUiTotalDeposit(NUM_MARKETS), mangoGroup.getUiTotalBorrow(NUM_MARKETS))
          console.log(ma.getUiDeposit(mangoGroup, NUM_MARKETS), ma.getUiBorrow(mangoGroup, NUM_MARKETS))
          console.log(`Buy to close ${marketIndex} ${size}`)
          await client.placeOrder(connection, programId, mangoGroup, ma, market, payer, 'buy', price, size, 'limit')
        }
      }

      await client.settleAll(connection, programId, mangoGroup, ma, markets, payer)
      console.log('settleAll complete')
      ma = await client.getCompleteMarginAccount(connection, ma.publicKey, dexProgramId)

      console.log('Liquidation process complete\n', ma.toPrettyString(mangoGroup))

      console.log('withdrawing USDC')
    }

    await sleep(sleepTime)
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
  const marginAccountPk = new PublicKey("6qiX5n1TTiv1R8GqAZUk1BaP7qFaPow6MoAqX6rrgEcg")
  let marginAccount = await client.getCompleteMarginAccount(connection, marginAccountPk, dexProgramId)

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
  //   100,
  //   0.5
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

  marginAccount = await client.getCompleteMarginAccount(connection, marginAccountPk, dexProgramId)

  console.log(marginAccount.toPrettyString(mangoGroup))
}


function sleep(ms) {
  return new Promise(resolve => setTimeout(resolve, ms));
}


async function testing() {
  const client = new MangoClient()
  const cluster = 'devnet'
  const connection = new Connection(IDS.cluster_urls[cluster], 'singleGossip')

  // The address of the Mango Program on the blockchain
  const programId = new PublicKey(IDS[cluster].mango_program_id)
  // The address of the serum dex program on the blockchain: https://github.com/project-serum/serum-dex
  const dexProgramId = new PublicKey(IDS[cluster].dex_program_id)

  // Address of the MangoGroup
  const mangoGroupPk = new PublicKey(IDS[cluster].mango_groups.BTC_ETH_USDC.mango_group_pk)


  // TODO fetch these automatically
  const keyPairPath = '/home/dd/.config/solana/id.json'
  const payer = new Account(JSON.parse(fs.readFileSync(keyPairPath, 'utf-8')))

  let mangoGroup = await client.getMangoGroup(connection, mangoGroupPk)
  const totalBorrow = mangoGroup.getUiTotalBorrow(0)
  const totalDeposit = mangoGroup.getUiTotalDeposit(0)

  // save it in the database

  mangoGroup = await client.getMangoGroup(connection, mangoGroupPk)

  await sleep(5000)
}
// setupMarginAccounts()
// main()
testing()