import {
  Account,
  AccountInfo,
  Connection,
  PublicKey,
  sendAndConfirmRawTransaction, SYSVAR_CLOCK_PUBKEY,
  SYSVAR_RENT_PUBKEY,
  Transaction,
  TransactionInstruction,
  TransactionSignature,
} from '@solana/web3.js';
import {
  encodeMangoInstruction,
  MangoGroupLayout,
  MarginAccountLayout,
  publicKeyLayout,
  u64,
  WideBits
} from "./layout";
import BN from "bn.js";
import {struct, blob, u8 } from 'buffer-layout';
import {createAccountInstruction} from "./utils";
import { Market, OpenOrders } from '@project-serum/serum';
import { Wallet } from '@project-serum/sol-wallet-adapter';
import { TOKEN_PROGRAM_ID } from '@project-serum/serum/lib/token-instructions';


export class MangoGroup {
  publicKey: PublicKey;

  accountFlags!: WideBits;
  tokens!: PublicKey[];
  vaults!: PublicKey[];
  indexes!: { lastUpdate: BN, borrow: number, deposit: number };
  spotMarkets!: PublicKey[];
  oracles!: PublicKey[];
  signerNonce!: BN;
  signerKey!: PublicKey;
  dexProgramId!: PublicKey;
  totalDeposits!: number[];
  totalBorrows!: number[];
  maintCollRatio!: number;
  initCollRatio!: number;

  constructor(publicKey: PublicKey, decoded: any) {
    this.publicKey = publicKey;
    Object.assign(this, decoded);
  }

  async getPrices(
    connection: Connection,
  ): Promise<number[]>  {
    const prices: number[] = [];
    const oracleAccs = await getMultipleAccounts(connection, this.oracles);
    for (let i = 0; i < oracleAccs.length; i++) {
      const decoded = decodeAggregatorInfo(oracleAccs[i].accountInfo)
      prices.push(decoded.submissionValue)
    }
    prices.push(1.0)
    return prices
  }

  getMarketIndex(spotMarket: Market): number {
    for (let i = 0; i < this.spotMarkets.length; i++) {
      if (this.spotMarkets[i].equals(spotMarket.publicKey)) {
        return i
      }
    }
    throw new Error("This Market does not belong to this MangoGroup")
  }
}

export class MarginAccount {
  publicKey: PublicKey;

  accountFlags!: WideBits;
  mangoGroup!: PublicKey;
  owner!: PublicKey;
  deposits!: number[];
  borrows!: number[];
  positions!: BN[];
  openOrders!: PublicKey[];

  constructor(publicKey: PublicKey, decoded: any) {
    this.publicKey = publicKey;
    Object.assign(this, decoded);
  }

}

export class MangoClient {
  async initMangoGroup() {
    throw new Error("Not Implemented");
  }
  async sendTransaction(
    connection: Connection,
    transaction: Transaction,
    payer: Account | Wallet,
    additionalSigners: Account[]
  ): Promise<TransactionSignature> {
    transaction.recentBlockhash = (await connection.getRecentBlockhash('max')).blockhash
    transaction.setSigners(payer.publicKey, ...additionalSigners.map( a => a.publicKey ))

    // if Wallet was provided, sign with wallet
    if ((typeof payer) == Wallet) {  // TODO test with wallet
      if (additionalSigners.length > 0) {
        transaction.partialSign(...additionalSigners)
      }
      transaction = payer.signTransaction(transaction)
    } else {
      // otherwise sign with the payer account
      const signers = [payer].concat(additionalSigners)
      transaction.sign(...signers)
    }
    const rawTransaction = transaction.serialize();
    return await sendAndConfirmRawTransaction(connection, rawTransaction, {skipPreflight: true})
  }
  async initMarginAccount(
    connection: Connection,
    programId: PublicKey,
    dexProgramId: PublicKey,  // Serum DEX program ID
    mangoGroup: MangoGroup,
    payer: Account | Wallet
  ): Promise<PublicKey> {

    // Create a Solana account for the MarginAccount and allocate space
    const accInstr = await createAccountInstruction(connection, payer.publicKey, MarginAccountLayout.span, programId)

    // Specify the accounts this instruction takes in (see program/src/instruction.rs)
    const keys = [
      { isSigner: false, isWritable: false, pubkey: mangoGroup.publicKey},
      { isSigner: false, isWritable: true,  pubkey: accInstr.account.publicKey },
      { isSigner: true,  isWritable: false, pubkey: payer.publicKey },
      { isSigner: false, isWritable: false, pubkey: SYSVAR_RENT_PUBKEY }
    ]

    // Encode and create instruction for actual initMarginAccount instruction
    const data = encodeMangoInstruction({ InitMarginAccount: {} })
    const initMarginAccountInstruction = new TransactionInstruction( { keys, data, programId })

    // Add all instructions to one atomic transaction
    const transaction = new Transaction()
    transaction.add(accInstr.instruction)
    transaction.add(initMarginAccountInstruction)

    // Specify signers in addition to the wallet
    const additionalSigners = [
      accInstr.account,
    ]

    // sign, send and confirm transaction
    await this.sendTransaction(connection, transaction, payer, additionalSigners)

    return accInstr.account.publicKey
  }

