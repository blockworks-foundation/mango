import {
  findLargestTokenAccountForOwner,
  IDS,
  MangoClient,
  MangoGroup,
  MarginAccount,
  MarginAccountLayout, nativeToUi,
  NUM_TOKENS,
} from '@mango/client';
import {
  Account,
  Connection, LAMPORTS_PER_SOL,
  PublicKey,
  SYSVAR_RENT_PUBKEY,
  Transaction,
  TransactionInstruction, TransactionSignature,
} from '@solana/web3.js';
import fs from 'fs';
import { getUnixTs, sleep } from './utils';
import { createAccountInstruction, getFilteredProgramAccounts } from '@mango/client/lib/utils';
import { encodeMangoInstruction, NUM_MARKETS } from '@mango/client/lib/layout';
import { Token, MintLayout, AccountLayout, TOKEN_PROGRAM_ID } from '@solana/spl-token';
import { homedir } from 'os';
import { Market } from '@project-serum/serum';


async function genMarginAccounts() {
  const client = new MangoClient()
  const cluster = 'devnet'
  const connection = new Connection(IDS.cluster_urls[cluster], 'singleGossip')

  // The address of the Mango Program on the blockchain
  const programId = new PublicKey(IDS[cluster].mango_program_id)
  // The address of the serum dex program on the blockchain: https://github.com/project-serum/serum-dex
  const dexProgramId = new PublicKey(IDS[cluster].dex_program_id)

  // Address of the MangoGroup
  const mangoGroupPk = new PublicKey(IDS[cluster].mango_groups.BTC_ETH_USDC.mango_group_pk)

  const keyPairPath = '/home/dd/.config/solana/id.json'
  const payer = new Account(JSON.parse(fs.readFileSync(keyPairPath, 'utf-8')))

  const mangoGroup = await client.getMangoGroup(connection, mangoGroupPk)

  const n = 1800

  const t0 = getUnixTs()
  for (let i = 0; i < n; i++) {
    // const pk = await client.initMarginAccount(connection, programId, mangoGroup, payer)
    const pks = await initMultipleMarginAccounts(client, connection, programId, mangoGroup, payer, 5)

    const elapsed = getUnixTs() - t0
    console.log(i, elapsed / (i+1), elapsed)

    for (const pk of pks) {
      console.log(pk.toBase58())
    }
    console.log('\n')
  }
}


async function initMultipleMarginAccounts(
  client: MangoClient,
  connection: Connection,
  programId: PublicKey,
  mangoGroup: MangoGroup,
  owner: Account,  // assumed to be same as payer for now
  n: number
): Promise<PublicKey[]> {
  const transaction = new Transaction()

  const additionalSigners: Account[] = []
  const marginAccountKeys: PublicKey[] = []
  for (let i = 0; i < n; i++) {
    // Create a Solana account for the MarginAccount and allocate space
    const accInstr = await createAccountInstruction(connection,
      owner.publicKey, MarginAccountLayout.span, programId)

    // Specify the accounts this instruction takes in (see program/src/instruction.rs)
    const keys = [
      { isSigner: false, isWritable: false, pubkey: mangoGroup.publicKey },
      { isSigner: false, isWritable: true,  pubkey: accInstr.account.publicKey },
      { isSigner: true,  isWritable: false, pubkey: owner.publicKey },
      { isSigner: false, isWritable: false, pubkey: SYSVAR_RENT_PUBKEY }
    ]

    // Encode and create instruction for actual initMarginAccount instruction
    const data = encodeMangoInstruction({ InitMarginAccount: {} })
    const initMarginAccountInstruction = new TransactionInstruction( { keys, data, programId })

    // Add all instructions to one atomic transaction
    transaction.add(accInstr.instruction)
    transaction.add(initMarginAccountInstruction)

    // Specify signers in addition to the wallet
    additionalSigners.push(accInstr.account)


    marginAccountKeys.push(accInstr.account.publicKey)
  }

  // sign, send and confirm transaction
  await client.sendTransaction(connection, transaction, owner, additionalSigners)

  return marginAccountKeys

}

async function testRent() {
  const client = new MangoClient()
  const cluster = 'mainnet-beta'
  const connection = new Connection(IDS.cluster_urls[cluster], 'singleGossip')
  const r = await connection.getMinimumBalanceForRentExemption(240, 'singleGossip')

  console.log(r, LAMPORTS_PER_SOL, r / LAMPORTS_PER_SOL, 16 * r / LAMPORTS_PER_SOL)

}


