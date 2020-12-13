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