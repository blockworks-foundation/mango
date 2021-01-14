# CLI
### InitMangoGroup
Make sure the ids.json file contains the symbols and program ids
```
CLUSTER=devnet
KEYPAIR=~/.config/solana/id.json
IDS_PATH=../web/src/ids.json
TOKENS="BTC ETH USDC"
cargo run -- $CLUSTER init-mango-group --payer $KEYPAIR --ids-path $IDS_PATH --tokens $TOKENS
```

### InitMarginAccount
```
MANGO_GROUP_NAME=BTC_ETH_USDC
cargo run -- $CLUSTER init-margin-account --payer $KEYPAIR --ids-path $IDS_PATH --mango-group-name $MANGO_GROUP_NAME
```

### Deposit
```
MARGIN_ACCOUNT=EE6QQDGN7HZkUeSzCxU3PNbeB6batFXuGxw3thGiZ76W
MANGO_GROUP_NAME=BTC_ETH_USDC
TOKEN=BTC
cargo run -- $CLUSTER deposit --payer $KEYPAIR --ids-path $IDS_PATH --mango-group-name $MANGO_GROUP_NAME --token-symbol $TOKEN --quantity 1.2 --margin-account $MARGIN_ACCOUNT
 
```


### Run Full
``` 
cd ~/mango
pushd program
cargo build-bpf
MANGO_PROGRAM_ID="$(solana deploy target/deploy/mango.so | jq .programId -r)"
popd
cd cli

CLUSTER=devnet
KEYPAIR=~/.config/solana/id.json
IDS_PATH=../web/src/ids.json
TOKENS="BTC ETH USDC"
MANGO_GROUP_NAME=BTC_ETH_USDC
TOKEN=USDC
BORR_TOKEN=BTC
QUANTITY=2700
BORR=0.08

cargo run -- $CLUSTER init-mango-group --payer $KEYPAIR --ids-path $IDS_PATH --tokens $TOKENS --mango-program-id $MANGO_PROGRAM_ID
MARGIN_ACCOUNT=$(cargo run -- $CLUSTER init-margin-account --payer $KEYPAIR --ids-path $IDS_PATH --mango-group-name $MANGO_GROUP_NAME | tail -1)
cargo run -- $CLUSTER deposit --payer $KEYPAIR --ids-path $IDS_PATH --mango-group-name $MANGO_GROUP_NAME --token-symbol $TOKEN --quantity $QUANTITY --margin-account $MARGIN_ACCOUNT
cargo run -- $CLUSTER borrow --payer $KEYPAIR --ids-path $IDS_PATH --mango-group-name $MANGO_GROUP_NAME --margin-account $MARGIN_ACCOUNT --token-symbol $BORR_TOKEN --quantity $BORR
```

### Run Liquidator
```
CLUSTER=devnet
KEYPAIR=~/.config/solana/id.json
IDS_PATH=../web/src/ids.json
MANGO_GROUP_NAME=BTC_ETH_USDC
cargo run -- $CLUSTER run-liquidator --ids-path $IDS_PATH --payer $KEYPAIR --mango-group-name $MANGO_GROUP_NAME

```