async function testTokenCall() {

  const client = new MangoClient()
  const cluster = 'mainnet-beta'
  const clusterUrl = IDS['cluster_urls'][cluster]
  const connection = new Connection(clusterUrl, 'singleGossip')
  const usdtKey = new PublicKey(IDS[cluster]['symbols']['USDC'])
  // const usdtKey = new PublicKey("8GxiBm7XirFqisDry3QdgiZDYMNfuZF1RKFTQbqBRVmp")

  const filters = [
    {
      memcmp: {
        offset: AccountLayout.offsetOf('mint'),
        bytes: usdtKey.toBase58(),
      }
    },

    {
      dataSize: AccountLayout.span,
    },
  ]
  const t0 = getUnixTs()
  const accounts = await getFilteredProgramAccounts(connection, TOKEN_PROGRAM_ID, filters)
  const t1 = getUnixTs()
  console.log(accounts.length, t1 - t0)
}

async function testServer() {
  const cluster = 'mainnet-beta'
  let clusterUrl = process.env.CLUSTER_URL
  if (!clusterUrl) {
    clusterUrl = IDS['cluster_urls'][cluster]
  }
  const connection = new Connection(clusterUrl, 'singleGossip')
  const usdtKey = new PublicKey(IDS[cluster]['symbols']['USDT'])
  const filters = [
    {
      memcmp: {
        offset: AccountLayout.offsetOf('mint'),
        bytes: usdtKey.toBase58(),
      }
    },

    {
      dataSize: AccountLayout.span,
    },
  ]
  const t0 = getUnixTs()
  const accounts = await getFilteredProgramAccounts(connection, TOKEN_PROGRAM_ID, filters)
  const t1 = getUnixTs()
  console.log(accounts.length, t1 - t0, accounts.length * AccountLayout.span)
}

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


async function testAll() {
  const client = new MangoClient()
  const cluster = 'mainnet-beta'
  const clusterUrl = process.env.CLUSTER_URL || IDS.cluster_urls[cluster]
  const connection = new Connection(clusterUrl, 'singleGossip')
  const programId = new PublicKey(IDS[cluster].mango_program_id)
  const dexProgramId = new PublicKey(IDS[cluster].dex_program_id)
  const mangoGroupPk = new PublicKey(IDS[cluster].mango_groups['BTC_ETH_USDT'].mango_group_pk)

  const keyPairPath = process.env.KEYPAIR || homedir() + '/.config/solana/id.json'

  const payer = new Account(JSON.parse(fs.readFileSync(keyPairPath, 'utf-8')))
  const mangoGroup = await client.getMangoGroup(connection, mangoGroupPk)


  /**
   * Verify that balances in the vault matches total deposits + amount in all the open orders
   */
  async function testVaultBalances() {
    return 0
  }

  /**
   * Test what happens when you try to withdraw what's in your deposits, but some of your coins are still borrowed
   */
  async function testWithdrawExcess() {
    return 0
  }

  async function testPlaceCancelOrder() {

    const prices = await mangoGroup.getPrices(connection)
    const marginAccounts = (await client.getMarginAccountsForOwner(connection, programId, mangoGroup, payer))
    marginAccounts.sort(
      (a, b) => (a.computeValue(mangoGroup, prices) > b.computeValue(mangoGroup, prices) ? -1 : 1)
    )
    let marginAccount = marginAccounts[0]

    const market = await Market.load(connection, mangoGroup.spotMarkets[0], { skipPreflight: true, commitment: 'singleGossip'}, mangoGroup.dexProgramId)
    console.log('placing order')
    const txid = await client.placeOrder(connection, programId, mangoGroup, marginAccount, market, payer, 'buy', 48000, 0.0001)
    console.log('order placed')

    await sleep(5000)
    marginAccount = await client.getMarginAccount(connection, marginAccount.publicKey, mangoGroup.dexProgramId)
    const bids = await market.loadBids(connection)
    const asks = await market.loadAsks(connection)
    console.log('canceling orders')
    await marginAccount.cancelAllOrdersByMarket(connection, client, programId, mangoGroup, market, bids, asks, payer)
    console.log('orders canceled')

  }

  async function testGetOpenOrdersLatency() {
    const t0 = getUnixTs()
    const accounts = await client.getMarginAccountsForOwner(connection, programId, mangoGroup, payer)
    const t1 = getUnixTs()
    console.log(t1 - t0, accounts.length)
  }

  async function testDrainAccount() {
    const prices = await mangoGroup.getPrices(connection)
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

    const marginAccountPk = new PublicKey("BrfYHWjU8UaWELfdR73qug1T5bWReg2tNJwUyHbzCgc2")
    const ma = await client.getMarginAccount(connection, marginAccountPk, mangoGroup.dexProgramId)
    while (true) {
      try {
        await drainAccount(client, connection, programId, mangoGroup, ma, markets, payer, prices, tokenWallets[NUM_TOKENS-1])
        console.log('complete')
        break
      } catch (e) {
        await sleep(1000)
      }
    }

  }

  async function testBorrowLimits() {
    console.log(mangoGroup.borrowLimits.map((b, i) => nativeToUi(b, mangoGroup.mintDecimals[i])))
  }

  await testBorrowLimits()
  // await testGetOpenOrdersLatency()
  // await testPlaceCancelOrder()
  // await testDrainAccount()
}


testAll()
// testServer()