  /**
   * @Arthur
   * Find instruction details in program/src/instruction.rs
   * Look at cli/src/main.rs under Deposit command for an example in rust
   */
  async deposit() {
    throw new Error("Not Implemented");
  }
  async withdraw() {
    throw new Error("Not Implemented");
  }

  async borrow() {
    throw new Error("Not Implemented");
  }
  async settleBorrow() {
    throw new Error("Not Implemented");
  }
  async liquidate() {
    throw new Error("Not Implemented");
  }

  async placeOrder(
    connection: Connection,
    programId: PublicKey,
    mangoGroup: MangoGroup,
    marginAccount: MarginAccount,
    spotMarket: Market,
    owner: Account | Wallet,

    side: 'buy' | 'sell',
    price: number,
    size: number,
    orderType?: 'limit' | 'ioc' | 'postOnly',
    clientId?: BN,

  ): Promise<TransactionSignature> {

    const requestQueueKey = spotMarket['_decoded'].requestQueue
    const quoteVault = spotMarket['_decoded'].quoteVault
    const baseVault = spotMarket['_decoded'].baseVault

    orderType = orderType ?? 'limit'
    const marketIndex = mangoGroup.getMarketIndex(spotMarket)

    const zeroKey = new PublicKey(new Uint8Array(32))


    // Add all instructions to one atomic transaction
    const transaction = new Transaction()

    // Specify signers in addition to the wallet
    const additionalSigners: Account[] = []

    // Create a Solana account for the open orders account if it's missing
    const openOrdersKeys: PublicKey[] = [];
    for (let i = 0; i < marginAccount.openOrders.length; i++) {
      if (i === marketIndex && marginAccount.openOrders[marketIndex].equals(zeroKey)) {
        // open orders missing for this market; create a new one now
        const openOrdersSpace = OpenOrders.getLayout(mangoGroup.dexProgramId).span
        const openOrdersLamports = await connection.getMinimumBalanceForRentExemption(openOrdersSpace, 'singleGossip')
        const accInstr = await createAccountInstruction(
          connection, owner.publicKey, openOrdersSpace, mangoGroup.dexProgramId, openOrdersLamports
        )

        transaction.add(accInstr.instruction)
        additionalSigners.push(accInstr.account)
        openOrdersKeys.push(accInstr.account.publicKey)
      } else {
        openOrdersKeys.push(marginAccount.openOrders[i])
      }
    }


    const vaultIndex = (side === 'buy') ? mangoGroup.vaults.length - 1 : marketIndex

    const vaultPk = mangoGroup.vaults[vaultIndex]
    const keys = [
      { isSigner: false, isWritable: true, pubkey: mangoGroup.publicKey},
      { isSigner: true, isWritable: false,  pubkey: owner.publicKey },
      { isSigner: false,  isWritable: true, pubkey: marginAccount.publicKey },
      { isSigner: false, isWritable: false, pubkey: SYSVAR_CLOCK_PUBKEY },
      { isSigner: false, isWritable: false, pubkey: spotMarket.programId },
      { isSigner: false, isWritable: true, pubkey: spotMarket.publicKey },
      { isSigner: false, isWritable: true, pubkey: requestQueueKey },
      { isSigner: false, isWritable: true, pubkey: vaultPk },
      { isSigner: false, isWritable: false, pubkey: mangoGroup.signerKey },
      { isSigner: false, isWritable: true, pubkey: baseVault },
      { isSigner: false, isWritable: true, pubkey: quoteVault },
      { isSigner: false, isWritable: false, pubkey: TOKEN_PROGRAM_ID },
      { isSigner: false, isWritable: false, pubkey: SYSVAR_RENT_PUBKEY },
      ...openOrdersKeys.map( (pubkey) => ( { isSigner: false, isWritable: true, pubkey })),
      ...mangoGroup.oracles.map( (pubkey) => ( { isSigner: false, isWritable: false, pubkey })),
      ...mangoGroup.tokens.map( (pubkey) => ( { isSigner: false, isWritable: false, pubkey })),
    ]

    const limitPrice = spotMarket.priceNumberToLots(price)
    const maxQuantity = spotMarket.baseSizeNumberToLots(size)
    if (maxQuantity.lte(new BN(0))) {
      throw new Error('size too small')
    }
    if (limitPrice.lte(new BN(0))) {
      throw new Error('invalid price')
    }

    // TODO allow wrapped SOL wallets
    // TODO allow fee discounts
    const selfTradeBehavior = 'decrementTake'
    const data = encodeMangoInstruction(
      {
        PlaceOrder:
          clientId
            ? { side, limitPrice, maxQuantity, orderType, clientId, selfTradeBehavior }
            : { side, limitPrice, maxQuantity, orderType, selfTradeBehavior }
      }
    )

    const placeOrderInstruction = new TransactionInstruction( { keys, data, programId })
    transaction.add(placeOrderInstruction)


    // sign, send and confirm transaction
    return await this.sendTransaction(connection, transaction, owner, additionalSigners)

  }
  async settleFunds(
    connection: Connection,
    programId: PublicKey,
    mangoGroup: MangoGroup,
    marginAccount: MarginAccount,
    owner: Account | Wallet,
    spotMarket: Market,

  ): Promise<TransactionSignature> {

    const marketIndex = mangoGroup.getMarketIndex(spotMarket)
    const dexSigner = await PublicKey.createProgramAddress(
      [
        spotMarket.publicKey.toBuffer(),
        spotMarket['_decoded'].vaultSignerNonce.toArrayLike(Buffer, 'le', 8)
      ],
      spotMarket.programId
    )

    const keys = [
      { isSigner: false, isWritable: true, pubkey: mangoGroup.publicKey},
      { isSigner: true, isWritable: false,  pubkey: owner.publicKey },
      { isSigner: false,  isWritable: true, pubkey: marginAccount.publicKey },
      { isSigner: false, isWritable: false, pubkey: SYSVAR_CLOCK_PUBKEY },
      { isSigner: false, isWritable: false, pubkey: spotMarket.programId },
      { isSigner: false, isWritable: true, pubkey: spotMarket.publicKey },
      { isSigner: false, isWritable: true, pubkey: marginAccount.openOrders[marketIndex] },
      { isSigner: false, isWritable: false, pubkey: mangoGroup.signerKey },
      { isSigner: false, isWritable: true, pubkey: spotMarket['_decoded'].baseVault },
      { isSigner: false, isWritable: true, pubkey: spotMarket['_decoded'].quoteVault },
      { isSigner: false, isWritable: true, pubkey: mangoGroup.vaults[marketIndex] },
      { isSigner: false, isWritable: true, pubkey: mangoGroup.vaults[mangoGroup.vaults.length - 1] },
      { isSigner: false, isWritable: false, pubkey: dexSigner },
      { isSigner: false, isWritable: false, pubkey: TOKEN_PROGRAM_ID },
    ]
    const data = encodeMangoInstruction( {SettleFunds: {}} )

    const instruction = new TransactionInstruction( { keys, data, programId })

    // Add all instructions to one atomic transaction
    const transaction = new Transaction()
    transaction.add(instruction)

    // Specify signers in addition to the wallet
    const additionalSigners = []

    // sign, send and confirm transaction
    return await this.sendTransaction(connection, transaction, owner, additionalSigners)
  }
  async cancelOrder() {
    throw new Error("Not Implemented");
  }

