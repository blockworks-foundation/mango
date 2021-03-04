// TODO make sure funds in liquidatable account can actually be withdrawn
//    -- can't be withdrawn if total deposits == total_borrows

import {
  getMultipleAccounts,
  IDS,
  MangoClient,
  MangoGroup,
  MarginAccount,
  MarginAccountLayout, nativeToUi,
  NUM_TOKENS, parseTokenAccountData,
  findLargestTokenAccountForOwner,
} from '@mango/client';
import { Account, Connection, LAMPORTS_PER_SOL, PublicKey, TransactionSignature } from '@solana/web3.js';
import fs from 'fs';
import { Market, OpenOrders } from '@project-serum/serum';
import { NUM_MARKETS } from '@mango/client/lib/layout';
import { getUnixTs, sleep } from './utils';
import { homedir } from 'os'
import { TOKEN_PROGRAM_ID } from '@solana/spl-token';


async function drainAccount(
  client: MangoClient,
  connection: Connection,
  programId: PublicKey,
  mangoGroup: MangoGroup,
  ma: MarginAccount,
  markets: Market[],
  payer: Account,
  prices: number[],
  usdWallet: PublicKey
) {
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

  console.log()
  await client.settleAll(connection, programId, mangoGroup, ma, markets, payer)
  console.log('settleAll complete')
  await sleep(2000)
  ma = await client.getMarginAccount(connection, ma.publicKey, mangoGroup.dexProgramId)

  // sort non-quote currency assets by value
  const assets = ma.getAssets(mangoGroup)
  const liabs = ma.getLiabs(mangoGroup)

  const netValues: [number, number][] = []

  for (let i = 0; i < NUM_TOKENS - 1; i++) {
    netValues.push([i, (assets[i] - liabs[i]) * prices[i]])
  }
  netValues.sort((a, b) => (b[1] - a[1]))

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
  ma = await client.getMarginAccount(connection, ma.publicKey, mangoGroup.dexProgramId)
  console.log('Liquidation process complete\n', ma.toPrettyString(mangoGroup, prices))

  console.log('Withdrawing USD')
  await client.withdraw(connection, programId, mangoGroup, ma, payer, mangoGroup.tokens[NUM_TOKENS-1], usdWallet, ma.getUiDeposit(mangoGroup, NUM_TOKENS-1) * 0.999)

}

async function runLiquidator() {
  const client = new MangoClient()
  const cluster = process.env.CLUSTER || 'mainnet-beta'
  const group_name = 'BTC_ETH_USDT'
  const clusterUrl = process.env.CLUSTER_URL || IDS.cluster_urls[cluster]
  const connection = new Connection(clusterUrl, 'singleGossip')

  // The address of the Mango Program on the blockchain
  const programId = new PublicKey(IDS[cluster].mango_program_id)

  // The address of the serum dex program on the blockchain: https://github.com/project-serum/serum-dex
  const dexProgramId = new PublicKey(IDS[cluster].dex_program_id)

  // Address of the MangoGroup
  const mangoGroupPk = new PublicKey(IDS[cluster].mango_groups[group_name].mango_group_pk)

  // liquidator's keypair
  const keyPairPath = process.env.KEYPAIR || homedir() + '/.config/solana/id.json'
  const payer = new Account(JSON.parse(fs.readFileSync(keyPairPath, 'utf-8')))

  let mangoGroup = await client.getMangoGroup(connection, mangoGroupPk)

  const tokenWallets = (await Promise.all(
    mangoGroup.tokens.map(
      (mint) => findLargestTokenAccountForOwner(connection, payer.publicKey, mint).then(
        (response) => response.publicKey
      )
    )
  ))

  // load all markets
  const markets = await Promise.all(mangoGroup.spotMarkets.map(
    (pk) => Market.load(connection, pk, {skipPreflight: true, commitment: 'singleGossip'}, dexProgramId)
  ))
  const sleepTime = 5000
  // TODO handle failures in any of the steps
  // Find a way to get all margin accounts without querying fresh--get incremental updates to margin accounts

  while (true) {
    try {
      mangoGroup = await client.getMangoGroup(connection, mangoGroupPk)
      const marginAccounts = await client.getAllMarginAccounts(connection, programId, mangoGroup)
      let prices = await mangoGroup.getPrices(connection)  // TODO put this on websocket as well

      console.log(prices)

      const tokenAccs = await getMultipleAccounts(connection, mangoGroup.vaults)
      const vaultValues = tokenAccs.map(
        (a, i) => nativeToUi(parseTokenAccountData(a.accountInfo.data).amount, mangoGroup.mintDecimals[i])
      )
      console.log(vaultValues)

      for (let ma of marginAccounts) {  // parallelize this if possible

        let liquidated = false
        while (true) {
          try {
            const assetsVal = ma.getAssetsVal(mangoGroup, prices)
            const liabsVal = ma.getLiabsVal(mangoGroup, prices)

            if (liabsVal === 0) {
              break
            }
            const collRatio = assetsVal / liabsVal

            if (collRatio >= mangoGroup.maintCollRatio) {
              break
            }

            const deficit = liabsVal * mangoGroup.initCollRatio - assetsVal
            console.log('liquidatable', deficit)
            console.log(ma.toPrettyString(mangoGroup, prices), '\n')
            await client.liquidate(connection, programId, mangoGroup, ma, payer,
              tokenWallets, [0, 0, deficit * 1.01])
            liquidated = true
            break
          } catch (e) {
            if (!e.timeout) {
              throw e
            } else {
              await sleep(1000)
              prices = await mangoGroup.getPrices(connection)
            }
          }
        }
        if (liquidated) {
          console.log('liquidation success')
          console.log(ma.toPrettyString(mangoGroup, prices))

          while (true) {
            try {
              ma = await client.getMarginAccount(connection, ma.publicKey, dexProgramId)
              await drainAccount(client, connection, programId, mangoGroup, ma, markets, payer, prices, tokenWallets[NUM_TOKENS-1])
              console.log('Account drain success')
              break
            } catch (e) {
              console.log('Failed while draining account. Trying again in 1s')
              await sleep(1000)
            }
          }
        }
      }

    } catch (e) {
      console.log(e)
    } finally {
      await sleep(sleepTime)
    }
  }
}

runLiquidator()
