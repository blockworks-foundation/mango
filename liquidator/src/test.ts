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
import { getUnixTs } from './utils';
import { createAccountInstruction, getFilteredProgramAccounts } from '@mango/client/lib/utils';
import { encodeMangoInstruction } from '@mango/client/lib/layout';
import { Token, MintLayout, AccountLayout, TOKEN_PROGRAM_ID } from '@solana/spl-token';


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

testServer()
