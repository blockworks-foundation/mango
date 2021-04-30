1. First build and deploy serum dex to devnet (if you just want to use already deployed then skip this step)
2. Go to blockworks-foundation/solana-flux-aggregator, Add the token pairs you want into config/setup.dev.json
3. Run `yarn solink setup config/setup.dev.json`
4. Make sure the feeds in solana flux aggregator can feed new tokens
   * Make sure the supported exchanges in feeds.ts have the tokens you want, if not write the feed
5. Add the oracle pubkeys found in deploy.dev.json into mango-client-ts/src/ids.json devnet.oracles
6. Add the token mints for your new tokens to ids.json devnet.symbols
7. Amend devnet.env and add your new new symbols
8. List the new markets. For example:

```
source ~/mango/cli/devnet.env
cd ~/blockworks-foundation/serum-dex/
cargo run -- $CLUSTER list-market $KEYPAIR $DEX_PROGRAM_ID --coin-mint $BTC --pc-mint $USDT
cargo run -- $CLUSTER list-market $KEYPAIR $DEX_PROGRAM_ID --coin-mint $ETH --pc-mint $USDT
```

9. Add the MarketState pubkeys to ids.json devnet.spot_markets
10. go to blockworks-foundation/liquidator/crank.sh and add support for your new markets
11. run crank.sh to run the cranks, for example
```
source crank.sh $KEYPAIR btc usdt
source crank.sh $KEYPAIR eth usdt
```
12. Deploy new mango group for example:
```
CLUSTER=devnet
KEYPAIR=~/.config/solana/id.json
IDS_PATH=~/mango-client-ts/src/ids.json
TOKENS="BTC ETH USDT"
MANGO_GROUP_NAME=BTC_ETH_USDT
BORROW_LIMITS="1.0 20.0 50000.0"

cargo run -- $CLUSTER init-mango-group \
--payer $KEYPAIR \
--ids-path $IDS_PATH \
--tokens $TOKENS \
--mango-program-id $MANGO_PROGRAM_ID \
--borrow-limits $BORROW_LIMITS
```

13. For mainnet, it's recommended that you first do this on devnet and then rework it for mainnet