import { IDS, MangoClient, MangoGroup, MarginAccount, MarginAccountLayout } from '@mango/client';
import {
  Account,
  Connection, LAMPORTS_PER_SOL,
  PublicKey,
  SYSVAR_RENT_PUBKEY,
  Transaction,
  TransactionInstruction,
} from '@solana/web3.js';
import fs from 'fs';
import { getUnixTs, sleep } from './utils';
import { createAccountInstruction, getFilteredProgramAccounts } from '@mango/client/lib/utils';
import { encodeMangoInstruction } from '@mango/client/lib/layout';
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

  // await testGetOpenOrdersLatency()
  await testPlaceCancelOrder()
}

testAll()
// testServer()