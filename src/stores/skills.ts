import { desktopApi } from '@/lib/desktop/api';
/**
 * Skills State Store
 * Manages skill/plugin state
 */
import { create } from 'zustand';
import type { Skill, MarketplaceSkill } from '../types/skill';

const MARKETPLACE_CACHE_KEY = 'clawy:skills-marketplace-cache:v1';
const MARKETPLACE_CACHE_TTL_MS = 15 * 60 * 1000;
const MARKETPLACE_CACHE_MAX_ENTRIES = 20;

type GatewaySkillStatus = {
  skillKey: string;
  slug?: string;
  name?: string;
  description?: string;
  disabled?: boolean;
  emoji?: string;
  version?: string;
  author?: string;
  config?: Record<string, unknown>;
  bundled?: boolean;
  always?: boolean;
};

type GatewaySkillsStatusResult = {
  skills?: GatewaySkillStatus[];
};

type GatewayRpcResponse<T> = {
  success: boolean;
  result?: T;
  error?: string;
};

type ClawHubListResult = {
  slug: string;
  version?: string;
};

type MarketplaceCacheEntry = {
  results: MarketplaceSkill[];
  updatedAt: number;
};

type MarketplaceCache = Record<string, MarketplaceCacheEntry>;

function normalizeErrorKey(
  error: unknown,
  {
    timeoutKey,
    rateLimitKey,
  }: {
    timeoutKey: string;
    rateLimitKey: string;
  },
): string {
  const raw = error instanceof Error ? error.message : String(error);
  const normalized = raw.replace(/^Error:\s*/i, '').trim();
  const lowered = normalized.toLowerCase();

  if (lowered.includes('timeout')) {
    return timeoutKey;
  }
  if (lowered.includes('rate limit')) {
    return rateLimitKey;
  }

  return normalized || raw;
}

function loadMarketplaceCache(): MarketplaceCache {
  if (typeof window === 'undefined') {
    return {};
  }

  try {
    const raw = window.localStorage.getItem(MARKETPLACE_CACHE_KEY);
    if (!raw) {
      return {};
    }

    const parsed = JSON.parse(raw) as MarketplaceCache;
    if (!parsed || typeof parsed !== 'object') {
      return {};
    }

    return parsed;
  } catch {
    return {};
  }
}

function saveMarketplaceCache(cache: MarketplaceCache): void {
  if (typeof window === 'undefined') {
    return;
  }

  try {
    const trimmedEntries = Object.entries(cache)
      .sort((a, b) => b[1].updatedAt - a[1].updatedAt)
      .slice(0, MARKETPLACE_CACHE_MAX_ENTRIES);

    window.localStorage.setItem(
      MARKETPLACE_CACHE_KEY,
      JSON.stringify(Object.fromEntries(trimmedEntries)),
    );
  } catch {
    // Ignore cache persistence failures.
  }
}

function getCachedMarketplaceResults(query: string, allowStale = false): MarketplaceCacheEntry | null {
  const cacheKey = query.trim().toLowerCase();
  if (!cacheKey && query.trim() !== '') {
    return null;
  }

  const cache = loadMarketplaceCache();
  const entry = cache[cacheKey];
  if (!entry) {
    return null;
  }

  if (!allowStale && Date.now() - entry.updatedAt > MARKETPLACE_CACHE_TTL_MS) {
    return null;
  }

  return entry;
}

function cacheMarketplaceResults(query: string, results: MarketplaceSkill[]): void {
  const cacheKey = query.trim().toLowerCase();
  const cache = loadMarketplaceCache();
  cache[cacheKey] = {
    results,
    updatedAt: Date.now(),
  };
  saveMarketplaceCache(cache);
}

interface SkillsState {
  skills: Skill[];
  searchResults: MarketplaceSkill[];
  loading: boolean;
  searching: boolean;
  searchError: string | null;
  installing: Record<string, boolean>; // slug -> boolean
  error: string | null;

