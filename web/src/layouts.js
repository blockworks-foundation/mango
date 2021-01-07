import BufferLayout from 'buffer-layout';

export const NUM_TOKENS = 3;
export const NUM_MARKETS = NUM_TOKENS - 1;

export const MangoIndexLayout = BufferLayout.struct([
  BufferLayout.nu64('last_update'),
  BufferLayout.blob(16, 'borrow'), // U64F64
  BufferLayout.blob(16, 'deposit')  // U64F64
]);

export const MangoGroupLayout = BufferLayout.struct([
  BufferLayout.nu64('account_flags'),
  BufferLayout.blob(32 * NUM_TOKENS, 'tokens'),
  BufferLayout.blob(32 * NUM_TOKENS, 'vaults'),
  BufferLayout.blob(MangoIndexLayout.span * NUM_TOKENS, 'indexes'),
  BufferLayout.blob(32 * NUM_MARKETS, 'spot_markets'),
  BufferLayout.blob(32 * NUM_MARKETS, 'oracles'),
  BufferLayout.nu64('signer_nonce'),
  BufferLayout.blob(32, 'signer_key'),
  BufferLayout.blob(32, 'dex_program_id'),

  BufferLayout.blob(16 * NUM_TOKENS, 'total_deposits'),
  BufferLayout.blob(16 * NUM_TOKENS, 'total_borrows'),
  BufferLayout.blob(16, 'maint_coll_ratio'),
  BufferLayout.blob(16, 'init_coll_ratio'),
]);




export const MarginAccountLayout = BufferLayout.struct([
  BufferLayout.nu64('account_flags'),
  BufferLayout.blob(32, 'mango_group'),
  BufferLayout.blob(32, 'owner'),
  BufferLayout.blob(16*NUM_TOKENS, 'deposits'),
  BufferLayout.blob(16*NUM_TOKENS, 'borrows'),
  BufferLayout.blob(8*NUM_TOKENS, 'positions'),
  BufferLayout.blob(32*NUM_MARKETS, 'open_orders'),
]);

export const OpenOrdersLayout = BufferLayout.struct([
  BufferLayout.blob(5, 'head_padding'),
  BufferLayout.nu64('account_flags'),
  BufferLayout.blob(32, 'market'),
  BufferLayout.blob(32, 'owner'),
  BufferLayout.nu64('native_coin_free'),
  BufferLayout.nu64('native_coin_total'),
  BufferLayout.nu64('native_pc_free'),
  BufferLayout.nu64('native_pc_total'),
  BufferLayout.blob(16, 'free_slot_bits'),
  BufferLayout.blob(16, 'is_bid_bits'),
  BufferLayout.blob(16*128, 'orders'),
  BufferLayout.blob(8*128, 'client_order_ids'),
  BufferLayout.nu64('referrer_rebates_accrued'),
  BufferLayout.blob(7, 'tail_padding'),
]);

export const MangoInstructionLayout = BufferLayout.union(BufferLayout.u32('instruction'));

MangoInstructionLayout.addVariant(0, BufferLayout.struct([]), 'InitMangoGroup');
MangoInstructionLayout.addVariant(1, BufferLayout.struct([]), 'InitMarginAccount');
MangoInstructionLayout.addVariant(2, BufferLayout.struct([BufferLayout.nu64('quantity')]), 'Deposit');
MangoInstructionLayout.addVariant(3, BufferLayout.struct([BufferLayout.nu64('quantity')]), 'Withdraw');

const instructionMaxSpan = Math.max(...Object.values(MangoInstructionLayout.registry).map((r) => r.span));
export function encodeMangoInstruction(data) {
  const b = Buffer.alloc(instructionMaxSpan);
  const span = MangoInstructionLayout.encode(data, b);
  return b.slice(0, span);
}

