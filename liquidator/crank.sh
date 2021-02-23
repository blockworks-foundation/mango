source ~/mango-client-ts/devnet.env
DEX_PROGRAM_ID=$(cat ~/mango-client-ts/src/ids.json | jq .devnet.dex_program_id -r)
BTCUSDC=$(cat ~/mango-client-ts/src/ids.json | jq '.devnet.spot_markets|.["BTC/USDC"]' -r)

cd ~/blockworks-foundation/serum-dex/dex/crank
cargo run -- $CLUSTER consume-events --dex-program-id $DEX_PROGRAM_ID --payer $KEYPAIR --market $BTCUSDC --coin-wallet $BTC_WALLET --pc-wallet $USDC_WALLET --num-workers 1 --events-per-worker 5 --log-directory .
