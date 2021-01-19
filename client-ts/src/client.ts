import {
  Account,
  AccountInfo,
  Connection,
  PublicKey,
  sendAndConfirmRawTransaction,
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
import {OpenOrders} from "@project-serum/serum";
import { Wallet } from '@project-serum/sol-wallet-adapter';


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
    return await sendAndConfirmRawTransaction(connection, rawTransaction)
  }
  async initMarginAccount(
    connection: Connection,
    programId: PublicKey,
    dexProgramId: PublicKey,  // public key of serum dex MarketState
    mangoGroup: MangoGroup,
    payer: Account | Wallet
  ): Promise<PublicKey> {

    // Create a Solana account for the MarginAccount and allocate space
    const accInstr = await createAccountInstruction(connection, payer.publicKey, MarginAccountLayout.span, programId)
    const openOrdersSpace = OpenOrders.getLayout(dexProgramId).span

    // Create a Solana account for each of the OpenOrders accounts
    const openOrdersLamports = await connection.getMinimumBalanceForRentExemption(openOrdersSpace, 'singleGossip')
    const openOrdersAccInstrs = await Promise.all(mangoGroup.spotMarkets.map(
      (_) => createAccountInstruction(connection, payer.publicKey, openOrdersSpace, dexProgramId, openOrdersLamports)
    ))

    // Specify the accounts this instruction takes in (see program/src/instruction.rs)
    const keys = [
      { isSigner: false, isWritable: false, pubkey: mangoGroup.publicKey},
      { isSigner: false, isWritable: true,  pubkey: accInstr.account.publicKey },
      { isSigner: true,  isWritable: false, pubkey: payer.publicKey },
      { isSigner: false, isWritable: false, pubkey: SYSVAR_RENT_PUBKEY },
      ...openOrdersAccInstrs.map(
        (o) => ({ isSigner: false,  isWritable: false, pubkey: o.account.publicKey })
      )
    ]

    // Encode and create instruction for actual initMarginAccount instruction
    const data = encodeMangoInstruction({ InitMarginAccount: {} })
    const initMarginAccountInstruction = new TransactionInstruction( { keys, data, programId })

    // Add all instructions to one atomic transaction
    const transaction = new Transaction()
    transaction.add(accInstr.instruction)
    transaction.add(...openOrdersAccInstrs.map( o => o.instruction ))
    transaction.add(initMarginAccountInstruction)

    // Specify signers in addition to the wallet
    const additionalSigners = [
      accInstr.account,
      ...openOrdersAccInstrs.map( o => o.account )
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
  async placeOrder() {
    throw new Error("Not Implemented");
  }
  async settleFunds() {
    throw new Error("Not Implemented");
  }
  async cancelOrder() {
    throw new Error("Not Implemented");
  }

  async getMangoGroup(
    connection: Connection,
    programId: PublicKey,
    mangoGroupPk: PublicKey
  ): Promise<MangoGroup> {
    const acc = await connection.getAccountInfo(mangoGroupPk);
    return new MangoGroup(mangoGroupPk, MangoGroupLayout.decode(acc?.data));
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
