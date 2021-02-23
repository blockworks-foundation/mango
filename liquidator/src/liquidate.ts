// TODO make sure funds in liquidatable account can actually be withdrawn
//    -- can't be withdrawn if total deposits == total_borrows

import { IDS, MangoClient } from '@mango/client';
import { Account, Connection, PublicKey } from '@solana/web3.js';
import fs from 'fs';

async function runLiquidator() {
  const client = new MangoClient()
  const cluster = 'devnet'
  const connection = new Connection(IDS.cluster_urls[cluster], 'singleGossip')

  // The address of the Mango Program on the blockchain
  const programId = new PublicKey(IDS[cluster].mango_program_id)

  // The address of the serum dex program on the blockchain: https://github.com/project-serum/serum-dex
  const dexProgramId = new PublicKey(IDS[cluster].dex_program_id)

  // Address of the MangoGroup
  const mangoGroupPk = new PublicKey(IDS[cluster].mango_groups.BTC_ETH_USDC.mango_group_pk)


  // liquidator's keypair
  const keyPairPath = '/home/dd/.config/solana/id.json'
  const payer = new Account(JSON.parse(fs.readFileSync(keyPairPath, 'utf-8')))

  // TODO fetch these automatically
  const tokenWallets = [
    new PublicKey("HLoPtihB8oETm1kkTpx17FEnXm7afQdS4hojTNvbg3Rg"),
    new PublicKey("8ASVNBAo94RnJCABYybnkJnXGpBHan2svW3pRsKdbn7s"),
    new PublicKey("GBBtcVE7WA8qdrHyhWTZkYDaz71EVHsg7wVaca9iq9xs")
  ]


}