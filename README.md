# Mango Markets - Decentralized Margin Trading

## ⚠️ Warning

Any content produced by Blockworks, or developer resources that Blockworks provides, are for educational and inspiration purposes only. Blockworks does not encourage, induce or sanction the deployment of any such applications in violation of applicable laws or regulations.

## Contribute
Significant contributions to the source code may be compensated with a grant from the Blockworks Foundation.

## Security
Mango is currently unaudited software. Use at your own risk.

You may be eligible for a substantial reward if you find a vulnerability and report it privately to hello@blockworks.foundation

## Setup
This setup assumes you're familiar with the basics of Solana development. 
If you're not, you might find it useful to follow the instructions here: https://github.com/solana-labs/example-helloworld 
and/or here: https://docs.solana.com/cli to get set up with node and rust as well

### get rust
```
sudo apt-get install -y pkg-config build-essential python3-pip jq
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source $HOME/.cargo/env
rustup component add rustfmt
rustup default nightly
rustup component add rust-src
```

Note that currently rust version at least 1.50 is needed. Check rust version and upgrade if necessary with
```
rustc --version
rustup update
```

### get mango
```
VERSION=v1.6.4
sh -c "$(curl -sSfL https://release.solana.com/$VERSION/install)"
sudo apt-get install -y libssl-dev libudev-dev
cargo install spl-token-cli

git clone git@github.com:blockworks-foundation/mango.git
git clone git@github.com:blockworks-foundation/mango-client-ts.git
```


### get devnet coins
- Go to mango-client-ts/src/ids.json and create a token account in [sollet.io](http://sollet.io) for each of the symbols under devnet
- Go to [https://spl-token-ui.netlify.app/#/token-faucets](https://spl-token-ui.netlify.app/#/token-faucets),
- switch to devnet cluster in top right and `Token airdrop` tab
- then copy paste the faucet id for the token you want from ids.json.devnet.faucets
- paste in your corresponding token account address
- hit `Airdrop tokens` and check your [sollet.io](http://sollet.io) to see if you've received tokens

### deploy devnet
Note: Make sure the IDS_PATH and KEYPAIR are correct in mango/cli/devnet.env

Then set the devnet keys and URL
```
source cli/devnet.env
solana config set --url $CLUSTER_URL
```

Then compile program and deploy using devnet_deploy.sh in mango/cli
```
cd cli
. devnet_deploy.sh
```

### deploy mainnet
Rework devnet_deploy.sh and use cli/mainnet.env to deploy to mainnet 


### run tests
Regression and integration tests are in progress. To run them
```
cd program
cargo test  # run non-solana VM tests (none at the moment but would include simple unit tests in the future)
cargo test-bpf  # run tests that use the solana VM (ie the smart contract tests)
```