  async getMangoGroup(
    connection: Connection,
    mangoGroupPk: PublicKey
  ): Promise<MangoGroup> {
    const acc = await connection.getAccountInfo(mangoGroupPk);
    return new MangoGroup(mangoGroupPk, MangoGroupLayout.decode(acc?.data));
  }

  async getMarginAccount(
    connection: Connection,
    marginAccountPk: PublicKey
  ): Promise<MarginAccount> {
    const acc = await connection.getAccountInfo(marginAccountPk, 'singleGossip')
    return new MarginAccount(marginAccountPk, MarginAccountLayout.decode(acc?.data))
  }
  async getAllMarginAccounts(
    connection: Connection,
    programId: PublicKey,
    mangoGroupPk: PublicKey
  ): Promise<MarginAccount[]>{
    const filters = [
      {
        memcmp: {
          offset: MarginAccountLayout.offsetOf('mangoGroup'),
          bytes: mangoGroupPk.toBase58(),
        },
      },

      {
        dataSize: MarginAccountLayout.span,
      },
    ];

    const accounts = await getFilteredProgramAccounts(connection, programId, filters);
    return accounts.map(
      ({ publicKey, accountInfo }) =>
        new MarginAccount(publicKey, MarginAccountLayout.decode(accountInfo?.data))
    );
  }
}

