import {Account, Connection, PublicKey, SystemProgram, TransactionInstruction} from "@solana/web3.js";
import { publicKeyLayout, u64 } from './layout';
import BN from 'bn.js';
import { WRAPPED_SOL_MINT } from '@project-serum/serum/lib/token-instructions';
import { blob, struct, u8 } from 'buffer-layout';


export const zeroKey = new PublicKey(new Uint8Array(32))


export async function createAccountInstruction(
  connection: Connection,
  payer: PublicKey,
  space: number,
  owner: PublicKey,
  lamports?: number
): Promise<{ account: Account, instruction: TransactionInstruction }> {
  const account = new Account();
  const instruction = SystemProgram.createAccount({
    fromPubkey: payer,
    newAccountPubkey: account.publicKey,
    lamports: lamports ? lamports : await connection.getMinimumBalanceForRentExemption(space),
    space,
    programId: owner
  })

  return { account, instruction };
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


const MINT_LAYOUT = struct([blob(44), u8('decimals'), blob(37)]);

export async function getMintDecimals(
  connection: Connection,
  mint: PublicKey,
): Promise<number> {
  if (mint.equals(WRAPPED_SOL_MINT)) {
    return 9;
  }
  const { data } = throwIfNull(
    await connection.getAccountInfo(mint),
    'mint not found',
  );
  const { decimals } = MINT_LAYOUT.decode(data);
  return decimals;
}

function throwIfNull<T>(value: T | null, message = 'account not found'): T {
  if (value === null) {
    throw new Error(message);
  }
  return value;
}


export function uiToNative(amount: number, decimals: number): BN {
  return new BN(Math.round(amount * Math.pow(10, decimals)))
}

export function nativeToUi(amount: number, decimals: number): number {

  return amount / Math.pow(10, decimals)

}