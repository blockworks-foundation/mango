cd ~/serum-dex/dex/crank
source ~/mango/client-ts/devnet.env
cargo run -- $CLUSTER consume-events --dex-program-id $DEX_PROGRAM_ID --payer $KEYPAIR --market $BTCUSD --coin-wallet $BTC_WALLET --pc-wallet $USDC_WALLET --num-workers 1 --events-per-worker 5 --log-directory .