# devnet
# First deploy our custom version of dex
cd ~/blockworks-foundation/serum-dex/
./do.sh update
./do.sh build dex
DEX_PROGRAM_ID="$(solana program deploy dex/target/bpfel-unknown-unknown/release/serum_dex.so | jq .programId -r)"

# Then deploy markets
cd dex/crank
source ~/mango-client-ts/devnet.env

cargo run -- $CLUSTER list-market $KEYPAIR $DEX_PROGRAM_ID --coin-mint $BTC --pc-mint $USDT
cargo run -- $CLUSTER list-market $KEYPAIR $DEX_PROGRAM_ID --coin-mint $ETH --pc-mint $USDT


# Next start cranks for our devnet markets (use tmux)
cd ~/mango/liquidator
source crank.sh btc usdt
source crank.sh eth usdt

# deploy mango program and new mango group
cd ~/mango
pushd program
cargo build-bpf
MANGO_PROGRAM_ID="$(solana program deploy target/deploy/mango.so | jq .programId -r)"
popd
cd cli

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

# run the solink oracles
cd ~/solana-flux-aggregator
yarn solink oracle


# mainnet
source ~/mango-client-ts/mainnet.env
cd ~/mango
pushd program
cargo build-bpf
MANGO_PROGRAM_ID="$(solana program deploy --keypair $KEYPAIR target/deploy/mango.so | jq .programId -r)"
popd
cd cli

CLUSTER=mainnet
KEYPAIR=~/.config/solana/blw.json
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
