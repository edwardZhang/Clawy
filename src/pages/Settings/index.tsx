import { desktopApi } from '@/lib/desktop/api';
/**
 * Settings Page
 * Application configuration
 */
import { useCallback, useEffect, useState } from 'react';
import {
  Sun,
  Moon,
  Monitor,
  RefreshCw,
  Loader2,
  Play,
  Square,
  Terminal,
  ExternalLink,
  Key,
  Download,
  Copy,
  FileText,
} from 'lucide-react';
import { Button } from '@/components/ui/button';
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card';
import { ConfirmDialog } from '@/components/ui/confirm-dialog';
import { Label } from '@/components/ui/label';
import { Switch } from '@/components/ui/switch';
import { Separator } from '@/components/ui/separator';
import { Badge } from '@/components/ui/badge';
import { Input } from '@/components/ui/input';
import { Progress } from '@/components/ui/progress';
import { Textarea } from '@/components/ui/textarea';
import { toast } from 'sonner';
import { useSettingsStore } from '@/stores/settings';
import { useGatewayStore } from '@/stores/gateway';
import { useUpdateStore } from '@/stores/update';
import { ProvidersSettings } from '@/components/settings/ProvidersSettings';
import { UpdateSettings } from '@/components/settings/UpdateSettings';
import { useTranslation } from 'react-i18next';
import { SUPPORTED_LANGUAGES } from '@/i18n';
type ControlUiInfo = {
  url: string;
  token: string;
  port: number;
};

type BridgeTokenInfo = {
  nodeId: string;
  token: string;
  tokenSource: 'env' | 'local' | string;
  managedByEnv: boolean;
  configPath: string;
  baseUrl: string;
  apiBaseUrl: string;
  restartRequired: boolean;
};

type BridgeNetworkConfig = {
  lanEnabled: boolean;
  listenHost: string;
  trustedRemoteCidrs: string[];
  allowedOrigins: string[];
  publicBaseUrl: string;
  baseUrl: string;
  apiBaseUrl: string;
  restartRequired: boolean;
};

type RuntimeInstallEventPayload = {
  runtime?: 'nodejs' | 'openclaw';
  phase?: 'preparing' | 'downloading' | 'installing' | 'verifying' | 'completed' | 'failed';
  status?: 'running' | 'completed' | 'failed';
  percent?: number;
  version?: string;
  strategy?: 'official' | 'oss';
  detail?: string;
  error?: string;
  progress?: {
    total?: number;
    transferred?: number;
    bytesPerSecond?: number;
  };
};

type RuntimeStatusPayload = {
  node: {
    path?: string;
    source?: 'path' | 'managed' | 'bundled';
    version?: string;
    diagnostics?: Array<{ detail?: string }>;
  };
  openclaw: {
    dir?: string;
    source?: 'managed' | 'nodeModules' | 'bundled';
    version?: string;
    diagnostics?: Array<{ detail?: string }>;
  };
};

type NodeGlobalStatusPayload = {
  managedNodeAvailable: boolean;
  managedNodeVersion?: string;
  enabled: boolean;
  binDir: string;
  commandPath: string;
  exports: string[];
  scope: string;
  activationMethod: 'userPath' | 'shellProfile' | string;
  persistedPathConfigured: boolean;
  currentProcessPathConfigured: boolean;
  restartRequired: boolean;
  configPath?: string;
};

type RuntimeInstallResponse = {
  success?: boolean;
  error?: string;
  started?: boolean;
  alreadyRunning?: boolean;
  targetVersion?: string;
  strategy?: 'official' | 'oss';
};

type OpenClawUpdateStatusRefreshEventPayload = {
  status?: 'running' | 'completed' | 'failed';
  mode?: 'summary' | 'full';
  result?: OpenClawUpdateStatus;
  error?: string;
};

type OpenClawUpdateStatus = {
  success?: boolean;
  currentVersion?: string | null;
  currentSource?: 'managed' | 'nodeModules' | 'bundled' | null;
  currentDir?: string | null;
  managedVersion?: string | null;
  latestVersion?: string | null;
  officialLatestVersion?: string | null;
  ossLatestVersion?: string | null;
  officialError?: string | null;
  preferredStrategy?: 'official' | 'oss' | null;
  resolvedLatestStrategy?: 'official' | 'oss' | null;
  fallbackAvailable?: boolean;
  usedFallbackForLatestVersion?: boolean;
  recommendedVersion?: string | null;
  updateAvailable?: boolean;
  channel?: {
    value?: string;
    label?: string;
    source?: string;
  } | null;
  availability?: {
    available?: boolean;
    hasRegistryUpdate?: boolean;
    latestVersion?: string;
  } | null;
  dryRun?: {
    actions?: string[];
    effectiveChannel?: string;
    tag?: string;
  } | null;
};

function formatRuntimeSource(source?: OpenClawUpdateStatus['currentSource']): string {
  switch (source) {
    case 'managed':
      return 'Managed runtime';
    case 'nodeModules':
      return 'Workspace package';
    case 'bundled':
      return 'Bundled runtime';
    default:
      return 'Unknown';
  }
}

function formatBytes(bytes?: number): string {
  if (!bytes) return '0 B';
  const units = ['B', 'KB', 'MB', 'GB', 'TB'];
  let value = bytes;
  let unitIndex = 0;
  while (value >= 1024 && unitIndex < units.length - 1) {
    value /= 1024;
    unitIndex += 1;
  }
  return unitIndex === 0 ? `${Math.round(value)} ${units[unitIndex]}` : `${value.toFixed(1)} ${units[unitIndex]}`;
}

function describeInstallProgress(payload: RuntimeInstallEventPayload): string | undefined {
  if (payload.detail) {
    return payload.detail;
  }
  if (payload.progress?.total) {
    return `${formatBytes(payload.progress.transferred)} / ${formatBytes(payload.progress.total)}`;
  }
  if (payload.progress?.transferred) {
    return `${formatBytes(payload.progress.transferred)} downloaded`;
  }
  return undefined;
}

function splitListInput(value: string): string[] {
  return value
    .split(/[\n,;]+/)
    .map((item) => item.trim())
    .filter((item) => item.length > 0);
}

function formatNodeSource(source?: RuntimeStatusPayload['node']['source']): string {
  switch (source) {
    case 'managed':
      return 'Managed runtime';
    case 'path':
      return 'System PATH';
    case 'bundled':
      return 'Bundled runtime';
    default:
      return 'Unknown';
  }
}