  // Actions
  fetchSkills: () => Promise<void>;
  searchSkills: (query: string) => Promise<void>;
  installSkill: (slug: string, version?: string) => Promise<void>;
  uninstallSkill: (slug: string) => Promise<void>;
  enableSkill: (skillId: string) => Promise<void>;
  disableSkill: (skillId: string) => Promise<void>;
  setSkills: (skills: Skill[]) => void;
  updateSkill: (skillId: string, updates: Partial<Skill>) => void;
}

export const useSkillsStore = create<SkillsState>((set, get) => ({
  skills: [],
  searchResults: [],
  loading: false,
  searching: false,
  searchError: null,
  installing: {},
  error: null,

  fetchSkills: async () => {
    // Only show loading state if we have no skills yet (initial load)
    if (get().skills.length === 0) {
      set({ loading: true, error: null });
    }
    try {
      // 1. Fetch from Gateway (running skills)
      const gatewayResult = await desktopApi.ipcRenderer.invoke(
        'gateway:rpc',
        'skills.status'
      ) as GatewayRpcResponse<GatewaySkillsStatusResult>;

      // 2. Fetch from ClawHub (installed on disk)
      const clawhubResult = await desktopApi.ipcRenderer.invoke(
        'clawhub:list'
      ) as { success: boolean; results?: ClawHubListResult[]; error?: string };

      // 3. Fetch persisted desktop-side configuration (Gateway doesn't return it)
      const configResult = await desktopApi.ipcRenderer.invoke(
        'skill:getAllConfigs'
      ) as Record<string, { apiKey?: string; env?: Record<string, string> }>;

      let combinedSkills: Skill[] = [];
      const currentSkills = get().skills;

      // Map gateway skills info
      if (gatewayResult.success && gatewayResult.result?.skills) {
        combinedSkills = gatewayResult.result.skills.map((s: GatewaySkillStatus) => {
          // Merge with direct config if available
          const directConfig = configResult[s.skillKey] || {};

          return {
            id: s.skillKey,
            slug: s.slug || s.skillKey,
            name: s.name || s.skillKey,
            description: s.description || '',
            enabled: !s.disabled,
            icon: s.emoji || '📦',
            version: s.version || '1.0.0',
            author: s.author,
            config: {
              ...(s.config || {}),
              ...directConfig,
            },
            isCore: s.bundled && s.always,
            isBundled: s.bundled,
          };
        });
      } else if (currentSkills.length > 0) {
        // ... if gateway down ...
        combinedSkills = [...currentSkills];
      }

      // Merge with ClawHub results
      if (clawhubResult.success && clawhubResult.results) {
        clawhubResult.results.forEach((cs: ClawHubListResult) => {
          const existing = combinedSkills.find(s => s.id === cs.slug);
          if (!existing) {
            const directConfig = configResult[cs.slug] || {};
            combinedSkills.push({
              id: cs.slug,
              slug: cs.slug,
              name: cs.slug,
              description: 'Recently installed, initializing...',
              enabled: false,
              icon: '⌛',
              version: cs.version || 'unknown',
              author: undefined,
              config: directConfig,
              isCore: false,
              isBundled: false,
            });
          }
        });
      }

      set({ skills: combinedSkills, loading: false });
    } catch (error) {
      console.error('Failed to fetch skills:', error);
      let errorMsg = error instanceof Error ? error.message : String(error);
      if (errorMsg.includes('Timeout')) {
        errorMsg = 'timeoutError';
      } else if (errorMsg.toLowerCase().includes('rate limit')) {
        errorMsg = 'rateLimitError';
      }
      set({ loading: false, error: errorMsg });
    }
  },

  searchSkills: async (query: string) => {
    const normalizedQuery = query.trim();
    const cached = getCachedMarketplaceResults(normalizedQuery);
    if (cached) {
      set({
        searchResults: cached.results,
        searchError: null,
        searching: false,
      });
      return;
    }

    set({ searching: true, searchError: null });
    try {
      const result = await desktopApi.ipcRenderer.invoke(
        'clawhub:search',
        {
          query: normalizedQuery,
          limit: normalizedQuery ? 20 : 30,
        },
      ) as { success: boolean; results?: MarketplaceSkill[]; error?: string };
      if (result.success) {
        const results = result.results || [];
        cacheMarketplaceResults(normalizedQuery, results);
        set({ searchResults: results });
      } else {
        throw new Error(normalizeErrorKey(result.error || 'Search failed', {
          timeoutKey: 'searchTimeoutError',
          rateLimitKey: 'searchRateLimitError',
        }));
      }
    } catch (error) {
      const normalizedError = normalizeErrorKey(error, {
        timeoutKey: 'searchTimeoutError',
        rateLimitKey: 'searchRateLimitError',
      });
      const fallbackResults = getCachedMarketplaceResults(normalizedQuery, true)?.results;

      set({
        searchError: fallbackResults?.length ? null : normalizedError,
        searchResults: fallbackResults?.length ? fallbackResults : get().searchResults,
      });
    } finally {
      set({ searching: false });
    }
  },

  installSkill: async (slug: string, version?: string) => {
    set((state) => ({ installing: { ...state.installing, [slug]: true } }));
    try {
      const result = await desktopApi.ipcRenderer.invoke('clawhub:install', { slug, version }) as { success: boolean; error?: string };
      if (!result.success) {
        if (result.error?.includes('Timeout')) {
          throw new Error('installTimeoutError');
        }
        if (result.error?.toLowerCase().includes('rate limit')) {
          throw new Error('installRateLimitError');
        }
        throw new Error(result.error || 'Install failed');
      }
      // Refresh skills after install
      await get().fetchSkills();
    } catch (error) {
      console.error('Install error:', error);
      throw error;
    } finally {
      set((state) => {
        const newInstalling = { ...state.installing };
        delete newInstalling[slug];
        return { installing: newInstalling };
      });
    }
  },

  uninstallSkill: async (slug: string) => {
    set((state) => ({ installing: { ...state.installing, [slug]: true } }));
    try {
      const result = await desktopApi.ipcRenderer.invoke('clawhub:uninstall', { slug }) as { success: boolean; error?: string };
      if (!result.success) {
        throw new Error(result.error || 'Uninstall failed');
      }
      // Refresh skills after uninstall
      await get().fetchSkills();
    } catch (error) {
      console.error('Uninstall error:', error);
      throw error;
    } finally {
      set((state) => {
        const newInstalling = { ...state.installing };
        delete newInstalling[slug];
        return { installing: newInstalling };
      });
    }
  },

  enableSkill: async (skillId) => {
    const { updateSkill } = get();

    try {
      const result = await desktopApi.ipcRenderer.invoke(
        'gateway:rpc',
        'skills.update',
        { skillKey: skillId, enabled: true }
      ) as GatewayRpcResponse<unknown>;

      if (result.success) {
        updateSkill(skillId, { enabled: true });
      } else {
        throw new Error(result.error || 'Failed to enable skill');
      }
    } catch (error) {
      console.error('Failed to enable skill:', error);
      throw error;
    }
  },

  disableSkill: async (skillId) => {
    const { updateSkill, skills } = get();

    const skill = skills.find((s) => s.id === skillId);
    if (skill?.isCore) {
      throw new Error('Cannot disable core skill');
    }

    try {
      const result = await desktopApi.ipcRenderer.invoke(
        'gateway:rpc',
        'skills.update',
        { skillKey: skillId, enabled: false }
      ) as GatewayRpcResponse<unknown>;

      if (result.success) {
        updateSkill(skillId, { enabled: false });
      } else {
        throw new Error(result.error || 'Failed to disable skill');
      }
    } catch (error) {
      console.error('Failed to disable skill:', error);
      throw error;
    }
  },

  setSkills: (skills) => set({ skills }),

  updateSkill: (skillId, updates) => {
    set((state) => ({
      skills: state.skills.map((skill) =>
        skill.id === skillId ? { ...skill, ...updates } : skill
      ),
    }));
  },
}));
