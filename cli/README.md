# CLI
### InitMangoGroup
Make sure the ids.json file contains the symbols and program ids
```
CLUSTER=devnet
KEYPAIR=~/.config/solana/id.json
IDS_PATH=../common/ids.json
TOKENS="BTC ETH USD"
cargo run -- $CLUSTER init-mango-group --payer $KEYPAIR --ids-path $IDS_PATH --tokens $TOKENS
```

### InitMarginAccount
```
MANGO_GROUP_NAME=BTC_ETH_USDC
cargo run -- $CLUSTER init-margin-account --payer $KEYPAIR --ids-path $IDS_PATH\ 
--mango-group-name $MANGO_GROUP_NAME --margin-account $MARGIN_ACCOUNT
```

### Deposit (doesn't work)
```
MARGIN_ACCOUNT=63wi7BsQjZoLmzWrQyoEn7A48DVC7ZgCdfcjS9FrJdCs
MANGO_GROUP_NAME=BTC_ETH_USDC
TOKEN=BTC
cargo run -- $CLUSTER deposit --payer $KEYPAIR --ids-path $IDS_PATH --mango-group-name $MANGO_GROUP_NAME\ 
--token-symbol $TOKEN --quantity 1.2 --margin-account $MARGIN_ACCOUNT
```