export function Settings() {
  const { t } = useTranslation('settings');
  const {
    theme,
    setTheme,
    language,
    setLanguage,
    gatewayAutoStart,
    setGatewayAutoStart,
    proxyEnabled,
    proxyServer,
    proxyHttpServer,
    proxyHttpsServer,
    proxyAllServer,
    proxyBypassRules,
    setProxyEnabled,
    setProxyServer,
    setProxyHttpServer,
    setProxyHttpsServer,
    setProxyAllServer,
    setProxyBypassRules,
    autoCheckUpdate,
    setAutoCheckUpdate,
    autoDownloadUpdate,
    setAutoDownloadUpdate,
    devModeUnlocked,
    setDevModeUnlocked,
  } = useSettingsStore();

  const { status: gatewayStatus, start: startGateway, stop: stopGateway, restart: restartGateway } = useGatewayStore();
  const currentVersion = useUpdateStore((state) => state.currentVersion);
  const updateSetAutoDownload = useUpdateStore((state) => state.setAutoDownload);
  const [controlUiInfo, setControlUiInfo] = useState<ControlUiInfo | null>(null);
  const [controlUiInfoLoading, setControlUiInfoLoading] = useState(false);
  const [bridgeTokenInfo, setBridgeTokenInfo] = useState<BridgeTokenInfo | null>(null);
  const [bridgeTokenLoading, setBridgeTokenLoading] = useState(false);
  const [bridgeTokenError, setBridgeTokenError] = useState<string | null>(null);
  const [bridgeTokenRegenerating, setBridgeTokenRegenerating] = useState(false);
  const [showBridgeTokenConfirm, setShowBridgeTokenConfirm] = useState(false);
  const [bridgeNetworkConfig, setBridgeNetworkConfig] = useState<BridgeNetworkConfig | null>(null);
  const [bridgeNetworkLoading, setBridgeNetworkLoading] = useState(false);
  const [bridgeNetworkSaving, setBridgeNetworkSaving] = useState(false);
  const [bridgeNetworkError, setBridgeNetworkError] = useState<string | null>(null);
  const [showBridgeNetworkConfirm, setShowBridgeNetworkConfirm] = useState(false);
  const [bridgeLanEnabledDraft, setBridgeLanEnabledDraft] = useState(false);
  const [bridgeTrustedCidrsDraft, setBridgeTrustedCidrsDraft] = useState('');
  const [bridgeAllowedOriginsDraft, setBridgeAllowedOriginsDraft] = useState('');
  const [bridgePublicBaseUrlDraft, setBridgePublicBaseUrlDraft] = useState('');
  const [nodeRuntimeStatus, setNodeRuntimeStatus] = useState<RuntimeStatusPayload['node'] | null>(null);
  const [nodeGlobalStatus, setNodeGlobalStatus] = useState<NodeGlobalStatusPayload | null>(null);
  const [nodeGlobalLoading, setNodeGlobalLoading] = useState(false);
  const [nodeGlobalApplying, setNodeGlobalApplying] = useState(false);
  const [nodeGlobalError, setNodeGlobalError] = useState<string | null>(null);
  const [openclawCliCommand, setOpenclawCliCommand] = useState('');
  const [openclawCliError, setOpenclawCliError] = useState<string | null>(null);
  const [openclawCliLoading, setOpenclawCliLoading] = useState(false);
  const [proxyServerDraft, setProxyServerDraft] = useState('');
  const [proxyHttpServerDraft, setProxyHttpServerDraft] = useState('');
  const [proxyHttpsServerDraft, setProxyHttpsServerDraft] = useState('');
  const [proxyAllServerDraft, setProxyAllServerDraft] = useState('');
  const [proxyBypassRulesDraft, setProxyBypassRulesDraft] = useState('');
  const [proxyEnabledDraft, setProxyEnabledDraft] = useState(false);
  const [savingProxy, setSavingProxy] = useState(false);
  const [openclawRuntimeStatus, setOpenclawRuntimeStatus] = useState<OpenClawUpdateStatus | null>(null);
  const [openclawRuntimeLoading, setOpenclawRuntimeLoading] = useState(false);
  const [openclawRuntimeInstalling, setOpenclawRuntimeInstalling] = useState(false);
  const [openclawRuntimeError, setOpenclawRuntimeError] = useState<string | null>(null);
  const [openclawInstallProgress, setOpenclawInstallProgress] = useState<RuntimeInstallEventPayload | null>(null);
  const [openclawRuntimeRefreshInteractive, setOpenclawRuntimeRefreshInteractive] = useState(false);
  const [openclawLastFailedInstallStrategy, setOpenclawLastFailedInstallStrategy] = useState<'official' | 'oss' | null>(null);

  const isWindows = desktopApi.platform === 'win32';
  const showCliTools = true;
  const [showLogs, setShowLogs] = useState(false);
  const [logContent, setLogContent] = useState('');
  const [gatewayAction, setGatewayAction] = useState<'start' | 'stop' | 'restart' | null>(null);

  const handleShowLogs = async () => {
    try {
      const logs = await desktopApi.ipcRenderer.invoke('log:readFile', 100) as string;
      setLogContent(logs);
      setShowLogs(true);
    } catch {
      setLogContent('(Failed to load logs)');
      setShowLogs(true);
    }
  };

  const handleOpenLogDir = async () => {
    try {
      const logDir = await desktopApi.ipcRenderer.invoke('log:getDir') as string;
      if (logDir) {
        await desktopApi.ipcRenderer.invoke('shell:showItemInFolder', logDir);
      }
    } catch {
      // ignore
    }
  };

  const handleToggleGateway = async () => {
    const shouldStop = gatewayStatus.state === 'running'
      || gatewayStatus.state === 'starting'
      || gatewayStatus.state === 'reconnecting';
    const nextAction = shouldStop ? 'stop' : 'start';

    setGatewayAction(nextAction);
    try {
      if (shouldStop) {
        await stopGateway();
      } else {
        await startGateway();
      }
    } finally {
      setGatewayAction(null);
    }
  };

  const handleRestartGateway = async () => {
    setGatewayAction('restart');
    try {
      await restartGateway();
    } finally {
      setGatewayAction(null);
    }
  };

  const handleGatewayAutoStartChange = async (checked: boolean) => {
    setGatewayAutoStart(checked);
    if (!checked) {
      return;
    }

    if (gatewayStatus.state === 'stopped' || gatewayStatus.state === 'error') {
      setGatewayAction('start');
      try {
        await startGateway();
      } finally {
        setGatewayAction(null);
      }
    }
  };

  const loadOpenClawRuntimeStatus = useCallback(async (
    showToast = false,
    mode: 'summary' | 'full' = 'summary',
  ) => {
    setOpenclawRuntimeLoading(true);
    try {
      const result = await desktopApi.ipcRenderer.invoke('openclaw:getUpdateStatus', { mode }) as OpenClawUpdateStatus;
      setOpenclawRuntimeStatus(result);
      setOpenclawRuntimeError(null);
      if (showToast) {
        if (result.updateAvailable && result.latestVersion) {
          toast.success(t('openclawRuntime.toast.updateAvailable', { version: result.latestVersion }));
        } else {
          toast.success(t('openclawRuntime.toast.upToDate'));
        }
      }
    } catch (error) {
      const message = String(error);
      setOpenclawRuntimeError(message);
      if (showToast) {
        toast.error(t('openclawRuntime.toast.checkFailed', { error: message }));
      }
    } finally {
      setOpenclawRuntimeLoading(false);
    }
  }, [t]);

  const handleInstallOpenClawUpdate = async (strategy: 'official' | 'oss' = 'official') => {
    setOpenclawRuntimeInstalling(true);
    setOpenclawRuntimeError(null);
    setOpenclawLastFailedInstallStrategy(null);
    try {
      const payload = openclawRuntimeStatus?.latestVersion
        ? { version: openclawRuntimeStatus.latestVersion, strategy }
        : undefined;
      const result = await desktopApi.ipcRenderer.invoke('openclaw:installUpdate', payload) as RuntimeInstallResponse;
      if (result.success === false) {
        throw new Error(result.error || 'OpenClaw update failed');
      }
      if (result.started === false && result.alreadyRunning) {
        toast.info(t('openclawRuntime.actions.installing'));
      }
    } catch (error) {
      const message = String(error);
      setOpenclawRuntimeError(message);
      setOpenclawLastFailedInstallStrategy(strategy);
      setOpenclawRuntimeInstalling(false);
      setOpenclawInstallProgress(null);
      toast.error(t('openclawRuntime.toast.installFailed', { error: message }));
    }
  };

  const handleRefreshOpenClawRuntimeStatus = async () => {
    setOpenclawRuntimeLoading(true);
    setOpenclawRuntimeError(null);
    setOpenclawRuntimeRefreshInteractive(true);
    try {
      const result = await desktopApi.ipcRenderer.invoke('openclaw:refreshUpdateStatus', { mode: 'full' }) as RuntimeInstallResponse;
      if (result.success === false) {
        throw new Error(result.error || 'OpenClaw update check failed');
      }
      if (result.started === false && result.alreadyRunning) {
        toast.info(t('openclawRuntime.actions.checking'));
      }
    } catch (error) {
      const message = String(error);
      setOpenclawRuntimeError(message);
      setOpenclawRuntimeLoading(false);
      setOpenclawRuntimeRefreshInteractive(false);
      toast.error(t('openclawRuntime.toast.checkFailed', { error: message }));
    }
  };

  const refreshControlUiInfo = useCallback(async (showErrorToast = false) => {
    setControlUiInfoLoading(true);
    try {
      const result = await desktopApi.ipcRenderer.invoke('gateway:getControlUiUrl') as {
        success: boolean;
        url?: string;
        token?: string;
        port?: number;
        error?: string;
      };
      if (result.success && result.url && result.token && typeof result.port === 'number') {
        setControlUiInfo({ url: result.url, token: result.token, port: result.port });
      } else if (showErrorToast && result.error) {
        toast.error(result.error);
      }
    } catch (error) {
      if (showErrorToast) {
        toast.error(String(error));
      }
    } finally {
      setControlUiInfoLoading(false);
    }
  }, []);

  const refreshBridgeTokenInfo = useCallback(async (showErrorToast = false) => {
    setBridgeTokenLoading(true);
    try {
      const result = await desktopApi.ipcRenderer.invoke('bridge:getTokenInfo') as BridgeTokenInfo;
      setBridgeTokenInfo(result);
      setBridgeTokenError(null);
    } catch (error) {
      const message = String(error);
      setBridgeTokenError(message);
      if (showErrorToast) {
        toast.error(t('bridge.toast.loadFailed', { error: message }));
      }
    } finally {
      setBridgeTokenLoading(false);
    }
  }, [t]);

  const refreshBridgeNetworkConfig = useCallback(async (showErrorToast = false) => {
    setBridgeNetworkLoading(true);
    try {
      const result = await desktopApi.ipcRenderer.invoke('bridge:getNetworkConfig') as BridgeNetworkConfig;
      setBridgeNetworkConfig(result);
      setBridgeNetworkError(null);
    } catch (error) {
      const message = String(error);
      setBridgeNetworkError(message);
      if (showErrorToast) {
        toast.error(t('bridge.toast.networkLoadFailed', { error: message }));
      }
    } finally {
      setBridgeNetworkLoading(false);
    }
  }, [t]);

  const refreshNodeGlobalStatus = useCallback(async (showErrorToast = false) => {
    setNodeGlobalLoading(true);
    try {
      const [runtimeStatus, globalStatus] = await Promise.all([
        desktopApi.ipcRenderer.invoke('runtime:status') as Promise<RuntimeStatusPayload>,
        desktopApi.ipcRenderer.invoke('runtime:getNodeGlobalStatus') as Promise<NodeGlobalStatusPayload>,
      ]);
      setNodeRuntimeStatus(runtimeStatus.node);
      setNodeGlobalStatus(globalStatus);
      setNodeGlobalError(null);
    } catch (error) {
      const message = String(error);
      setNodeGlobalError(message);
      if (showErrorToast) {
        toast.error(t('nodeRuntime.toast.loadFailed', { error: message }));
      }
    } finally {
      setNodeGlobalLoading(false);
    }
  }, [t]);

  const loadOpenClawCliCommand = useCallback(async () => {
    if (!showCliTools) return;
    setOpenclawCliLoading(true);
    try {
      const result = await desktopApi.ipcRenderer.invoke('openclaw:getCliCommand') as {
        success: boolean;
        command?: string;
        error?: string;
      };
      if (result.success && result.command) {
        setOpenclawCliCommand(result.command);
        setOpenclawCliError(null);
      } else {
        setOpenclawCliCommand('');
        setOpenclawCliError(result.error || 'OpenClaw CLI unavailable');
      }
    } catch (error) {
      setOpenclawCliCommand('');
      setOpenclawCliError(String(error));
    } finally {
      setOpenclawCliLoading(false);
    }
  }, [showCliTools]);

  // Open developer console
  const openDevConsole = async () => {
    setControlUiInfoLoading(true);
    try {
      const result = await desktopApi.ipcRenderer.invoke('gateway:getControlUiUrl') as {
        success: boolean;
        url?: string;
        token?: string;
        port?: number;
        error?: string;
      };
      if (result.success && result.url && result.token && typeof result.port === 'number') {
        setControlUiInfo({ url: result.url, token: result.token, port: result.port });
        desktopApi.openExternal(result.url);
      } else {
        console.error('Failed to get Dev Console URL:', result.error);
      }
    } catch (err) {
      console.error('Error opening Dev Console:', err);
    } finally {
      setControlUiInfoLoading(false);
    }
  };

  const handleCopyGatewayToken = async () => {
    if (!controlUiInfo?.token) return;
    try {
      await navigator.clipboard.writeText(controlUiInfo.token);
      toast.success(t('developer.tokenCopied'));
    } catch (error) {
      toast.error(`Failed to copy token: ${String(error)}`);
    }
  };

  const handleCopyBridgeToken = async () => {
    if (!bridgeTokenInfo?.token) return;
    try {
      await navigator.clipboard.writeText(bridgeTokenInfo.token);
      toast.success(t('bridge.toast.tokenCopied'));
    } catch (error) {
      toast.error(t('bridge.toast.copyFailed', { error: String(error) }));
    }
  };

  const handleCopyBridgeApiUrl = async () => {
    const apiBaseUrl = bridgeNetworkConfig?.apiBaseUrl || bridgeTokenInfo?.apiBaseUrl;
    if (!apiBaseUrl) return;
    try {
      await navigator.clipboard.writeText(apiBaseUrl);
      toast.success(t('bridge.toast.apiCopied'));
    } catch (error) {
      toast.error(t('bridge.toast.copyFailed', { error: String(error) }));
    }
  };

  const handleConfirmBridgeTokenRegeneration = async () => {
    setBridgeTokenRegenerating(true);
    try {
      const result = await desktopApi.ipcRenderer.invoke('bridge:regenerateToken') as BridgeTokenInfo;
      setBridgeTokenInfo(result);
      setBridgeTokenError(null);
      setShowBridgeTokenConfirm(false);
      toast.success(t('bridge.toast.regenerated'));
      if (result.restartRequired) {
        window.setTimeout(() => {
          void desktopApi.ipcRenderer.invoke('app:relaunch');
        }, 400);
      }
    } catch (error) {
      toast.error(t('bridge.toast.regenerateFailed', { error: String(error) }));
    } finally {
      setBridgeTokenRegenerating(false);
    }
  };

  const handleConfirmBridgeNetworkSave = async () => {
    setBridgeNetworkSaving(true);
    try {
      const result = await desktopApi.ipcRenderer.invoke('bridge:updateNetworkConfig', {
        lanEnabled: bridgeLanEnabledDraft,
        trustedRemoteCidrs: splitListInput(bridgeTrustedCidrsDraft),
        allowedOrigins: splitListInput(bridgeAllowedOriginsDraft),
        publicBaseUrl: bridgePublicBaseUrlDraft.trim(),
      }) as BridgeNetworkConfig;
      setBridgeNetworkConfig(result);
      setBridgeNetworkError(null);
      setShowBridgeNetworkConfirm(false);
      setBridgeTokenInfo((current) => current ? {
        ...current,
        baseUrl: result.baseUrl,
        apiBaseUrl: result.apiBaseUrl,
      } : current);
      toast.success(t('bridge.toast.networkSaved'));
      if (result.restartRequired) {
        window.setTimeout(() => {
          void desktopApi.ipcRenderer.invoke('app:relaunch');
        }, 400);
      }
    } catch (error) {
      toast.error(t('bridge.toast.networkSaveFailed', { error: String(error) }));
    } finally {
      setBridgeNetworkSaving(false);
    }
  };

  const handleEnableNodeGlobal = async () => {
    setNodeGlobalApplying(true);
    try {
      const result = await desktopApi.ipcRenderer.invoke('runtime:makeManagedNodeGlobal') as NodeGlobalStatusPayload;
      setNodeGlobalStatus(result);
      setNodeGlobalError(null);
      await refreshNodeGlobalStatus(false);
      toast.success(
        result.restartRequired
          ? t('nodeRuntime.toast.enabledRestart', { binDir: result.binDir })
          : t('nodeRuntime.toast.enabled', { binDir: result.binDir }),
      );
    } catch (error) {
      const message = String(error);
      setNodeGlobalError(message);
      toast.error(t('nodeRuntime.toast.enableFailed', { error: message }));
    } finally {
      setNodeGlobalApplying(false);
    }
  };

  useEffect(() => {
    if (!devModeUnlocked) {
      return;
    }

    let cancelled = false;
    const timeoutId = window.setTimeout(() => {
      window.requestAnimationFrame(() => {
        if (cancelled) return;
        void refreshControlUiInfo();
        void loadOpenClawCliCommand();
      });
    }, 80);

    return () => {
      cancelled = true;
      window.clearTimeout(timeoutId);
    };
  }, [devModeUnlocked, loadOpenClawCliCommand, refreshControlUiInfo]);

  useEffect(() => {
    let cancelled = false;
    const timeoutId = window.setTimeout(() => {
      window.requestAnimationFrame(() => {
        if (!cancelled) {
          void refreshNodeGlobalStatus();
          void refreshBridgeTokenInfo();
          void refreshBridgeNetworkConfig();
        }
      });
    }, 80);

    return () => {
      cancelled = true;
      window.clearTimeout(timeoutId);
    };
  }, [refreshBridgeNetworkConfig, refreshBridgeTokenInfo, refreshNodeGlobalStatus]);

  useEffect(() => {
    setBridgeLanEnabledDraft(bridgeNetworkConfig?.lanEnabled ?? false);
    setBridgeTrustedCidrsDraft((bridgeNetworkConfig?.trustedRemoteCidrs ?? []).join('\n'));
    setBridgeAllowedOriginsDraft((bridgeNetworkConfig?.allowedOrigins ?? []).join('\n'));
    setBridgePublicBaseUrlDraft(bridgeNetworkConfig?.publicBaseUrl ?? '');
  }, [bridgeNetworkConfig]);

  const handleCopyCliCommand = async () => {
    if (!openclawCliCommand) return;
    try {
      await navigator.clipboard.writeText(openclawCliCommand);
      toast.success(t('developer.cmdCopied'));
    } catch (error) {
      toast.error(`Failed to copy command: ${String(error)}`);
    }
  };

  useEffect(() => {
    const unsubscribe = desktopApi.ipcRenderer.on(
      'openclaw:cli-installed',
      (...args: unknown[]) => {
        const installedPath = typeof args[0] === 'string' ? args[0] : '';
        toast.success(`openclaw CLI installed at ${installedPath}`);
      },
    );
    return () => { unsubscribe?.(); };
  }, [loadOpenClawRuntimeStatus]);

  useEffect(() => {
    let cancelled = false;
    const timeoutId = window.setTimeout(() => {
      window.requestAnimationFrame(() => {
        if (!cancelled) {
          void desktopApi.ipcRenderer.invoke('openclaw:refreshUpdateStatus', { mode: 'summary' }).catch(() => {
            void loadOpenClawRuntimeStatus(false, 'summary');
          });
        }
      });
    }, 600);

    return () => {
      cancelled = true;
      window.clearTimeout(timeoutId);
    };
  }, [loadOpenClawRuntimeStatus]);

  useEffect(() => {
    const unsubscribe = desktopApi.ipcRenderer.on('runtime:install-progress', (payload) => {
      const event = payload as RuntimeInstallEventPayload;
      if (event.runtime !== 'openclaw') {
        return;
      }

      setOpenclawInstallProgress(event);

      if (event.status === 'failed') {
        setOpenclawRuntimeError(event.error || event.detail || null);
        setOpenclawLastFailedInstallStrategy(event.strategy || 'official');
        setOpenclawRuntimeInstalling(false);
        setOpenclawInstallProgress(null);
      }

      if (event.status === 'completed') {
        const version = event.version || openclawRuntimeStatus?.latestVersion || '';
        toast.success(t('openclawRuntime.toast.updated', { version }));
        setOpenclawLastFailedInstallStrategy(null);
        setOpenclawRuntimeInstalling(false);
        void loadOpenClawRuntimeStatus(false, 'summary').finally(() => {
          setOpenclawInstallProgress(null);
        });
      }
    });

    return () => { unsubscribe?.(); };
  }, [loadOpenClawRuntimeStatus, openclawRuntimeStatus?.latestVersion, t]);

  useEffect(() => {
    const unsubscribe = desktopApi.ipcRenderer.on('openclaw:update-status-changed', (payload) => {
      const event = payload as OpenClawUpdateStatusRefreshEventPayload;
      if (event.status === 'running') {
        setOpenclawRuntimeLoading(true);
        return;
      }

      if (event.status === 'completed' && event.result) {
        setOpenclawRuntimeStatus(event.result);
        setOpenclawRuntimeError(null);
        setOpenclawRuntimeLoading(false);
        if (openclawRuntimeRefreshInteractive) {
          if (event.result.updateAvailable && event.result.latestVersion) {
            toast.success(t('openclawRuntime.toast.updateAvailable', { version: event.result.latestVersion }));
          } else {
            toast.success(t('openclawRuntime.toast.upToDate'));
          }
          setOpenclawRuntimeRefreshInteractive(false);
        }
        return;
      }

      if (event.status === 'failed') {
        const message = event.error || 'OpenClaw update check failed';
        setOpenclawRuntimeError(message);
        setOpenclawRuntimeLoading(false);
        if (openclawRuntimeRefreshInteractive) {
          toast.error(t('openclawRuntime.toast.checkFailed', { error: message }));
          setOpenclawRuntimeRefreshInteractive(false);
        }
      }
    });

    return () => { unsubscribe?.(); };
  }, [openclawRuntimeRefreshInteractive, t]);

  const canResolveOpenClawWithOss = Boolean(
    openclawRuntimeStatus?.fallbackAvailable
      && openclawRuntimeStatus?.latestVersion
      && (
        openclawLastFailedInstallStrategy === 'official'
        || (!!openclawRuntimeError && !!openclawRuntimeStatus?.officialError)
      ),
  );

  const bridgeNetworkDirty = bridgeNetworkConfig !== null && (
    bridgeLanEnabledDraft !== bridgeNetworkConfig.lanEnabled
      || bridgePublicBaseUrlDraft.trim() !== bridgeNetworkConfig.publicBaseUrl
      || JSON.stringify(splitListInput(bridgeTrustedCidrsDraft)) !== JSON.stringify(bridgeNetworkConfig.trustedRemoteCidrs)
      || JSON.stringify(splitListInput(bridgeAllowedOriginsDraft)) !== JSON.stringify(bridgeNetworkConfig.allowedOrigins)
  );
  const nodeRuntimeAvailable = Boolean(nodeRuntimeStatus?.path);
  const nodeGlobalEnabled = Boolean(nodeGlobalStatus?.enabled);
  const nodeGlobalCommands = (nodeGlobalStatus?.exports ?? []).join(', ');

  useEffect(() => {
    setProxyEnabledDraft(proxyEnabled);
  }, [proxyEnabled]);

  useEffect(() => {
    setProxyServerDraft(proxyServer);
  }, [proxyServer]);

  useEffect(() => {
    setProxyHttpServerDraft(proxyHttpServer);
  }, [proxyHttpServer]);

  useEffect(() => {
    setProxyHttpsServerDraft(proxyHttpsServer);
  }, [proxyHttpsServer]);

  useEffect(() => {
    setProxyAllServerDraft(proxyAllServer);
  }, [proxyAllServer]);

  useEffect(() => {
    setProxyBypassRulesDraft(proxyBypassRules);
  }, [proxyBypassRules]);

  const handleSaveProxySettings = async () => {
    setSavingProxy(true);
    try {
      const normalizedProxyServer = proxyServerDraft.trim();
      const normalizedHttpServer = proxyHttpServerDraft.trim();
      const normalizedHttpsServer = proxyHttpsServerDraft.trim();
      const normalizedAllServer = proxyAllServerDraft.trim();
      const normalizedBypassRules = proxyBypassRulesDraft.trim();
      await desktopApi.ipcRenderer.invoke('settings:setMany', {
        proxyEnabled: proxyEnabledDraft,
        proxyServer: normalizedProxyServer,
        proxyHttpServer: normalizedHttpServer,
        proxyHttpsServer: normalizedHttpsServer,
        proxyAllServer: normalizedAllServer,
        proxyBypassRules: normalizedBypassRules,
      });

      setProxyServer(normalizedProxyServer);
      setProxyHttpServer(normalizedHttpServer);
      setProxyHttpsServer(normalizedHttpsServer);
      setProxyAllServer(normalizedAllServer);
      setProxyBypassRules(normalizedBypassRules);
      setProxyEnabled(proxyEnabledDraft);

      toast.success(t('gateway.proxySaved'));
    } catch (error) {
      toast.error(`${t('gateway.proxySaveFailed')}: ${String(error)}`);
    } finally {
      setSavingProxy(false);
    }
  };

  return (
    <div className="space-y-6 p-6">
      <div>
        <h1 className="text-2xl font-bold">{t('title')}</h1>
        <p className="text-muted-foreground">
          {t('subtitle')}
        </p>
      </div>

      {/* Appearance */}
      <Card>
        <CardHeader>
          <CardTitle>{t('appearance.title')}</CardTitle>
          <CardDescription>{t('appearance.description')}</CardDescription>
        </CardHeader>
        <CardContent className="space-y-4">
          <div className="space-y-2">
            <Label>{t('appearance.theme')}</Label>
            <div className="flex gap-2">
              <Button
                variant={theme === 'light' ? 'default' : 'outline'}
                size="sm"
                onClick={() => setTheme('light')}
              >
                <Sun className="h-4 w-4 mr-2" />
                {t('appearance.light')}
              </Button>
              <Button
                variant={theme === 'dark' ? 'default' : 'outline'}
                size="sm"
                onClick={() => setTheme('dark')}
              >
                <Moon className="h-4 w-4 mr-2" />
                {t('appearance.dark')}
              </Button>
              <Button
                variant={theme === 'system' ? 'default' : 'outline'}
                size="sm"
                onClick={() => setTheme('system')}
              >
                <Monitor className="h-4 w-4 mr-2" />
                {t('appearance.system')}
              </Button>
            </div>
          </div>
          <div className="space-y-2">
            <Label>{t('appearance.language')}</Label>
            <div className="flex gap-2">
              {SUPPORTED_LANGUAGES.map((lang) => (
                <Button
                  key={lang.code}
                  variant={language === lang.code ? 'default' : 'outline'}
                  size="sm"
                  onClick={() => setLanguage(lang.code)}
                >
                  {lang.label}
                </Button>
              ))}
            </div>
          </div>
        </CardContent>
      </Card>

      {/* AI Providers */}
      <Card>
        <CardHeader>
          <CardTitle className="flex items-center gap-2">
            <Key className="h-5 w-5" />
            {t('aiProviders.title')}
          </CardTitle>
          <CardDescription>{t('aiProviders.description')}</CardDescription>
        </CardHeader>
        <CardContent>
          <ProvidersSettings />
        </CardContent>
      </Card>

      {/* Gateway */}
      <Card>
        <CardHeader>
          <CardTitle>{t('gateway.title')}</CardTitle>
          <CardDescription>{t('gateway.description')}</CardDescription>
        </CardHeader>
        <CardContent className="space-y-4">
          <div className="flex items-center justify-between">
            <div>
              <Label>{t('gateway.status')}</Label>
              <p className="text-sm text-muted-foreground">
                {t('gateway.port')}: {gatewayStatus.port}
              </p>
            </div>
            <div className="flex items-center gap-2">
              <Badge
                variant={
                  gatewayStatus.state === 'running'
                    ? 'success'
                    : gatewayStatus.state === 'error'
                      ? 'destructive'
                      : 'secondary'
                }
              >
                {gatewayStatus.state}
              </Badge>
              <Button
                variant="outline"
                size="sm"
                onClick={handleToggleGateway}
                disabled={gatewayAction !== null}
              >
                {gatewayAction === 'start' || gatewayAction === 'stop' ? (
                  <Loader2 className="h-4 w-4 mr-2 animate-spin" />
                ) : gatewayStatus.state === 'running' || gatewayStatus.state === 'starting' || gatewayStatus.state === 'reconnecting' ? (
                  <Square className="h-4 w-4 mr-2" />
                ) : (
                  <Play className="h-4 w-4 mr-2" />
                )}
                {gatewayStatus.state === 'running' || gatewayStatus.state === 'starting' || gatewayStatus.state === 'reconnecting'
                  ? t('common:actions.stop')
                  : t('common:actions.start')}
              </Button>
              <Button
                variant="outline"
                size="sm"
                onClick={handleRestartGateway}
                disabled={gatewayAction !== null}
              >
                <RefreshCw className={`h-4 w-4 mr-2${gatewayAction === 'restart' ? ' animate-spin' : ''}`} />
                {t('common:actions.restart')}
              </Button>
              <Button variant="outline" size="sm" onClick={handleShowLogs}>
                <FileText className="h-4 w-4 mr-2" />
                {t('gateway.logs')}
              </Button>
            </div>
          </div>

          {showLogs && (
            <div className="mt-4 p-4 rounded-lg bg-black/10 dark:bg-black/40 border border-border">
              <div className="flex items-center justify-between mb-2">
                <p className="font-medium text-sm">{t('gateway.appLogs')}</p>
                <div className="flex gap-2">
                  <Button variant="ghost" size="sm" className="h-7 text-xs" onClick={handleOpenLogDir}>
                    <ExternalLink className="h-3 w-3 mr-1" />
                    {t('gateway.openFolder')}
                  </Button>
                  <Button variant="ghost" size="sm" className="h-7 text-xs" onClick={() => setShowLogs(false)}>
                    {t('common:actions.close')}
                  </Button>
                </div>
              </div>
              <pre className="text-xs text-muted-foreground bg-background/50 p-3 rounded max-h-60 overflow-auto whitespace-pre-wrap font-mono">
                {logContent || t('chat:noLogs')}
              </pre>
            </div>
          )}

          <Separator />

          <div className="flex items-center justify-between">
            <div>
              <Label>{t('gateway.autoStart')}</Label>
              <p className="text-sm text-muted-foreground">
                {t('gateway.autoStartDesc')}
              </p>
            </div>
            <Switch
              checked={gatewayAutoStart}
              onCheckedChange={(checked) => {
                void handleGatewayAutoStartChange(checked);
              }}
            />
          </div>

          <Separator />

          <div className="space-y-4">
            <div className="flex items-center justify-between">
              <div>
                <Label>{t('gateway.proxyTitle')}</Label>
                <p className="text-sm text-muted-foreground">
                  {t('gateway.proxyDesc')}
                </p>
              </div>
              <Switch
                checked={proxyEnabledDraft}
                onCheckedChange={setProxyEnabledDraft}
              />
            </div>

            <div className="space-y-2">
              <Label htmlFor="proxy-server">{t('gateway.proxyServer')}</Label>
              <Input
                id="proxy-server"
                value={proxyServerDraft}
                onChange={(event) => setProxyServerDraft(event.target.value)}
                placeholder="http://127.0.0.1:7890"
              />
              <p className="text-xs text-muted-foreground">
                {t('gateway.proxyServerHelp')}
              </p>
            </div>

            {devModeUnlocked && (
              <>
                <div className="space-y-2">
                  <Label htmlFor="proxy-http-server">{t('gateway.proxyHttpServer')}</Label>
                  <Input
                    id="proxy-http-server"
                    value={proxyHttpServerDraft}
                    onChange={(event) => setProxyHttpServerDraft(event.target.value)}
                    placeholder={proxyServerDraft || 'http://127.0.0.1:7890'}
                  />
                  <p className="text-xs text-muted-foreground">
                    {t('gateway.proxyHttpServerHelp')}
                  </p>
                </div>

                <div className="space-y-2">
                  <Label htmlFor="proxy-https-server">{t('gateway.proxyHttpsServer')}</Label>
                  <Input
                    id="proxy-https-server"
                    value={proxyHttpsServerDraft}
                    onChange={(event) => setProxyHttpsServerDraft(event.target.value)}
                    placeholder={proxyServerDraft || 'http://127.0.0.1:7890'}
                  />
                  <p className="text-xs text-muted-foreground">
                    {t('gateway.proxyHttpsServerHelp')}
                  </p>
                </div>

                <div className="space-y-2">
                  <Label htmlFor="proxy-all-server">{t('gateway.proxyAllServer')}</Label>
                  <Input
                    id="proxy-all-server"
                    value={proxyAllServerDraft}
                    onChange={(event) => setProxyAllServerDraft(event.target.value)}
                    placeholder={proxyServerDraft || 'socks5://127.0.0.1:7891'}
                  />
                  <p className="text-xs text-muted-foreground">
                    {t('gateway.proxyAllServerHelp')}
                  </p>
                </div>
              </>
            )}

            <div className="space-y-2">
              <Label htmlFor="proxy-bypass">{t('gateway.proxyBypass')}</Label>
              <Input
                id="proxy-bypass"
                value={proxyBypassRulesDraft}
                onChange={(event) => setProxyBypassRulesDraft(event.target.value)}
                placeholder="<local>;localhost;127.0.0.1;::1"
              />
              <p className="text-xs text-muted-foreground">
                {t('gateway.proxyBypassHelp')}
              </p>
            </div>

            <div className="flex items-center justify-between gap-3 rounded-lg border border-border/60 bg-background/40 p-3">
              <p className="text-sm text-muted-foreground">
                {t('gateway.proxyRestartNote')}
              </p>
              <Button
                variant="outline"
                onClick={handleSaveProxySettings}
                disabled={savingProxy}
              >
                <RefreshCw className={`h-4 w-4 mr-2${savingProxy ? ' animate-spin' : ''}`} />
                {savingProxy ? t('common:status.saving') : t('common:actions.save')}
              </Button>
            </div>
          </div>
        </CardContent>
      </Card>

      {/* Updates */}
      <Card>
        <CardHeader>
          <CardTitle className="flex items-center gap-2">
            <Download className="h-5 w-5" />
            {t('updates.title')}
          </CardTitle>
          <CardDescription>{t('updates.description')}</CardDescription>
        </CardHeader>
        <CardContent className="space-y-4">
          <UpdateSettings />

          <Separator />

          <div className="flex items-center justify-between">
            <div>
              <Label>{t('updates.autoCheck')}</Label>
              <p className="text-sm text-muted-foreground">
                {t('updates.autoCheckDesc')}
              </p>
            </div>
            <Switch
              checked={autoCheckUpdate}
              onCheckedChange={setAutoCheckUpdate}
            />
          </div>

          <div className="flex items-center justify-between">
            <div>
              <Label>{t('updates.autoDownload')}</Label>
              <p className="text-sm text-muted-foreground">
                {t('updates.autoDownloadDesc')}
              </p>
            </div>
            <Switch
              checked={autoDownloadUpdate}
              onCheckedChange={(value) => {
                setAutoDownloadUpdate(value);
                updateSetAutoDownload(value);
              }}
            />
          </div>
        </CardContent>
      </Card>

      <Card>
        <CardHeader>
          <CardTitle className="flex items-center gap-2">
            <Terminal className="h-5 w-5" />
            {t('nodeRuntime.title')}
          </CardTitle>
          <CardDescription>{t('nodeRuntime.description')}</CardDescription>
        </CardHeader>
        <CardContent className="space-y-4">
          <div className="grid gap-4 md:grid-cols-2">
            <div className="space-y-1">
              <Label>{t('nodeRuntime.currentVersion')}</Label>
              <p className="text-sm font-medium">
                {nodeRuntimeStatus?.version || t('nodeRuntime.notInstalled')}
              </p>
              <p className="text-xs text-muted-foreground">
                {t('nodeRuntime.source')}: {formatNodeSource(nodeRuntimeStatus?.source)}
              </p>
            </div>

            <div className="space-y-1">
              <Label>{t('nodeRuntime.globalStatus')}</Label>
              <div className="flex items-center gap-2">
                <Badge variant={nodeGlobalEnabled ? 'success' : 'secondary'}>
                  {nodeGlobalEnabled ? t('nodeRuntime.status.global') : t('nodeRuntime.status.localOnly')}
                </Badge>
                <p className="text-xs text-muted-foreground">
                  {isWindows ? t('nodeRuntime.methods.userPath') : t('nodeRuntime.methods.shellProfile')}
                </p>
              </div>
            </div>
          </div>

          <div className="space-y-2">
            <Label>{t('nodeRuntime.runtimePath')}</Label>
            <Input
              readOnly
              value={nodeRuntimeStatus?.path || ''}
              placeholder={t('nodeRuntime.notInstalled')}
              className="font-mono"
            />
          </div>

          <div className="space-y-2">
            <Label>{t('nodeRuntime.binDir')}</Label>
            <Input
              readOnly
              value={nodeGlobalStatus?.binDir || ''}
              placeholder={t('nodeRuntime.unavailable')}
              className="font-mono"
            />
            <p className="text-xs text-muted-foreground">
              {t('nodeRuntime.binDirDesc', { commands: nodeGlobalCommands || 'node, npm, npx, corepack' })}
            </p>
          </div>

          <div className="space-y-2">
            <Label>{t('nodeRuntime.activation')}</Label>
            <p className="text-sm text-muted-foreground">
              {nodeGlobalEnabled
                ? nodeGlobalStatus?.restartRequired
                  ? t('nodeRuntime.activationReadyRestart')
                  : t('nodeRuntime.activationReady')
                : t('nodeRuntime.activationPending')}
            </p>
            {nodeGlobalStatus?.configPath ? (
              <p className="text-xs text-muted-foreground break-all">
                {t('nodeRuntime.configPath')}: {nodeGlobalStatus.configPath}
              </p>
            ) : null}
          </div>

          <div className="flex flex-col gap-2 md:flex-row">
            <Button
              type="button"
              variant="outline"
              onClick={() => void refreshNodeGlobalStatus(true)}
              disabled={nodeGlobalLoading || nodeGlobalApplying}
            >
              <RefreshCw className={`h-4 w-4 mr-2${nodeGlobalLoading ? ' animate-spin' : ''}`} />
              {t('common:actions.refresh')}
            </Button>
            <Button
              type="button"
              onClick={() => void handleEnableNodeGlobal()}
              disabled={!nodeRuntimeAvailable || nodeGlobalLoading || nodeGlobalApplying}
            >
              <Terminal className="h-4 w-4 mr-2" />
              {nodeGlobalApplying
                ? t('nodeRuntime.actions.enabling')
                : nodeGlobalEnabled
                  ? t('nodeRuntime.actions.reapply')
                  : t('nodeRuntime.actions.enable')}
            </Button>
          </div>

          <p className="text-xs text-muted-foreground">
            {isWindows ? t('nodeRuntime.windowsNote') : t('nodeRuntime.posixNote')}
          </p>

          {nodeGlobalError ? (
            <div className="rounded-lg border border-red-500/30 bg-red-500/5 p-3 text-sm text-red-500">
              {nodeGlobalError}
            </div>
          ) : null}
        </CardContent>
      </Card>

      <Card>
        <CardHeader>
          <CardTitle>{t('openclawRuntime.title')}</CardTitle>
          <CardDescription>{t('openclawRuntime.description')}</CardDescription>
        </CardHeader>
        <CardContent className="space-y-4">
          <div className="grid gap-4 md:grid-cols-2">
            <div className="space-y-1">
              <Label>{t('openclawRuntime.currentVersion')}</Label>
              <p className="text-sm font-medium">
                {openclawRuntimeStatus?.currentVersion || t('openclawRuntime.notInstalled')}
              </p>
              <p className="text-xs text-muted-foreground">
                {t('openclawRuntime.source')}: {formatRuntimeSource(openclawRuntimeStatus?.currentSource ?? undefined)}
              </p>
            </div>

            <div className="space-y-1">
              <Label>{t('openclawRuntime.latestVersion')}</Label>
              <div className="flex items-center gap-2">
                <p className="text-sm font-medium">
                  {openclawRuntimeStatus?.latestVersion || t('openclawRuntime.unknown')}
                </p>
                <Badge
                  variant={
                    openclawRuntimeStatus?.updateAvailable
                      ? 'secondary'
                      : 'success'
                  }
                >
                  {openclawRuntimeStatus?.updateAvailable
                    ? t('openclawRuntime.status.updateAvailable')
                    : t('openclawRuntime.status.upToDate')}
                </Badge>
              </div>
              <p className="text-xs text-muted-foreground">
                {t('openclawRuntime.channel')}: {openclawRuntimeStatus?.channel?.label || openclawRuntimeStatus?.channel?.value || t('openclawRuntime.unknown')}
              </p>
            </div>
          </div>

          <div className="grid gap-4 md:grid-cols-2">
            <div className="space-y-1">
              <Label>{t('openclawRuntime.managedVersion')}</Label>
              <p className="text-sm text-muted-foreground">
                {openclawRuntimeStatus?.managedVersion || t('openclawRuntime.notInstalled')}
              </p>
            </div>
            <div className="space-y-1">
              <Label>{t('openclawRuntime.runtimePath')}</Label>
              <p className="text-xs text-muted-foreground break-all">
                {openclawRuntimeStatus?.currentDir || t('openclawRuntime.unknown')}
              </p>
            </div>
          </div>

          {openclawRuntimeStatus?.dryRun?.actions?.length ? (
            <div className="space-y-2 rounded-lg border border-border/60 bg-background/40 p-3">
              <Label>{t('openclawRuntime.plan')}</Label>
              <ul className="space-y-1 text-sm text-muted-foreground">
                {openclawRuntimeStatus.dryRun.actions.map((action) => (
                  <li key={action}>• {action}</li>
                ))}
              </ul>
            </div>
          ) : null}

          {openclawRuntimeStatus?.officialError && openclawRuntimeStatus?.usedFallbackForLatestVersion ? (
            <div className="rounded-lg border border-amber-500/30 bg-amber-500/5 p-3 text-sm text-amber-700 dark:text-amber-300">
              <p className="font-medium">{t('openclawRuntime.fallback.title')}</p>
              <p className="mt-1 text-xs leading-5">{t('openclawRuntime.fallback.description')}</p>
              <p className="mt-2 text-xs break-words opacity-80">{openclawRuntimeStatus.officialError}</p>
            </div>
          ) : null}

          {openclawInstallProgress ? (
            <div className="space-y-2 rounded-lg border border-border/60 bg-background/40 p-3">
              <div className="flex items-center justify-between text-sm">
                <span>{t(`openclawRuntime.phases.${openclawInstallProgress.phase || 'installing'}`)}</span>
                <span>{Math.round(openclawInstallProgress.percent || 0)}%</span>
              </div>
              <Progress value={openclawInstallProgress.percent || 0} className="h-2" />
              {describeInstallProgress(openclawInstallProgress) ? (
                <p className="text-xs text-muted-foreground">
                  {describeInstallProgress(openclawInstallProgress)}
                </p>
              ) : null}
            </div>
          ) : null}

          {openclawRuntimeError ? (
            <div className="rounded-lg border border-red-500/30 bg-red-500/5 p-3 text-sm text-red-500">
              {openclawRuntimeError}
            </div>
          ) : null}

          <div className="flex items-center justify-between gap-3 rounded-lg border border-border/60 bg-background/40 p-3">
            <div>
              <p className="text-sm text-muted-foreground">{t('openclawRuntime.restartNote')}</p>
              <p className="text-xs text-muted-foreground">{t('openclawRuntime.installNote')}</p>
            </div>
            <div className="flex gap-2">
              <Button
                variant="outline"
                onClick={() => void handleRefreshOpenClawRuntimeStatus()}
                disabled={openclawRuntimeLoading || openclawRuntimeInstalling}
              >
                <RefreshCw className={`h-4 w-4 mr-2${openclawRuntimeLoading ? ' animate-spin' : ''}`} />
                {openclawRuntimeLoading ? t('openclawRuntime.actions.checking') : t('openclawRuntime.actions.check')}
              </Button>
              <Button
                onClick={() => void handleInstallOpenClawUpdate()}
                disabled={
                  openclawRuntimeInstalling ||
                  openclawRuntimeLoading ||
                  !openclawRuntimeStatus?.latestVersion ||
                  Boolean(openclawRuntimeStatus?.currentVersion) && !openclawRuntimeStatus?.updateAvailable
                }
              >
                <Download className={`h-4 w-4 mr-2${openclawRuntimeInstalling ? ' animate-bounce' : ''}`} />
                {openclawRuntimeInstalling ? t('openclawRuntime.actions.installing') : t('openclawRuntime.actions.install')}
              </Button>
              {canResolveOpenClawWithOss ? (
                <Button
                  variant="outline"
                  onClick={() => void handleInstallOpenClawUpdate('oss')}
                  disabled={openclawRuntimeInstalling || openclawRuntimeLoading}
                >
                  <Download className="h-4 w-4 mr-2" />
                  {t('openclawRuntime.actions.resolve')}
                </Button>
              ) : null}
            </div>
          </div>
        </CardContent>
      </Card>

      <Card>
        <CardHeader>
          <CardTitle className="flex items-center gap-2">
            <Key className="h-5 w-5" />
            {t('bridge.title')}
          </CardTitle>
          <CardDescription>{t('bridge.description')}</CardDescription>
        </CardHeader>
        <CardContent className="space-y-4">
          <div className="grid gap-4 md:grid-cols-2">
            <div className="space-y-1">
              <Label>{t('bridge.nodeId')}</Label>
              <p className="text-sm font-medium break-all">
                {bridgeTokenInfo?.nodeId || t('bridge.unavailable')}
              </p>
            </div>
            <div className="space-y-1">
              <Label>{t('bridge.tokenSource')}</Label>
              {bridgeTokenInfo ? (
                <div className="flex items-center gap-2">
                  <Badge variant={bridgeTokenInfo.managedByEnv ? 'secondary' : 'success'}>
                    {bridgeTokenInfo.managedByEnv ? t('bridge.sources.env') : t('bridge.sources.local')}
                  </Badge>
                  <p className="text-xs text-muted-foreground">
                    {bridgeTokenInfo.managedByEnv ? t('bridge.envManaged') : t('bridge.localManaged')}
                  </p>
                </div>
              ) : (
                <p className="text-sm text-muted-foreground">{t('bridge.unavailable')}</p>
              )}
            </div>
          </div>

          <div className="grid gap-4 md:grid-cols-2">
            <div className="space-y-2">
              <Label>{t('bridge.accessMode')}</Label>
              <div className="flex items-center justify-between rounded-lg border border-border/60 bg-background/40 p-3">
                <div>
                  <p className="text-sm font-medium">
                    {bridgeLanEnabledDraft ? t('bridge.modes.lan') : t('bridge.modes.local')}
                  </p>
                  <p className="text-xs text-muted-foreground">
                    {bridgeLanEnabledDraft ? t('bridge.lanDesc') : t('bridge.localDesc')}
                  </p>
                </div>
                <Switch
                  checked={bridgeLanEnabledDraft}
                  onCheckedChange={setBridgeLanEnabledDraft}
                  disabled={bridgeNetworkLoading || bridgeNetworkSaving}
                />
              </div>
            </div>

            <div className="space-y-2">
              <Label>{t('bridge.listenHost')}</Label>
              <Input
                readOnly
                value={bridgeNetworkConfig?.listenHost || ''}
                placeholder={t('bridge.unavailable')}
                className="font-mono"
              />
              <p className="text-xs text-muted-foreground">
                {t('bridge.listenHostDesc')}
              </p>
            </div>
          </div>

          <div className="space-y-2">
            <Label>{t('bridge.publicBaseUrl')}</Label>
            <Input
              value={bridgePublicBaseUrlDraft}
              onChange={(event) => setBridgePublicBaseUrlDraft(event.target.value)}
              placeholder="http://192.168.1.10:18790"
              className="font-mono"
              disabled={bridgeNetworkSaving}
            />
            <p className="text-xs text-muted-foreground">
              {t('bridge.publicBaseUrlDesc')}
            </p>
          </div>

          <div className="space-y-2">
            <Label>{t('bridge.trustedCidrs')}</Label>
            <Textarea
              value={bridgeTrustedCidrsDraft}
              onChange={(event) => setBridgeTrustedCidrsDraft(event.target.value)}
              placeholder={t('bridge.trustedCidrsPlaceholder')}
              disabled={bridgeNetworkSaving}
              className="font-mono"
            />
            <p className="text-xs text-muted-foreground">
              {t('bridge.trustedCidrsDesc')}
            </p>
          </div>

          <div className="space-y-2">
            <Label>{t('bridge.allowedOrigins')}</Label>
            <Textarea
              value={bridgeAllowedOriginsDraft}
              onChange={(event) => setBridgeAllowedOriginsDraft(event.target.value)}
              placeholder={t('bridge.allowedOriginsPlaceholder')}
              disabled={bridgeNetworkSaving}
              className="font-mono"
            />
            <p className="text-xs text-muted-foreground">
              {t('bridge.allowedOriginsDesc')}
            </p>
          </div>

          <div className="space-y-2">
            <Label>{t('bridge.apiBaseUrl')}</Label>
            <div className="flex flex-col gap-2 md:flex-row">
              <Input
                readOnly
                value={bridgeNetworkConfig?.apiBaseUrl || bridgeTokenInfo?.apiBaseUrl || ''}
                placeholder={t('bridge.unavailable')}
                className="font-mono"
              />
              <Button
                type="button"
                variant="outline"
                onClick={() => {
                  void refreshBridgeTokenInfo(true);
                  void refreshBridgeNetworkConfig(true);
                }}
                disabled={bridgeTokenLoading || bridgeTokenRegenerating || bridgeNetworkLoading || bridgeNetworkSaving}
              >
                <RefreshCw className={`h-4 w-4 mr-2${bridgeTokenLoading || bridgeNetworkLoading ? ' animate-spin' : ''}`} />
                {t('common:actions.refresh')}
              </Button>
              <Button
                type="button"
                variant="outline"
                onClick={handleCopyBridgeApiUrl}
                disabled={!bridgeNetworkConfig?.apiBaseUrl && !bridgeTokenInfo?.apiBaseUrl}
              >
                <Copy className="h-4 w-4 mr-2" />
                {t('common:actions.copy')}
              </Button>
            </div>
            <p className="text-xs text-muted-foreground break-all">
              {bridgeNetworkConfig?.baseUrl || bridgeTokenInfo?.baseUrl || bridgeTokenInfo?.configPath || t('bridge.unavailable')}
            </p>
          </div>

          <div className="space-y-2">
            <Label>{t('bridge.token')}</Label>
            <p className="text-sm text-muted-foreground">
              {t('bridge.tokenDesc')}
            </p>
            <div className="flex flex-col gap-2 md:flex-row">
              <Input
                readOnly
                value={bridgeTokenInfo?.token || ''}
                placeholder={t('bridge.unavailable')}
                className="font-mono"
              />
              <Button
                type="button"
                variant="outline"
                onClick={handleCopyBridgeToken}
                disabled={!bridgeTokenInfo?.token}
              >
                <Copy className="h-4 w-4 mr-2" />
                {t('common:actions.copy')}
              </Button>
              <Button
                type="button"
                variant="outline"
                onClick={() => setShowBridgeTokenConfirm(true)}
                disabled={bridgeTokenLoading || bridgeTokenRegenerating || !bridgeTokenInfo || bridgeTokenInfo.managedByEnv}
              >
                <RefreshCw className={`h-4 w-4 mr-2${bridgeTokenRegenerating ? ' animate-spin' : ''}`} />
                {bridgeTokenRegenerating ? t('bridge.actions.regenerating') : t('bridge.actions.regenerate')}
              </Button>
            </div>
            <p className="text-xs text-muted-foreground">
              {bridgeTokenInfo?.managedByEnv ? t('bridge.envManagedHelp') : t('bridge.restartNote')}
            </p>
          </div>

          <div className="flex items-center justify-between gap-3 rounded-lg border border-border/60 bg-background/40 p-3">
            <div>
              <p className="text-sm text-muted-foreground">{t('bridge.networkRestartNote')}</p>
              <p className="text-xs text-muted-foreground">{t('bridge.networkRestartHint')}</p>
            </div>
            <Button
              type="button"
              variant="outline"
              onClick={() => setShowBridgeNetworkConfirm(true)}
              disabled={!bridgeNetworkDirty || bridgeNetworkLoading || bridgeNetworkSaving}
            >
              <RefreshCw className={`h-4 w-4 mr-2${bridgeNetworkSaving ? ' animate-spin' : ''}`} />
              {bridgeNetworkSaving ? t('common:status.saving') : t('common:actions.save')}
            </Button>
          </div>

          {bridgeTokenError ? (
            <div className="rounded-lg border border-red-500/30 bg-red-500/5 p-3 text-sm text-red-500">
              {bridgeTokenError}
            </div>
          ) : null}

          {bridgeNetworkError ? (
            <div className="rounded-lg border border-red-500/30 bg-red-500/5 p-3 text-sm text-red-500">
              {bridgeNetworkError}
            </div>
          ) : null}
        </CardContent>
      </Card>

      {/* Advanced */}
      <Card>
        <CardHeader>
          <CardTitle>{t('advanced.title')}</CardTitle>
          <CardDescription>{t('advanced.description')}</CardDescription>
        </CardHeader>
        <CardContent className="space-y-4">
          <div className="flex items-center justify-between">
            <div>
              <Label>{t('advanced.devMode')}</Label>
              <p className="text-sm text-muted-foreground">
                {t('advanced.devModeDesc')}
              </p>
            </div>
            <Switch
              checked={devModeUnlocked}
              onCheckedChange={setDevModeUnlocked}
            />
          </div>
        </CardContent>
      </Card>

      {/* Developer */}
      {devModeUnlocked && (
        <Card>
          <CardHeader>
            <CardTitle>{t('developer.title')}</CardTitle>
            <CardDescription>{t('developer.description')}</CardDescription>
          </CardHeader>
          <CardContent className="space-y-4">
            <div className="space-y-2">
              <Label>{t('developer.console')}</Label>
              <p className="text-sm text-muted-foreground">
                {t('developer.consoleDesc')}
              </p>
              <Button variant="outline" onClick={openDevConsole}>
                <Terminal className="h-4 w-4 mr-2" />
                {t('developer.openConsole')}
                <ExternalLink className="h-3 w-3 ml-2" />
              </Button>
              <p className="text-xs text-muted-foreground">
                {t('developer.consoleNote')}
              </p>
              <div className="space-y-2 pt-2">
                <Label>{t('developer.gatewayToken')}</Label>
                <p className="text-sm text-muted-foreground">
                  {t('developer.gatewayTokenDesc')}
                </p>
                <div className="flex gap-2">
                  <Input
                    readOnly
                    value={controlUiInfo?.token || ''}
                    placeholder={t('developer.tokenUnavailable')}
                    className="font-mono"
                  />
                  <Button
                    type="button"
                    variant="outline"
                    onClick={() => { void refreshControlUiInfo(true); }}
                    disabled={!devModeUnlocked || controlUiInfoLoading}
                  >
                    <RefreshCw className={`h-4 w-4 mr-2${controlUiInfoLoading ? ' animate-spin' : ''}`} />
                    {t('common:actions.load')}
                  </Button>
                  <Button
                    type="button"
                    variant="outline"
                    onClick={handleCopyGatewayToken}
                    disabled={!controlUiInfo?.token}
                  >
                    <Copy className="h-4 w-4 mr-2" />
                    {t('common:actions.copy')}
                  </Button>
                </div>
              </div>
            </div>
            {showCliTools && (
              <>
                <Separator />
                <div className="space-y-2">
                  <Label>{t('developer.cli')}</Label>
                  <p className="text-sm text-muted-foreground">
                    {t('developer.cliDesc')}
                  </p>
                  {isWindows && (
                    <p className="text-xs text-muted-foreground">
                      {t('developer.cliPowershell')}
                    </p>
                  )}
                  <div className="flex gap-2">
                    <Input
                      readOnly
                      value={openclawCliCommand}
                      placeholder={
                        openclawCliLoading
                          ? t('common:status.loading')
                          : (openclawCliError || t('developer.cmdUnavailable'))
                      }
                      className="font-mono"
                    />
                    <Button
                      type="button"
                      variant="outline"
                      onClick={handleCopyCliCommand}
                      disabled={!openclawCliCommand}
                    >
                      <Copy className="h-4 w-4 mr-2" />
                      {t('common:actions.copy')}
                    </Button>
                  </div>
                </div>
              </>
            )}
          </CardContent>
        </Card>
      )}

      <ConfirmDialog
        open={showBridgeTokenConfirm}
        title={t('bridge.dialog.title')}
        message={t('bridge.dialog.message')}
        confirmLabel={t('bridge.actions.regenerate')}
        cancelLabel={t('common:actions.cancel')}
        onConfirm={() => { void handleConfirmBridgeTokenRegeneration(); }}
        onCancel={() => {
          if (!bridgeTokenRegenerating) {
            setShowBridgeTokenConfirm(false);
          }
        }}
      />

      <ConfirmDialog
        open={showBridgeNetworkConfirm}
        title={t('bridge.networkDialog.title')}
        message={t('bridge.networkDialog.message')}
        confirmLabel={t('common:actions.save')}
        cancelLabel={t('common:actions.cancel')}
        onConfirm={() => { void handleConfirmBridgeNetworkSave(); }}
        onCancel={() => {
          if (!bridgeNetworkSaving) {
            setShowBridgeNetworkConfirm(false);
          }
        }}
      />

      {/* About */}
      <Card>
        <CardHeader>
          <CardTitle>{t('about.title')}</CardTitle>
        </CardHeader>
        <CardContent className="space-y-2 text-sm text-muted-foreground">
          <p>
            <strong>{t('about.appName')}</strong> - {t('about.tagline')}
          </p>
          <p>{t('about.basedOn')}</p>
          <p>{t('about.version', { version: currentVersion })}</p>
          <div className="flex gap-4 pt-2">
            <Button
              variant="link"
              className="h-auto p-0"
              onClick={() => desktopApi.openExternal('https://clawy.wymsn.com')}
            >
              {t('about.docs')}
            </Button>
            <Button
              variant="link"
              className="h-auto p-0"
              onClick={() => desktopApi.openExternal('https://github.com/edwardZhang/Clawy')}
            >
              {t('about.github')}
            </Button>
          </div>
        </CardContent>
      </Card>
    </div>
  );
}

export default Settings;
