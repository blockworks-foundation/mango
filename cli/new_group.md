1. First build and deploy serum dex to devnet (if you just want to use already deployed then skip this step)
2. Go to blockworks-foundation/solana-flux-aggregator, Add the token pairs you want into config/setup.dev.json
3. Run `yarn solink setup config/setup.dev.json`
4. Make sure the feeds in solana flux aggregator can feed new tokens
5. Add the oracle pubkeys found in deploy.dev.json into ids.json devnet.oracles
6. Add the token mints to ids.json devnet.symbols
7. Amend devnet.env and add new symbols
8. List the new markets. For example:

```
source ~/mango/cli/devnet.env

cargo run -- $CLUSTER list-market $KEYPAIR $DEX_PROGRAM_ID --coin-mint $BTC --pc-mint $USDT
cargo run -- $CLUSTER list-market $KEYPAIR $DEX_PROGRAM_ID --coin-mint $ETH --pc-mint $USDT
```

9. Add the MarketState pubkeys to ids.json devnet.spot_markets
10. go to blockworks-foundation/liquidator/crank.sh and add support for your new markets
11. run crank.sh to run the cranks, for example
```
source crank.sh btc usdt
source crank.sh eth usdt
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