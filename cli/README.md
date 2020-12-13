# CLI
### InitMangoGroup
Make sure the ids.json file contains the symbols and program ids
```
CLUSTER=devnet
KEYPAIR=~/.config/solana/id.json
IDS_PATH=../common/ids.json
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
MARGIN_ACCOUNT=8c1CGBvgbuxU7ARvBtV1646omZXx9HLsxHTfkppKwSBA
MANGO_GROUP_NAME=BTC_ETH_USDC
TOKEN=BTC
cargo run -- $CLUSTER deposit --payer $KEYPAIR --ids-path $IDS_PATH --mango-group-name $MANGO_GROUP_NAME --token-symbol $TOKEN --quantity 1.2 --margin-account $MARGIN_ACCOUNT
 
```


### Run Full
```
cd mango
pushd program
cargo build-bpf
solana deploy target/deploy/mango.so
# copy the program id into mango_program_id in ids.json

popd
cd cli

CLUSTER=devnet
KEYPAIR=~/.config/solana/id.json
IDS_PATH=../common/ids.json
TOKENS="BTC ETH USDC"
MANGO_GROUP_NAME=BTC_ETH_USDC
TOKEN=BTC

cargo run -- $CLUSTER init-mango-group --payer $KEYPAIR --ids-path $IDS_PATH --tokens $TOKENS
MARGIN_ACCOUNT=$(cargo run -- $CLUSTER init-margin-account --payer $KEYPAIR --ids-path $IDS_PATH --mango-group-name $MANGO_GROUP_NAME | tail -1)
cargo run -- $CLUSTER deposit --payer $KEYPAIR --ids-path $IDS_PATH --mango-group-name $MANGO_GROUP_NAME --token-symbol $TOKEN --quantity 1.2 --margin-account $MARGIN_ACCOUNT

```