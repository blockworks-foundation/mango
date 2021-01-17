import {
  AccountInfo,
  Connection,
  PublicKey
} from "@solana/web3.js";
import {MarginAccountLayout, NUM_TOKENS} from "./layouts";



export class MangoClient {
  greeting: string;

  constructor(initGreet?: string) {
	  this.greeting = initGreet ?? 'hello world';
  }

  greet() {
	  console.log(this.greeting);
  }


  async getCollateralizationRatio(

  ): Promise<number> {
    return 1.2;
  }

  async getPrices(
    connection: Connection,
    programId: PublicKey,
    mangoGroup: any
  ): Promise<number[]>  {
    const prices = new Array(NUM_TOKENS);
    prices[NUM_TOKENS - 1] = 1;
    return prices;
  }


  async getAllMarginAccounts(
    connection: Connection,
    programId: PublicKey,
    mangoGroupPk: PublicKey
  ): Promise<{ pk: PublicKey, account: any}[]>{
    const filters = [
      {
        memcmp: {
          offset: MarginAccountLayout.offsetOf('mango_group'),
          bytes: mangoGroupPk.toBase58(),
        },
      },

      {
        dataSize: MarginAccountLayout.span,
      },
    ];

    const accounts = await getFilteredProgramAccounts(connection, programId, filters);
    return accounts.map(({ publicKey, accountInfo }) =>
      ({pk: publicKey, account: MarginAccountLayout.decode(accountInfo.data)})
    );
  }
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
