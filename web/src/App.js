import React, {useEffect, useMemo, useState} from 'react';

import { Connection } from '@solana/web3.js';
import { TokenInstructions } from '@project-serum/serum';
import Wallet from '@project-serum/sol-wallet-adapter';

import AmountInput from './components/AmountInput';

import ID from './ids.json';

import './App.css';


function App() {

  const [connected, setConnected] = useState(false);
  const [network, setNetwork] = useState('devnet');
  const config = useMemo(() => ID[network], [network]);
  const clusterUrl = useMemo(() => ID.cluster_urls[network], [network]);
  const connection = useMemo(() => new Connection(clusterUrl), [clusterUrl]);
  const wallet = useMemo(() => new Wallet('https://www.sollet.io', clusterUrl), [clusterUrl]);

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
  async function fetchAccounts() {
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

  async function deposit() {
  }

  function handleOnChange(e) {
    setNetwork(e.target.value);
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
          <button className="button" disabled={!connected} onClick={fetchAccounts}>
             Fetch SPL Accounts
          </button>
          <p>
            Needs to be connected: { connected ? "✅" : "❌" }
          </p>
          <pre>
            { accounts.map( (a) => <>{a[0]}: {a[1]}<br /></> ) }
          </pre>
        </div>

        <div className="box">
          <div className="content action-box is-spaced">
            <AmountInput label="Deposit" icon='mdi-currency-usd-circle-outline' action={deposit} disabled={!connected} />
            <p className="instructions">
              Deposit USDC as a collateral.
            </p>
          </div>
        </div>
      </main>
    </div>
  );
}


export default App;
