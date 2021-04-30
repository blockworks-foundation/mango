# devnet
if [ $# -eq 0 ]
  then
    KEYPAIR=~/.config/solana/id.json
  else
    KEYPAIR=$1
fi

# deploy mango program and new mango group
source ~/mango/cli/devnet.env $KEYPAIR
solana config set --url $DEVNET_URL

cd ~/mango
pushd program

# build bpf for devnet (just do cargo build-bpf for the mainnet version
mkdir target/devnet
cargo build-bpf --features devnet --bpf-out-dir target/devnet

# this will give a separate program id for devnet
#solana-keygen new --outfile target/devnet/mango-dev.json
#MANGO_PROGRAM_ID="$(solana program deploy target/devnet/mango.so --program-id target/devnet/mango-dev.json | jq .programId -r)"
MANGO_PROGRAM_ID="$(solana program deploy target/devnet/mango.so --program-id $MANGO_PROGRAM_ID --output json-compact | jq .programId -r)"
popd
cd cli

CLUSTER=devnet
TOKENS="BTC ETH USDT"
MANGO_GROUP_NAME=BTC_ETH_USDT
BORROW_LIMITS="1.0 20.0 50000.0"

# This will deploy the BTC_ETH_USDT mango group and automatically update the ids.json in mango client
# Make sure IDS_PATH is set correctly in mango/cli/devnet.env, or set it again before running this
cargo run -- $CLUSTER init-mango-group \
--payer $KEYPAIR \
--ids-path $IDS_PATH \
--tokens $TOKENS \
--mango-program-id $MANGO_PROGRAM_ID \
--borrow-limits $BORROW_LIMITS
