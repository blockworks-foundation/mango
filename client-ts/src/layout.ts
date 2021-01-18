
import {struct, blob, nu64, union, u32, Layout, bits, Blob, seq, BitStructure } from 'buffer-layout';
import { PublicKey } from '@solana/web3.js';
import BN from 'bn.js';

export const NUM_TOKENS = 3;
export const NUM_MARKETS = NUM_TOKENS - 1;

class PublicKeyLayout extends Blob {
  constructor(property) {
    super(32, property);
  }

  decode(b, offset) {
    return new PublicKey(super.decode(b, offset));
  }

  encode(src, b, offset) {
    return super.encode(src.toBuffer(), b, offset);
  }
}

export function publicKeyLayout(property = "") {
  return new PublicKeyLayout(property);
}

class BNLayout extends Blob {
  constructor(number: number, property) {
    super(number, property);
  }

  decode(b, offset) {
    return new BN(super.decode(b, offset), 10, 'le');
  }

  encode(src, b, offset) {
    return super.encode(src.toArrayLike(Buffer, 'le', super.span), b, offset);
  }
}

export function u64(property = "") {
  return new BNLayout(8, property);
}

export function u128(property = "") {
  return new BNLayout(16, property);
}


class U64F64Layout extends Blob {
  constructor(property: string) {
    super(16, property);
  }

  decode(b, offset) {
    const raw = new BN(super.decode(b, offset), 10, 'le');

    // @ts-ignore
    return raw / Math.pow(2, 64);
  }

  encode(src, b, offset) {
    return super.encode(src.toArrayLike(Buffer, 'le', super.span), b, offset);
  }
}

export function U64F64(property = "") {
  return new U64F64Layout(property)
}

export class WideBits extends Layout {
  _lower: BitStructure;
  _upper: BitStructure;

  constructor(property) {
    super(8, property);
    this._lower = bits(u32(), false);
    this._upper = bits(u32(), false);
  }

  addBoolean(property) {
    if (this._lower.fields.length < 32) {
      this._lower.addBoolean(property);
    } else {
      this._upper.addBoolean(property);
    }
  }

  decode(b, offset = 0) {
    const lowerDecoded = this._lower.decode(b, offset);
    const upperDecoded = this._upper.decode(b, offset + this._lower.span);
    return { ...lowerDecoded, ...upperDecoded };
  }

  replicate(property: string) {
    return super.replicate(property);
  }
  encode(src, b, offset = 0) {
    return (
      this._lower.encode(src, b, offset) +
      this._upper.encode(src, b, offset + this._lower.span)
    );
  }
}
const ACCOUNT_FLAGS_LAYOUT = new WideBits(undefined);
ACCOUNT_FLAGS_LAYOUT.addBoolean('Initialized');
ACCOUNT_FLAGS_LAYOUT.addBoolean('MangoGroup');
ACCOUNT_FLAGS_LAYOUT.addBoolean('MarginAccount');

export function accountFlagsLayout(property = 'accountFlags') {
  return ACCOUNT_FLAGS_LAYOUT.replicate(property);  // TODO: when ts check is on, replicate throws error, doesn't compile
}

export const MangoIndexLayout = struct([
  u64('lastUpdate'),
  U64F64('borrow'), // U64F64
  U64F64('deposit')  // U64F64
]);

export const MangoGroupLayout = struct([
  accountFlagsLayout('accountFlags'),
  seq(publicKeyLayout(), NUM_TOKENS, 'tokens'),
  seq(publicKeyLayout(), NUM_TOKENS, 'vaults'),
  seq(MangoIndexLayout.replicate(), NUM_TOKENS, 'indexes'),
  seq(publicKeyLayout(), NUM_MARKETS, 'spotMarkets'),
  seq(publicKeyLayout(), NUM_MARKETS, 'oracles'),

  u64('signerNonce'),
  publicKeyLayout('signerKey'),
  publicKeyLayout('dexProgramId'),
  seq(U64F64(), NUM_TOKENS, 'totalDeposits'),
  seq(U64F64(), NUM_TOKENS, 'totalBorrows'),
  U64F64('maintCollRatio'),
  U64F64('initCollRatio')
]);


export const MarginAccountLayout = struct([
  accountFlagsLayout('accountFlags'),
  publicKeyLayout('mangoGroup'),
  publicKeyLayout('owner'),

  seq(U64F64(), NUM_TOKENS, 'deposits'),
  seq(U64F64(), NUM_TOKENS, 'borrows'),
  seq(u64(), NUM_TOKENS, 'positions'),
  seq(publicKeyLayout(), NUM_MARKETS, 'openOrders')
]);

export const MangoInstructionLayout = union(u32('instruction'));

MangoInstructionLayout.addVariant(0, struct([]), 'InitMangoGroup');
MangoInstructionLayout.addVariant(1, struct([]), 'InitMarginAccount');
MangoInstructionLayout.addVariant(2, struct([nu64('quantity')]), 'Deposit');
MangoInstructionLayout.addVariant(3, struct([nu64('quantity')]), 'Withdraw');


