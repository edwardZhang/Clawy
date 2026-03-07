import { describe, expect, it } from 'vitest';
import {
  PROVIDER_TYPES,
  PROVIDER_TYPE_INFO,
  resolveProviderApiKeyForSave,
  resolveProviderModelForSave,
  shouldShowProviderModelId,
} from '@/lib/providers';

describe('provider metadata', () => {
  it('includes ark in the frontend provider registry', () => {
    expect(PROVIDER_TYPES).toContain('ark');

    expect(PROVIDER_TYPE_INFO).toEqual(
      expect.arrayContaining([
        expect.objectContaining({
          id: 'ark',
          name: 'ByteDance Ark',
          requiresApiKey: true,
          defaultBaseUrl: 'https://ark.cn-beijing.volces.com/api/v3',
          showBaseUrl: true,
          showModelId: true,
        }),
      ])
    );
  });

  it('uses OpenAI-compatible Ollama default base URL', () => {
    expect(PROVIDER_TYPE_INFO).toEqual(
      expect.arrayContaining([
        expect.objectContaining({
          id: 'ollama',
          defaultBaseUrl: 'http://localhost:11434/v1',
          requiresApiKey: false,
          showBaseUrl: true,
          showModelId: true,
        }),
      ])
    );
  });

  it('only exposes OpenRouter and SiliconFlow model overrides in developer mode', () => {
    const openrouter = PROVIDER_TYPE_INFO.find((provider) => provider.id === 'openrouter');
    const siliconflow = PROVIDER_TYPE_INFO.find((provider) => provider.id === 'siliconflow');

    expect(openrouter).toMatchObject({
      showModelId: true,
      showModelIdInDevModeOnly: true,
      defaultModelId: 'anthropic/claude-opus-4.6',
    });
    expect(siliconflow).toMatchObject({
      showModelId: true,
      showModelIdInDevModeOnly: true,
      defaultModelId: 'deepseek-ai/DeepSeek-V3',
    });

    expect(shouldShowProviderModelId(openrouter, false)).toBe(false);
    expect(shouldShowProviderModelId(siliconflow, false)).toBe(false);
    expect(shouldShowProviderModelId(openrouter, true)).toBe(true);
    expect(shouldShowProviderModelId(siliconflow, true)).toBe(true);
  });

  it('only saves OpenRouter and SiliconFlow model overrides in developer mode', () => {
    const openrouter = PROVIDER_TYPE_INFO.find((provider) => provider.id === 'openrouter');
    const siliconflow = PROVIDER_TYPE_INFO.find((provider) => provider.id === 'siliconflow');
    const ark = PROVIDER_TYPE_INFO.find((provider) => provider.id === 'ark');

    expect(resolveProviderModelForSave(openrouter, 'openai/gpt-5', false)).toBeUndefined();
    expect(resolveProviderModelForSave(siliconflow, 'Qwen/Qwen3-Coder-480B-A35B-Instruct', false)).toBeUndefined();

    expect(resolveProviderModelForSave(openrouter, 'openai/gpt-5', true)).toBe('openai/gpt-5');
    expect(resolveProviderModelForSave(siliconflow, 'Qwen/Qwen3-Coder-480B-A35B-Instruct', true)).toBe('Qwen/Qwen3-Coder-480B-A35B-Instruct');

    expect(resolveProviderModelForSave(openrouter, '   ', true)).toBe('anthropic/claude-opus-4.6');
    expect(resolveProviderModelForSave(siliconflow, '   ', true)).toBe('deepseek-ai/DeepSeek-V3');
    expect(resolveProviderModelForSave(ark, '  ep-custom-model  ', false)).toBe('ep-custom-model');
  });

  it('normalizes provider API keys for save flow', () => {
    expect(resolveProviderApiKeyForSave('ollama', '')).toBe('ollama-local');
    expect(resolveProviderApiKeyForSave('ollama', '   ')).toBe('ollama-local');
    expect(resolveProviderApiKeyForSave('ollama', 'real-key')).toBe('real-key');
    expect(resolveProviderApiKeyForSave('openai', '')).toBeUndefined();
    expect(resolveProviderApiKeyForSave('openai', ' sk-test ')).toBe('sk-test');
  });
});