async function getMultipleAccounts(
  connection: Connection,
  publicKeys: PublicKey[]

): Promise<{ publicKey: PublicKey; accountInfo: AccountInfo<Buffer> }[]> {
  const publickKeyStrs = publicKeys.map((pk) => (pk.toBase58()));

  // @ts-ignore
  const resp = await connection._rpcRequest('getMultipleAccounts', [publickKeyStrs]);
  if (resp.error) {
    throw new Error(resp.error.message);
  }
  return resp.result.value.map(
    ({ data, executable, lamports, owner } , i) => ({
      publicKey: publicKeys[i],
      accountInfo: {
        data: Buffer.from(data[0], 'base64'),
        executable,
        owner: new PublicKey(owner),
        lamports,
      },
    }),
  );
}

async function getFilteredProgramAccounts(
  connection: Connection,
  programId: PublicKey,
  filters,
): Promise<{ publicKey: PublicKey; accountInfo: AccountInfo<Buffer> }[]> {
  // @ts-ignore
  const resp = await connection._rpcRequest('getProgramAccounts', [
    programId.toBase58(),
    {
      commitment: connection.commitment,
      filters,
      encoding: 'base64',
    },
  ]);
  if (resp.error) {
    throw new Error(resp.error.message);
  }
  return resp.result.map(
    ({ pubkey, account: { data, executable, owner, lamports } }) => ({
      publicKey: new PublicKey(pubkey),
      accountInfo: {
        data: Buffer.from(data[0], 'base64'),
        executable,
        owner: new PublicKey(owner),
        lamports,
      },
    }),
  );
}


export function getMedian(submissions: number[]): number {
  const values = submissions
    .filter((s: any) => s.value != 0)
    .map((s: any) => s.value)
    .sort((a, b) => a - b)

  const len = values.length
  if (len == 0) {
    return 0
  } else if (len == 1) {
    return values[0]
  } else {
    const i = len / 2
    return len % 2 == 0 ? (values[i] + values[i-1])/2 : values[i]
  }

}

export const AggregatorLayout = struct([
  blob(4, "submitInterval"),
  u64("minSubmissionValue"),
  u64("maxSubmissionValue"),
  blob(32, "description"),
  u8("isInitialized"),
  publicKeyLayout('owner'),
  blob(576, "submissions")
]);

export const SubmissionLayout = struct([
  u64("time"),
  u64("value"),
  publicKeyLayout('oracle'),
]);

export function decodeAggregatorInfo(accountInfo) {
  const aggregator = AggregatorLayout.decode(accountInfo.data);

  const minSubmissionValue = aggregator.minSubmissionValue;
  const maxSubmissionValue = aggregator.maxSubmissionValue;
  const submitInterval = aggregator.submitInterval;
  const description = (aggregator.description.toString() as string).trim()

  // decode oracles
  const submissions: any[] = []
  const submissionSpace = SubmissionLayout.span
  let latestUpdateTime = new BN(0);

  for (let i = 0; i < aggregator.submissions.length / submissionSpace; i++) {
    const submission = SubmissionLayout.decode(
      aggregator.submissions.slice(i*submissionSpace, (i+1)*submissionSpace)
    )

    submission.value = submission.value / 100.0;
    if (!submission.oracle.equals(new PublicKey(0))) {
      submissions.push(submission)
    }
    if (submission.time > latestUpdateTime) {
      latestUpdateTime = submission.time
    }
  }

  return {
    minSubmissionValue: minSubmissionValue,
    maxSubmissionValue: maxSubmissionValue,
    submissionValue: getMedian(submissions),
    submitInterval,
    description,
    oracles: submissions.map(s => s.oracle.toString()),
    latestUpdateTime: new Date(Number(latestUpdateTime)*1000),
  }
}
