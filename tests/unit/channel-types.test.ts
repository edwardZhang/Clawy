import { describe, expect, it } from 'vitest';
import { CHANNEL_META, getPrimaryChannels } from '@/types/channel';

describe('channel types', () => {
  it('includes matrix in the primary channel list', () => {
    expect(getPrimaryChannels()).toContain('matrix');
  });

  it('includes qqbot, wecom, and wecom-app in the primary channel list', () => {
    expect(getPrimaryChannels()).toEqual(
      expect.arrayContaining(['qqbot', 'wecom', 'wecom-app'])
    );
  });

  it('keeps matrix marked as a plugin-backed channel', () => {
    expect(CHANNEL_META.matrix.isPlugin).toBe(true);
  });

  it('marks china reference channels as plugin-backed channels', () => {
    expect(CHANNEL_META.qqbot.isPlugin).toBe(true);
    expect(CHANNEL_META.wecom.isPlugin).toBe(true);
    expect(CHANNEL_META['wecom-app'].isPlugin).toBe(true);
  });
});
