source ~/mango-client-ts/devnet.env
DEX_PROGRAM_ID=$(cat ~/mango-client-ts/src/ids.json | jq .devnet.dex_program_id -r)

MARKET_STR="${1^^}/${2^^}"

if [ $MARKET_STR = "BTC/USDT" ]; then
  MARKET=$(cat ~/mango-client-ts/src/ids.json | jq '.devnet.spot_markets|.["BTC/USDT"]' -r)
  BASE_WALLET=$BTC_WALLET
  QUOTE_WALLET=$USDT_WALLET
elif [ $MARKET_STR = "ETH/USDT" ]; then
  MARKET=$(cat ~/mango-client-ts/src/ids.json | jq '.devnet.spot_markets|.["ETH/USDT"]' -r)
  BASE_WALLET=$ETH_WALLET
  QUOTE_WALLET=$USDT_WALLET
elif [ $MARKET_STR = "BTC/USDC" ]; then
  MARKET=$(cat ~/mango-client-ts/src/ids.json | jq '.devnet.spot_markets|.["BTC/USDC"]' -r)
  BASE_WALLET=$BTC_WALLET
  QUOTE_WALLET=$USDC_WALLET
elif [ $MARKET_STR = "ETH/USDC" ]; then
  MARKET=$(cat ~/mango-client-ts/src/ids.json | jq '.devnet.spot_markets|.["ETH/USDC"]' -r)
  BASE_WALLET=$ETH_WALLET
  QUOTE_WALLET=$USDC_WALLET
else
  echo "invalid args"
fi


cd ~/blockworks-foundation/serum-dex/dex/crank

cargo run -- $CLUSTER consume-events --dex-program-id $DEX_PROGRAM_ID --payer $KEYPAIR --market $MARKET --coin-wallet $BASE_WALLET --pc-wallet $QUOTE_WALLET --num-workers 1 --events-per-worker 5 --log-directory .
