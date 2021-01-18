import {
  AccountInfo,
  Connection,
  PublicKey
} from "@solana/web3.js";
import {MangoGroupLayout, MarginAccountLayout, NUM_TOKENS, publicKeyLayout, u64, WideBits} from "./layout";
import BN from "bn.js";
import {struct, blob, nu64, union, u8, u32, Layout, bits, Blob, seq, BitStructure } from 'buffer-layout';


export class MangoGroup {
  publicKey: PublicKey;

  accountFlags!: WideBits;
  tokens!: PublicKey[];
  vaults!: PublicKey[];
  indexes!: { lastUpdate: number, borrow: number, deposit: number };
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
  async initMarginAccount() {
    throw new Error("Not Implemented");
  }
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
