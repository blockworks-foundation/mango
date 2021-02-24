import { IDS, MangoClient, MarginAccount, NUM_TOKENS } from '@mango/client';

import { Account, Connection, PublicKey, TransactionSignature } from '@solana/web3.js';
import * as fs from 'fs';
import { Market } from '@project-serum/serum';
import { NUM_MARKETS } from '@mango/client/lib/layout';
import { sleep } from './utils';

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

  let mangoGroup = await client.getMangoGroup(connection, mangoGroupPk);

  const srmAccountPk = new PublicKey("6utvndL8EEjpwK5QVtguErncQEPVbkuyABmXu6FeygeV")
  // TODO auto fetch
  const marginAccounts = await client.getMarginAccountsForOwner(connection, mangoProgramId, mangoGroup, payer)
  let marginAccount: MarginAccount | undefined = undefined
  let minVal = 0
  for (const ma of marginAccounts) {
    const val = await ma.getValue(connection, mangoGroup)
    if (val >= minVal) {
      minVal = val
      marginAccount = ma
    }
  }
  if (marginAccount == undefined) {
    throw new Error("No margin accounts")
  }
  // await client.depositSrm(connection, mangoProgramId, mangoGroup, marginAccount, payer, srmAccountPk, 10000)

  marginAccount = await client.getMarginAccount(connection, marginAccount.publicKey, dexProgramId)

  const prices = await mangoGroup.getPrices(connection)

  console.log(marginAccount.toPrettyString(mangoGroup, prices), marginAccount.getUiSrmBalance())

  for (const ooa of marginAccount.openOrdersAccounts) {
    if (ooa == undefined) {
      continue
    }
    console.log(ooa.baseTokenFree.toString(), ooa.quoteTokenFree.toString(), ooa.baseTokenTotal.toString(), ooa.quoteTokenTotal.toString())
  }

  const marketIndex = 0  // index for BTC/USDC
  const spotMarket = await Market.load(
    connection,
    mangoGroup.spotMarkets[marketIndex],
    {skipPreflight: true, commitment: 'singleGossip'},
    mangoGroup.dexProgramId
  )
  console.log(prices)

  console.log('placing order')
  // margin short 0.1 BTC
  await client.placeOrder(
    connection,
    mangoProgramId,
    mangoGroup,
    marginAccount,
    spotMarket,
    payer,
    'sell',
    12000,
    0.1
  )

  // marginAccount = await client.getCompleteMarginAccount(connection, marginAccount.publicKey, dexProgramId)

  // await client.settleFunds(
  //   connection,
  //   mangoProgramId,
  //   mangoGroup,
  //   marginAccount,
  //   payer,
  //   spotMarket
  // )

  // await client.settleBorrow(connection, mangoProgramId, mangoGroup, marginAccount, payer, mangoGroup.tokens[2], 5000)
  // await client.settleBorrow(connection, mangoProgramId, mangoGroup, marginAccount, payer, mangoGroup.tokens[0], 1.0)

  await sleep(3000)
  marginAccount = await client.getMarginAccount(connection, marginAccount.publicKey, dexProgramId)
  console.log(marginAccount.toPrettyString(mangoGroup, prices))
  for (const ooa of marginAccount.openOrdersAccounts) {
    if (ooa == undefined) {
      continue
    }
    console.log(ooa.baseTokenFree.toString(), ooa.quoteTokenFree.toString(), ooa.baseTokenTotal.toString(), ooa.quoteTokenTotal.toString())
  }


  const [bids, asks] = await Promise.all([spotMarket.loadBids(connection), spotMarket.loadAsks(connection)])

  await marginAccount.cancelAllOrdersByMarket(
    connection,
    client,
    mangoProgramId,
    mangoGroup,
    spotMarket,
    bids,
    asks,
    payer
  )
  await client.settleFunds(connection, mangoProgramId, mangoGroup, marginAccount, payer, spotMarket)

  await sleep(3000)
  mangoGroup = await client.getMangoGroup(connection, mangoGroupPk)
  marginAccount = await client.getMarginAccount(connection, marginAccount.publicKey, dexProgramId)
  console.log(marginAccount.toPrettyString(mangoGroup, prices))
  // @ts-ignore
  for (const ooa of marginAccount.openOrdersAccounts) {
    if (ooa == undefined) {
      continue
    }
    console.log(ooa.baseTokenFree.toString(), ooa.quoteTokenFree.toString(), ooa.baseTokenTotal.toString(), ooa.quoteTokenTotal.toString())
  }

  console.log(mangoGroup.getUiTotalDeposit(0), mangoGroup.getUiTotalBorrow(0))
  console.log(mangoGroup.getUiTotalDeposit(NUM_MARKETS), mangoGroup.getUiTotalBorrow(NUM_MARKETS))

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
// testing()