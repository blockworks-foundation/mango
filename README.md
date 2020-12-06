# 🥭 Mango Margin

## 💫 Vision

We want to enable margin trading on the Serum with a focus on usability. Towards that end, Leverum tries to achieve the following design goals:

1. Hidden and automatic management of borrows when taking on a margin position
2. Easy to use graphical tools to automatically lend user funds at current market rates
3. Liquidity for borrowers on day 1
4. Execution of all trades on Serum's spot markets (incl. liquidations)

## 💸  Bond Market

The trader may issue bonds via Leverum given they deposit sufficient collateral in their margin account. The margin account is guarded by the Leverum program, which continuously calculates a fair valuation of the collateral and the debt taken on. The margin account can be accessed by the borrower to trade on Serum's regular spot markets, as well as by possible liquidators in a margin call scenario.
