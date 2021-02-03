# 🥭 Mango Margin

## Vision 💫

We want to enable margin trading on the Serum with a focus on usability. Towards that end, Leverum tries to achieve the following design goals:

1. Hidden and automatic management of borrows when taking on a margin position
2. Easy to use graphical tools to automatically lend user funds at current market rates
3. Liquidity for borrowers on day 1
4. Execution of all trades on Serum's spot markets (incl. liquidations)

## To Do
1. Enforcer Bot
    * Keep a list of margin accounts sorted by collateral ratio and check periodically if they need to be liquidated
    * Put accounts into reduce only mode if they fall below init_collateral_ratio
2. Trading client
    * Similar to Serum dex client so market makers can easily switch to Mango margin
3. User interface
    * Add geo fence for prohibited jurisdictions
    * Show historical earnings
4. Smart contract
    * comprehensive testing and audit
    * Change oracle to chainlink
    * Serum pooling incentive to reduce fees
    * Instruction to close empty margin account
5. Liquidity
    * Get USDC lenders

## Setup 🛠

1. Install the solana tools
    ```
    sh -c "$(curl -sSfL https://release.solana.com/v1.4.13/install)"
    CLUSTER=devnet
    solana config set --url $CLUSTER
    KEYPAIR=~/.config/solana/id.json
    solana-keygen new
    ```
2. Install the spl-token cli utility

    ```
    git clone https://github.com/solana-labs/solana.git
    cd solana
    git checkout v1.4.13
    cargo install spl-token-cli
    ```

3. Build serum dex and deploy it to the devnet

    ```
    git clone https://github.com/project-serum/serum-dex
    cd serum-dex
    git checkout 49628a3f24a7256a1682c279192a8f535efd2d64
    ./do.sh build dex
    DEX_PROGRAM_ID=$(solana deploy dex/target/bpfel-unknown-unknown/release/serum_dex.so --use-deprecated-loader | jq .programId -r)
    cd crank
    cargo run -- $CLUSTER whole-shebang $KEYPAIR $DEX_PROGRAM_ID
    ```

3. Build the mango on-chain program and deploy it to the devnet

    ```
    pushd program
    cargo build-bpf
    MANGO_PROGRAM_ID="$(solana deploy target/deploy/mango.so | jq .programId -r)"
    popd
    ```

4. Create a few tokens and serum-dex spot-markets

    ```
    QUOTE_MINT=$(spl-token create-token  | head -n 1 | cut -d' ' -f3)
    QUOTE_WALLET=$(spl-token create-account $QUOTE_MINT | head -n 1 | cut -d' ' -f3)
    spl-token mint $QUOTE_MINT 1000000 $QUOTE_WALLET

    BASE0_MINT=$(spl-token create-token  | head -n 1 | cut -d' ' -f3)
    BASE0_WALLET=$(spl-token create-account $BASE0_MINT | head -n 1 | cut -d' ' -f3)
    spl-token mint $BASE0_MINT 1000000 $BASE0_WALLET

    BASE1_MINT=$(spl-token create-token  | head -n 1 | cut -d' ' -f3)
    BASE1_WALLET=$(spl-token create-account $BASE1_MINT | head -n 1 | cut -d' ' -f3)
    spl-token mint $BASE1_MINT 1000000 $BASE1_WALLET

    pushd ~/src/serum-dex/crank
    cargo run -- $CLUSTER list-market $KEYPAIR $DEX_PROGRAM_ID --coin-mint $BASE0_MINT --pc-mint $QUOTE_MINT
    cargo run -- $CLUSTER list-market $KEYPAIR $DEX_PROGRAM_ID --coin-mint $BASE1_MINT --pc-mint $QUOTE_MINT
    popd

    ```



