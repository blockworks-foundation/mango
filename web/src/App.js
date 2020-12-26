import React, {useEffect, useMemo, useState} from 'react';

import { Account, Connection, PublicKey, sendAndConfirmRawTransaction, SystemProgram, Transaction, TransactionInstruction, SYSVAR_RENT_PUBKEY } from '@solana/web3.js';
import { TokenInstructions } from '@project-serum/serum';
import Wallet from '@project-serum/sol-wallet-adapter';

import AmountInput from './components/AmountInput';

import {
  MarginAccountLayout,
  OpenOrdersLayout,
  encodeMangoInstruction,
  NUM_TOKENS,
  MangoGroupLayout,
  MangoIndexLayout
} from './layouts';
import ID from './ids.json';

import './App.css';
import {MINT_LAYOUT} from "@project-serum/swap";
import * as util from "util";

const MINUTE = 60;
const HOUR = 60 * MINUTE;
const DAY = 24 * HOUR;
const YEAR = 365 * DAY;

function App() {

  const [network, setNetwork] = useState('devnet');
  const config = useMemo(() => ID[network], [network]);
  const clusterUrl = useMemo(() => ID.cluster_urls[network], [network]);
  const connection = useMemo(() => new Connection(clusterUrl), [clusterUrl]);
  const wallet = useMemo(() => new Wallet('https://www.sollet.io', clusterUrl), [clusterUrl]);


  const [connected, setConnected] = useState(false);
  useEffect(() => {
    wallet.on('connect', () => {
      console.log('Connected to wallet ', wallet.publicKey.toBase58());
      setConnected(true);
    });

    wallet.on('disconnect', () => {
      console.log('Disconnected from wallet');
      setConnected(false);
    });

    return () => {
      wallet.disconnect();
    };
  }, [wallet]);


  const [accounts, setAccounts] = useState([]);
  async function fetchSPLAccounts() {
    if (!wallet.publicKey || !connection || !connected) {
      return
    }

    console.log('Fetch all SPL tokens for', wallet.publicKey.toString());

    const response = await connection.getParsedTokenAccountsByOwner(
      wallet.publicKey,
      { programId: TokenInstructions.TOKEN_PROGRAM_ID }
    );

    console.log(response.value.length, 'SPL tokens found', response);

    response.value.map((a) => a.account.data.parsed.info).forEach((info, _) => {
      console.log(info.mint, info.tokenAmount.uiAmount);
    });

    setAccounts(response.value.map((a) => a.account.data.parsed.info).map((i) => [i.mint, i.tokenAmount.uiAmount]));

    return response.value;
  }

  async function createAccountInstruction(space, programId) {
    const account = new Account();
    const instruction = SystemProgram.createAccount({
        fromPubkey: wallet.publicKey,
        newAccountPubkey: account.publicKey,
        lamports: await connection.getMinimumBalanceForRentExemption(space),
        space,
        programId,
      })

    return { account, instruction };
  }

  async function signTransaction(transaction, additionalSigners = []) {
    transaction.recentBlockhash = (await connection.getRecentBlockhash('max')).blockhash;
    transaction.setSigners(wallet.publicKey, ...additionalSigners.map((s) => s.publicKey));
    if (additionalSigners.length > 0) {
      transaction.partialSign(...additionalSigners);
    }
    let res = await wallet.signTransaction(transaction);
    return res;
  }

  async function sendSignedTransaction(signedTransaction) {
    const rawTransaction = signedTransaction.serialize();
    return await sendAndConfirmRawTransaction(connection, rawTransaction)
  }

  async function sendTransaction(transaction, additionalSigners = []) {
    const signedTransaction = await signTransaction(transaction, additionalSigners);
    return await sendSignedTransaction(signedTransaction);
  }

  async function getFilteredProgramAccounts(connection, programId, filters) {
    const resp = await connection._rpcRequest('getProgramAccounts', [
      programId.toBase58(),
      {
        commitment: connection.commitment,
        filters,
        encoding: 'base64',
      },
    ]);

    if (resp.error) {
      throw new Error(resp.error.message);
    }
    return resp.result.map(({ pubkey, account: { data, executable, owner, lamports } }) => ({
      publicKey: new PublicKey(pubkey),
      accountInfo: {
        data: Buffer.from(data[0], 'base64'),
        executable,
        owner: new PublicKey(owner),
        lamports,
      },
    }));
  }

  const [marginAccounts, setMarginAccounts] = useState(undefined);



  async function fetchMarginAccounts() {
    if (!wallet.publicKey || !connection || !connected) {
      console.error('ensure wallet is connected', wallet, connection, connected);
      return
    }
    let mangoGroupPk = new PublicKey(config.mango_groups["BTC_ETH_USDC"].mango_group_pk);
    let mangoGroupInfo = await connection.getAccountInfo(mangoGroupPk);
    let mangoGroup = MangoGroupLayout.decode(mangoGroupInfo.data);

    const programId = new PublicKey(config.mango_program_id);
    const filters = [
      {
        memcmp: {
          offset: MarginAccountLayout.offsetOf('owner'),
          bytes: wallet.publicKey.toBase58(),
        },
      },
      {
        dataSize: MarginAccountLayout.span,
      },
    ];

    const response = await getFilteredProgramAccounts(connection, programId, filters);
    console.log(response)
    const decoded = response.map(a => [a.publicKey, MarginAccountLayout.decode(a.accountInfo.data)]);

    let marginAccount = decoded[0][1];
    for (let i = 0; i < NUM_TOKENS; i++) {
      u64f64BytesToFloat(marginAccount.deposits.slice(i * 16, (i+1) * 16));
    }
    console.log('MarginAccounts decoded', decoded, decoded[0][0].toString());

    setMarginAccounts(decoded);
  }

  const [openOrdersAccounts, setOpenOrdersAccounts] = useState(undefined);

  async function fetchOpenOrdersAccounts() {
    if (!wallet.publicKey || !connection || !connected) {
      console.error('ensure wallet is connected', wallet, connection, connected);
      return
    }

    console.log('Fetch mango OpenOrdersAccounts for', wallet.publicKey.toString());


    const programId = new PublicKey(config.dex_program_id);
    const filters = [
      /*{
        memcmp: {
          offset: OpenOrdersLayout.offsetOf('owner'),
          bytes: wallet.publicKey.toBase58(),
        },
      },*/
      {
        dataSize: OpenOrdersLayout.span,
      },
    ];

    const response = await getFilteredProgramAccounts(connection, programId, filters);
    console.log('OpenOrdersAccounts fetched', response);


    const decoded = response.map(a => [a.publicKey, a.accountInfo.owner, OpenOrdersLayout.decode(a.accountInfo.data)]);

    console.log('OpenOrdersAccounts decoded', decoded);

    setOpenOrdersAccounts(decoded);
  }


  function calculateInterest(nativeTotalDeposits, nativeTotalBorrows) {
    // interest function is not complete yet; returning 1% per year right now
    let optimalUtil = 0.7;
    let optimalInterest = 0.10;
    let maxInterest = 1;
    if (nativeTotalDeposits < nativeTotalBorrows || nativeTotalDeposits === 0) {
      return maxInterest;
    }
    let utilization = nativeTotalBorrows / nativeTotalDeposits;
    if (utilization > optimalUtil) {
      let extraUtil = utilization - optimalUtil;
      let slope = (maxInterest - optimalInterest) / (1 - optimalUtil);
      return optimalInterest + slope * extraUtil;
    } else {
      let slope = optimalInterest / optimalUtil;
      return slope * utilization;
    }
  }

  async function fetchMangoGroup() {
    if (!wallet.publicKey || !connection || !connected) {
      console.error('ensure wallet is connected', wallet, connection, connected);
      return
    }

    let mangoGroupPk = new PublicKey(config.mango_groups["BTC_ETH_USDC"].mango_group_pk);
    let mangoGroupInfo = await connection.getAccountInfo(mangoGroupPk);
    let mangoGroup = MangoGroupLayout.decode(mangoGroupInfo.data);

    return mangoGroup;
  }

  function u64f64BytesToFloat(bytes) {
    if (bytes.length !== 16) {
      throw 'Not a valid u64f64 bytes representation';
    }
    let val = 0;
    for (let i = 0; i < bytes.length; i++) {
      val += Math.pow(256, i - 8) * bytes[i];
    }
    // TODO check overflows and correct data type; do some testing on this as well
    return val;
  }

  async function getBalances() {
    if (!wallet.publicKey || !connection || !connected) {
      console.error('ensure wallet is connected', wallet, connection, connected);
      return
    }
    let mangoGroupName = "BTC_ETH_USDC";
    let assetNames = mangoGroupName.split('_');

    let mangoGroup = await fetchMangoGroup();
    let marginAccount = marginAccounts[0][1];
    for (let i = 0; i < NUM_TOKENS; i++) {
      // Get mint info for this token
      let tokenMintPk = new PublicKey(config.symbols[assetNames[i]]);
      let tokenMintInfo = await connection.getAccountInfo(tokenMintPk);
      let tokenMint = MINT_LAYOUT.decode(tokenMintInfo.data);

      // Get MangoIndex values
      let index = MangoIndexLayout.decode(mangoGroup.indexes.slice(i * MangoIndexLayout.span, (i+1) * MangoIndexLayout.span));
      let depIndex = u64f64BytesToFloat(index.deposit);
      let borrIndex = u64f64BytesToFloat(index.borrow);

      let deposit = u64f64BytesToFloat(marginAccount.deposits.slice(i * 16, (i+1) * 16));  // adjusted deposit
      let nativeDeposit = deposit * depIndex;  // actual deposits in token terms
      let uiDeposit = nativeDeposit / Math.pow(10, tokenMint.decimals);  // user interface version adjusting for decimals

      // Determine interest rate as a function of native total deposits and native total borrows
      let totalDeposits = u64f64BytesToFloat(mangoGroup.total_deposits.slice(i * 16, (i+1) * 16));
      let totalBorrows = u64f64BytesToFloat(mangoGroup.total_borrows.slice(i * 16, (i+1) * 16));
      let nativeTotalDeposits = totalDeposits * depIndex;
      let nativeTotalBorrows = totalBorrows * borrIndex;

      console.log(assetNames[i], uiDeposit, 'interest', 100 * calculateInterest(nativeTotalDeposits, nativeTotalBorrows));
    }


  }

  async function initMarginAccount() {
    const dex_program_id = new PublicKey(config.dex_program_id);
    const mango_program_id = new PublicKey(config.mango_program_id);
    const mango_group_name = "BTC_ETH_USDC";
    const mango_group_config = config.mango_groups[mango_group_name];
    const mango_group_pk = new PublicKey(mango_group_config.mango_group_pk);
    const spot_market_pks = mango_group_config.spot_market_pks.map( pk => new PublicKey(pk) );

    // create instructions
    console.log('create MarginAccount', MarginAccountLayout.span);
    const mango_account = await createAccountInstruction(MarginAccountLayout.span, mango_program_id);
    console.log('create OpenOrders', OpenOrdersLayout.span);
    const open_orders = await Promise.all(spot_market_pks.map(_ => createAccountInstruction(OpenOrdersLayout.span, dex_program_id)));

    console.log('dex_program_id', dex_program_id.toString());
    console.log('mango_program_id', mango_program_id.toString());
    console.log('mango_account', mango_account);
    console.log('open_orders', open_orders);
    async function initMarginAccountInstruction(programId) {
      let keys = [
        { isSigner: false, isWritable: false, pubkey: mango_group_pk},
        { isSigner: false, isWritable: true,  pubkey: mango_account.account.publicKey },
        { isSigner: true,  isWritable: false, pubkey: wallet.publicKey },
        { isSigner: false, isWritable: false, pubkey: SYSVAR_RENT_PUBKEY },
        ...open_orders.map( (o) => ({ isSigner: false, isWritable: false, pubkey: o.account.publicKey }) )
      ];
      let data = encodeMangoInstruction({ InitMarginAccount: {} });
      return new TransactionInstruction({ keys, data, programId });
    };

    const init_mango_account = await initMarginAccountInstruction(mango_program_id);
    console.log('init_mango_account', init_mango_account);

    // build transaction
    const transaction = new Transaction();
    transaction.add(mango_account.instruction);
    transaction.add(...open_orders.map( o => o.instruction ));
    transaction.add(init_mango_account);

    const additionalSigners = [
      mango_account.account,
      ...open_orders.map( o => o.account ),
    ];

    console.log('sending initMarginAccount', transaction, additionalSigners);
    const txid = await sendTransaction(transaction, additionalSigners);
    console.log('sent initMarginAccount:', txid);
  }

  async function deposit(amount) {
  }

  function handleOnChange(e) {
    setNetwork(e.target.value);
  }

  function renderAccount(a) {
    return (
      <>
        {a[0].toString()}: {a[1].toString()}
        <br />
      </> );
  }

  return (
    <div className="App">
      <header className="navbar is-fixed-top is-spaced">
        <div className="navbar-end">
          <div className="connection field has-addons">
            <div className="control is-expanded">
              <div className="select is-fullwidth">
                <select id="network" onChange={handleOnChange} value={network}>
                  { Object.keys(ID.cluster_urls).map( (k) => <option value={k}>{k}</option> ) }
                </select>
              </div>
            </div>
            <div className="control">
              <button className="button is-primary" disabled={wallet.connected} onClick={() => wallet.connect()}>Connect</button>
            </div>
          </div>
        </div>
      </header>
      <main>
        <div className="box">
          <button className="button" disabled={!connected} onClick={fetchSPLAccounts}>
             Fetch SPL Accounts
          </button>
          <p>
            Needs to be connected: { connected ? "✅" : "❌" }
          </p>
          <pre>
            { accounts.map( renderAccount ) }
          </pre>
        </div>

        <div className="box">
          <button className="button" disabled={!connected} onClick={fetchMarginAccounts}>
             Fetch Margin Accounts
          </button>
          <p>
            Needs to be connected: { connected ? "✅" : "❌" }
          </p>
          <pre>
            { marginAccounts && marginAccounts.map( renderAccount ) }
          </pre>
        </div>

        <div className="box">
          <button className="button" disabled={!connected} onClick={fetchOpenOrdersAccounts}>
             Fetch Open Orders Accounts
          </button>
          <p>
            Needs to be connected: { connected ? "✅" : "❌" }
          </p>
          <pre>
            { openOrdersAccounts && openOrdersAccounts.map( renderAccount ) }
          </pre>
        </div>


        <div className="box">
          <button className="button" disabled={!connected} onClick={initMarginAccount}>
             Init Margin Account
          </button>
          <p>
            Needs to be connected: { connected ? "✅" : "❌" }
          </p>
        </div>

        <div className="box">
          <div className="content action-box is-spaced">
            <AmountInput label="Deposit" icon='mdi-currency-usd-circle-outline' action={deposit} disabled={!connected} />
            <p className="instructions">
              Deposit USDC as a collateral.
            </p>
          </div>
        </div>

        <div className="box">
          <button className="button" disabled={!connected} onClick={getBalances}>
            Get balances
          </button>
          <p>
            Needs to be connected: { connected ? "✅" : "❌" }
          </p>
        </div>
      </main>
    </div>
  );
}


export default App;
