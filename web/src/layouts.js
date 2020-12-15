import BufferLayout from 'buffer-layout';

const NUM_TOKENS = 3;
export const MarginAccountLayout = BufferLayout.struct([
  BufferLayout.nu64('account_flags'),
  BufferLayout.blob(32, 'mango_group'),
  BufferLayout.blob(32, 'owner'),
  BufferLayout.blob(16*NUM_TOKENS, 'deposits'),
  BufferLayout.blob(16*NUM_TOKENS, 'borrows'),
  BufferLayout.blob(8*NUM_TOKENS, 'positions'),
  BufferLayout.blob(32*(NUM_TOKENS-1), 'open_orders'),
]);

export const OpenOrdersLayout = BufferLayout.struct([
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
  BufferLayout.blob(12, 'padding'),
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

