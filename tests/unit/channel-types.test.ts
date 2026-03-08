import { describe, expect, it } from 'vitest';
import { CHANNEL_META, getPrimaryChannels } from '@/types/channel';

describe('channel types', () => {
  it('includes matrix in the primary channel list', () => {
    expect(getPrimaryChannels()).toContain('matrix');
  });

  it('keeps matrix marked as a plugin-backed channel', () => {
    expect(CHANNEL_META.matrix.isPlugin).toBe(true);
  });
});
