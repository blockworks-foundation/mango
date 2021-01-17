// @ts-nocheck

import {struct, blob, nu64, union, u32, Layout, bits, Blob, seq } from 'buffer-layout';
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

export function publicKeyLayout(property) {
  return new PublicKeyLayout(property);
}

class BNLayout extends Blob {
  decode(b, offset) {
    return new BN(super.decode(b, offset), 10, 'le');
  }

  encode(src, b, offset) {
    return super.encode(src.toArrayLike(Buffer, 'le', this.span), b, offset);
  }
}

export function u64(property) {
  return new BNLayout(8, property);
}

export function u128(property) {
  return new BNLayout(16, property);
}


class U64F64Layout extends Blob {
  decode(b, offset) {
    let raw = new BN(super.decode(b, offset), 10, 'le');
    return raw / Math.pow(2, 64);
  }

  encode(src, b, offset) {
    return super.encode(src.toArrayLike(Buffer, 'le', this.span), b, offset);
  }
}

export function U64F64(property) {
  return new U64F64Layout(16, property)
}

export class WideBits extends Layout {
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

  encode(src, b, offset = 0) {
    return (
      this._lower.encode(src, b, offset) +
      this._upper.encode(src, b, offset + this._lower.span)
    );
  }
}
const ACCOUNT_FLAGS_LAYOUT = new WideBits();
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
  accountFlagsLayout('account_flags'),
  seq(publicKeyLayout(), NUM_TOKENS, 'tokens'),
  seq(publicKeyLayout(), NUM_TOKENS, 'vaults'),
  seq(MangoIndexLayout.replicate(), NUM_TOKENS, 'indexes'),
  seq(publicKeyLayout(), NUM_MARKETS, 'spot_markets'),
  seq(publicKeyLayout(), NUM_MARKETS, 'oracles'),

  u64('signer_nonce'),
  publicKeyLayout('signer_key'),
  publicKeyLayout('dex_program_id'),
  seq(U64F64(), NUM_TOKENS, 'total_deposits'),
  seq(U64F64(), NUM_TOKENS, 'total_borrows'),
  U64F64('maint_coll_ratio'),
  U64F64('init_coll_ratio')
]);


export const MarginAccountLayout = struct([
  accountFlagsLayout('account_flags'),
  publicKeyLayout('mango_group'),
  publicKeyLayout('owner'),

  seq(U64F64(), NUM_TOKENS, 'total_deposits'),
  seq(U64F64(), NUM_TOKENS, 'total_borrows'),
  seq(u64(), NUM_TOKENS, 'total_deposits'),
  seq(publicKeyLayout(), NUM_MARKETS, 'open_orders')
]);

export const OpenOrdersLayout = struct([
  blob(5, 'head_padding'),
  nu64('account_flags'),
  blob(32, 'market'),
  blob(32, 'owner'),
  nu64('native_coin_free'),
  nu64('native_coin_total'),
  nu64('native_pc_free'),
  nu64('native_pc_total'),
  blob(16, 'free_slot_bits'),
  blob(16, 'is_bid_bits'),
  blob(16*128, 'orders'),
  blob(8*128, 'client_order_ids'),
  nu64('referrer_rebates_accrued'),
  blob(7, 'tail_padding'),
]);

export const MangoInstructionLayout = union(u32('instruction'));

MangoInstructionLayout.addVariant(0, struct([]), 'InitMangoGroup');
MangoInstructionLayout.addVariant(1, struct([]), 'InitMarginAccount');
MangoInstructionLayout.addVariant(2, struct([nu64('quantity')]), 'Deposit');
MangoInstructionLayout.addVariant(3, struct([nu64('quantity')]), 'Withdraw');

const instructionMaxSpan = Math.max(...Object.values(MangoInstructionLayout.registry).map((r) => r.span));
export function encodeMangoInstruction(data) {
  const b = Buffer.alloc(instructionMaxSpan);
  const span = MangoInstructionLayout.encode(data, b);
  return b.slice(0, span);
}

