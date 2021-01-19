import {Account, Connection, PublicKey, SystemProgram, TransactionInstruction} from "@solana/web3.js";

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