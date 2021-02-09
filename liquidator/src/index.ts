import { MangoClient, IDS, MangoGroup } from "@mango/client";

import {
  Connection,
  PublicKey
} from "@solana/web3.js";

async function main() {
  const client = new MangoClient()
  const cluster = 'devnet'
  const connection = new Connection(IDS.cluster_urls[cluster], 'singleGossip')
  const programId = new PublicKey(IDS[cluster].mango_program_id)
  const dexProgramId = new PublicKey(IDS[cluster].dex_program_id)
  const mangoGroupPk = new PublicKey(IDS[cluster].mango_groups.BTC_ETH_USDC.mango_group_pk)

  const mangoGroup = await client.getMangoGroup(connection, mangoGroupPk)
  const marginAccounts = await client.getAllMarginAccounts(connection, programId, mangoGroupPk)

  // fetch open orders
  for (const ma of marginAccounts) {  // TODO load with websocket
    await ma.loadOpenOrders(connection, dexProgramId)
  }
  const prices = await mangoGroup.getPrices(connection)  // TODO put this on websocket as well
  for (const ma of marginAccounts) {
    console.log(ma.toPrettyString(mangoGroup))
    const assetsVal = ma.getAssetsVal(mangoGroup, prices)
    const liabsVal = ma.getLiabsVal(mangoGroup, prices)
    if (liabsVal === 0) {
      continue
    }
    const collRatio = assetsVal / liabsVal
    console.log(assetsVal, liabsVal, collRatio)

    if (collRatio < mangoGroup.maintCollRatio) {
      // handle undercoll case separately
      if (collRatio < 1) {
        throw new Error("Unimplemented")
        // Need to make sure there are enough funds in MangoGroup to be compensated fully
      }


      // determine how much to deposit to get the account above init coll ratio
      const needed = assetsVal - liabsVal * mangoGroup.initCollRatio



      // after depositing and receiving success, cancel outstanding open orders
      // place new orders to liquidate all positions into USDC
      // call settleBorrow on every open borrow
      // transfer assets out to own marginAccount
    }

  }

}

main();