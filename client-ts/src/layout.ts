
import {struct, u8, blob, union, u32, Layout, bits, Blob, seq, BitStructure, UInt } from 'buffer-layout';
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
    // restore prototype chain
    Object.setPrototypeOf(this, new.target.prototype)
  }

  decode(b, offset) {
    return new BN(super.decode(b, offset), 10, 'le');
  }

  encode(src, b, offset) {
    return super.encode(src.toArrayLike(Buffer, 'le', this['span']), b, offset);
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
    return super.encode(src.toArrayLike(Buffer, 'le', this['span']), b, offset);
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
  // seq(u64(), NUM_TOKENS, 'positions'),
  seq(publicKeyLayout(), NUM_MARKETS, 'openOrders')
]);


class EnumLayout extends UInt {
  values: any;
  constructor(values, span, property) {
    super(span, property);
    this.values = values
  }
  encode(src, b, offset) {
    if (this.values[src] !== undefined) {
      return super.encode(this.values[src], b, offset);
    }
    throw new Error('Invalid ' + this['property']);
  }

  decode(b, offset) {
    const decodedValue = super.decode(b, offset);
    const entry = Object.entries(this.values).find(
      ([, value]) => value === decodedValue,
    );
    if (entry) {
      return entry[0];
    }
    throw new Error('Invalid ' + this['property']);
  }
}

export function sideLayout(property) {
  return new EnumLayout({ buy: 0, sell: 1 }, 4, property);
}

export function orderTypeLayout(property) {
  return new EnumLayout({ limit: 0, ioc: 1, postOnly: 2 }, 4, property);
}

export function selfTradeBehaviorLayout(property) {
  return new EnumLayout({ decrementTake: 0, cancelProvide: 1 }, 4, property);
}

export const MangoInstructionLayout = union(u32('instruction'))

MangoInstructionLayout.addVariant(0, struct([]), 'InitMangoGroup')
MangoInstructionLayout.addVariant(1, struct([]), 'InitMarginAccount')
MangoInstructionLayout.addVariant(2, struct([u64('quantity')]), 'Deposit')
MangoInstructionLayout.addVariant(3, struct([u64('tokenIndex'), u64('quantity')]), 'Withdraw')
MangoInstructionLayout.addVariant(4, struct([u64('tokenIndex'), u64('quantity')]), 'Borrow')
MangoInstructionLayout.addVariant(5, struct([u64('tokenIndex'), u64('quantity')]), 'SettleBorrow')
MangoInstructionLayout.addVariant(6, struct([seq(u64(), NUM_TOKENS, 'depositQuantities')]), 'Liquidate')

MangoInstructionLayout.addVariant(7,
  struct(
    [
      sideLayout('side'),
      u64('limitPrice'),
      u64('maxQuantity'),
      orderTypeLayout('orderType'),
      u64('clientId'),
      selfTradeBehaviorLayout('selfTradeBehavior')
    ]
  ),
  'PlaceOrder'
)

MangoInstructionLayout.addVariant(8, struct([]), 'SettleFunds')
MangoInstructionLayout.addVariant(9,
  struct(
    [
      sideLayout('side'),
      u128('orderId'),
      publicKeyLayout('openOrders'),
      u8('openOrdersSlot')
    ]
  ),
  'CancelOrder'
)

MangoInstructionLayout.addVariant(10, struct([]), 'CancelOrderByClientId')

// @ts-ignore
const instructionMaxSpan = Math.max(...Object.values(MangoInstructionLayout.registry).map((r) => r.span));
export function encodeMangoInstruction(data) {
  const b = Buffer.alloc(instructionMaxSpan);
  const span = MangoInstructionLayout.encode(data, b);
  return b.slice(0, span);
}
