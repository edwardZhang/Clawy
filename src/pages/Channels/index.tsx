import { desktopApi } from '@/lib/desktop/api';
/**
 * Channels Page
 * Manage messaging channel connections with configuration UI
 */
import { useState, useEffect, useCallback, useRef, type SVGProps } from 'react';
import {
  Plus,
  Radio,
  RefreshCw,
  Trash2,
  Power,
  PowerOff,
  QrCode,
  Loader2,
  X,
  ExternalLink,
  BookOpen,
  Eye,
  EyeOff,
  Check,
  AlertCircle,
  CheckCircle,
  ShieldCheck,
} from 'lucide-react';
import {
  SiDiscord,
  SiElement,
  SiTelegram,
  SiWhatsapp,
} from '@icons-pack/react-simple-icons';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { Label } from '@/components/ui/label';
import { Select } from '@/components/ui/select';
import { Textarea } from '@/components/ui/textarea';
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card';
import { Separator } from '@/components/ui/separator';
import { Badge } from '@/components/ui/badge';
import { ConfirmDialog } from '@/components/ui/confirm-dialog';
import { useChannelsStore } from '@/stores/channels';
import { useGatewayStore } from '@/stores/gateway';
import { StatusBadge, type Status } from '@/components/common/StatusBadge';
import { LoadingSpinner } from '@/components/common/LoadingSpinner';
import {
  CHANNEL_ICONS,
  CHANNEL_NAMES,
  CHANNEL_META,
  getDefaultChannelConfigValues,
  getPrimaryChannels,
  type ChannelType,
  type Channel,
  type ChannelMeta,
  type ChannelConfigField,
} from '@/types/channel';
import { toast } from 'sonner';
import { useTranslation } from 'react-i18next';
import dingTalkIconUrl from '../../../resources/dingtalk.svg';

function LarkBrandIcon(props: SVGProps<SVGSVGElement>) {
  return (
    <svg viewBox="0 0 24 24" fill="none" aria-hidden="true" {...props}>
      <path
        fill="currentColor"
        d="M12.9 2.58a1 1 0 0 0-1.53.2L6.8 10.15a1 1 0 0 0 .86 1.55h3.18l-2.98 4.93a1 1 0 0 0 1.4 1.35l7.9-5.08a1 1 0 0 0-.55-1.84h-3.16l2.95-6.7a1 1 0 0 0-1.5-1.78L12.9 2.58Z"
      />
    </svg>
  );
}

function renderChannelBrandIcon(type: ChannelType, className: string) {
  switch (type) {
    case 'telegram':
      return <SiTelegram className={className} style={{ color: '#26A5E4' }} />;
    case 'discord':
      return <SiDiscord className={className} style={{ color: '#5865F2' }} />;
    case 'whatsapp':
      return <SiWhatsapp className={className} style={{ color: '#25D366' }} />;
    case 'matrix':
      return <SiElement className={className} style={{ color: '#0DBD8B' }} />;
    case 'dingtalk':
      return (
        <img
          src={dingTalkIconUrl}
          alt="DingTalk"
          className={`${className} object-contain`}
        />
      );
    case 'feishu':
      return <LarkBrandIcon className={className} style={{ color: '#00B96B' }} />;
    default:
      return (
        <span className={className.includes('h-7') ? 'text-2xl' : 'text-3xl'}>
          {CHANNEL_ICONS[type]}
        </span>
      );
  }
}

export function Channels() {
  const { t } = useTranslation('channels');
  const { channels, loading, error, fetchChannels, deleteChannel } = useChannelsStore();
  const gatewayStatus = useGatewayStore((state) => state.status);

  const [showAddDialog, setShowAddDialog] = useState(false);
  const [selectedChannelType, setSelectedChannelType] = useState<ChannelType | null>(null);
  const [configuredTypes, setConfiguredTypes] = useState<string[]>([]);
  const [channelToDelete, setChannelToDelete] = useState<{ id: string } | null>(null);

  // Fetch channels on mount
  useEffect(() => {
    fetchChannels();
  }, [fetchChannels]);

  // Fetch configured channel types from config file
  const fetchConfiguredTypes = useCallback(async () => {
    try {
      const result = await desktopApi.ipcRenderer.invoke('channel:listConfigured') as {
        success: boolean;
        channels?: string[];
      };
      if (result.success && result.channels) {
        setConfiguredTypes(result.channels);
      }
    } catch {
      // ignore
    }
  }, []);

  useEffect(() => {
    // eslint-disable-next-line react-hooks/set-state-in-effect
    void fetchConfiguredTypes();
  }, [fetchConfiguredTypes]);

  useEffect(() => {
    const unsubscribe = desktopApi.ipcRenderer.on('gateway:channel-status', () => {
      fetchChannels();
      fetchConfiguredTypes();
    });
    return () => {
      if (typeof unsubscribe === 'function') {
        unsubscribe();
      }
    };
  }, [fetchChannels, fetchConfiguredTypes]);

  // Get channel types to display
  const displayedChannelTypes = getPrimaryChannels();

  // Connected/disconnected channel counts
  const connectedCount = channels.filter((c) => c.status === 'connected').length;

  if (loading) {
    return (
      <div className="flex h-96 items-center justify-center">
        <LoadingSpinner size="lg" />
      </div>
    );
  }

  return (
    <div className="space-y-6">
      {/* Header */}
      <div className="flex items-center justify-between">
        <div>
          <h1 className="text-2xl font-bold">{t('title')}</h1>
          <p className="text-muted-foreground">
            {t('subtitle')}
          </p>
        </div>
        <div className="flex gap-2">
          <Button variant="outline" onClick={fetchChannels}>
            <RefreshCw className="h-4 w-4 mr-2" />
            {t('refresh')}
          </Button>
          <Button onClick={() => setShowAddDialog(true)}>
            <Plus className="h-4 w-4 mr-2" />
            {t('addChannel')}
          </Button>
        </div>
      </div>

      {/* Stats */}
      <div className="grid grid-cols-3 gap-4">
        <Card>
          <CardContent className="pt-6">
            <div className="flex items-center gap-4">
              <div className="rounded-full bg-primary/10 p-3">
                <Radio className="h-6 w-6 text-primary" />
              </div>
              <div>
                <p className="text-2xl font-bold">{channels.length}</p>
                <p className="text-sm text-muted-foreground">{t('stats.total')}</p>
              </div>
            </div>
          </CardContent>
        </Card>
        <Card>
          <CardContent className="pt-6">
            <div className="flex items-center gap-4">
              <div className="rounded-full bg-green-100 p-3 dark:bg-green-900">
                <Power className="h-6 w-6 text-green-600" />
              </div>
              <div>
                <p className="text-2xl font-bold">{connectedCount}</p>
                <p className="text-sm text-muted-foreground">{t('stats.connected')}</p>
              </div>
            </div>
          </CardContent>
        </Card>
        <Card>
          <CardContent className="pt-6">
            <div className="flex items-center gap-4">
              <div className="rounded-full bg-slate-100 p-3 dark:bg-slate-800">
                <PowerOff className="h-6 w-6 text-slate-600" />
              </div>
              <div>
                <p className="text-2xl font-bold">{channels.length - connectedCount}</p>
                <p className="text-sm text-muted-foreground">{t('stats.disconnected')}</p>
              </div>
            </div>
          </CardContent>
        </Card>
      </div>

      {/* Gateway Warning */}
      {gatewayStatus.state !== 'running' && (
        <Card className="border-yellow-500 bg-yellow-50 dark:bg-yellow-900/10">
          <CardContent className="py-4 flex items-center gap-3">
            <AlertCircle className="h-5 w-5 text-yellow-500" />
            <span className="text-yellow-700 dark:text-yellow-400">
              {t('gatewayWarning')}
            </span>
          </CardContent>
        </Card>
      )}

      {/* Error Display */}
      {error && (
        <Card className="border-destructive">
          <CardContent className="py-4 text-destructive">
            {error}
          </CardContent>
        </Card>
      )}

      {/* Configured Channels */}
      {channels.length > 0 && (
        <Card>
          <CardHeader>
            <CardTitle>{t('configured')}</CardTitle>
            <CardDescription>{t('configuredDesc')}</CardDescription>
          </CardHeader>
          <CardContent>
            <div className="grid grid-cols-1 gap-4 md:grid-cols-2 lg:grid-cols-3">
              {channels.map((channel) => (
                <ChannelCard
                  key={channel.id}
                  channel={channel}
                  onDelete={() => setChannelToDelete({ id: channel.id })}
                />
              ))}
            </div>
          </CardContent>
        </Card>
      )}

      {/* Available Channels */}
      <Card>
        <CardHeader>
          <div className="flex items-center justify-between">
            <div>
              <CardTitle>{t('available')}</CardTitle>
              <CardDescription>
                {t('availableDesc')}
              </CardDescription>
            </div>
          </div>
        </CardHeader>
        <CardContent>
          <div className="grid grid-cols-2 gap-4 md:grid-cols-4">
            {displayedChannelTypes.map((type) => {
              const meta = CHANNEL_META[type];
              const isConfigured = configuredTypes.includes(type);
              return (
                <button
                  key={type}
                  className={`p-4 rounded-lg border hover:bg-accent transition-colors text-left relative ${isConfigured ? 'border-green-500/50 bg-green-500/5' : ''}`}
                  onClick={() => {
                    setSelectedChannelType(type);
                    setShowAddDialog(true);
                  }}
                >
                  <span className="inline-flex h-8 w-8 items-center justify-center">
                    {renderChannelBrandIcon(type, 'h-8 w-8')}
                  </span>
                  <p className="font-medium mt-2">{meta.name}</p>
                  <p className="text-xs text-muted-foreground mt-1 line-clamp-2">
                    {t(meta.description)}
                  </p>
                  {isConfigured && (
                    <Badge className="absolute top-2 right-2 text-xs bg-green-600 hover:bg-green-600">
                      {t('configuredBadge')}
                    </Badge>
                  )}
                  {!isConfigured && meta.isPlugin && (
                    <Badge variant="secondary" className="absolute top-2 right-2 text-xs">
                      {t('pluginBadge')}
                    </Badge>
                  )}
                </button>
              );
            })}
          </div>
        </CardContent>
      </Card>

      {/* Add Channel Dialog */}
      {showAddDialog && (
        <AddChannelDialog
          selectedType={selectedChannelType}
          onSelectType={setSelectedChannelType}
          onClose={() => {
            setShowAddDialog(false);
            setSelectedChannelType(null);
          }}
          onChannelAdded={() => {
            fetchChannels();
            fetchConfiguredTypes();
            setShowAddDialog(false);
            setSelectedChannelType(null);
          }}
        />
      )}

      <ConfirmDialog
        open={!!channelToDelete}
        title={t('common.confirm', 'Confirm')}
        message={t('deleteConfirm')}
        confirmLabel={t('common.delete', 'Delete')}
        cancelLabel={t('common.cancel', 'Cancel')}
        variant="destructive"
        onConfirm={async () => {
          if (channelToDelete) {
            await deleteChannel(channelToDelete.id);
            setChannelToDelete(null);
          }
        }}
        onCancel={() => setChannelToDelete(null)}
      />
    </div>
  );
}

// ==================== Channel Card Component ====================

interface ChannelCardProps {
  channel: Channel;
  onDelete: () => void;
}

function ChannelCard({ channel, onDelete }: ChannelCardProps) {
  return (
    <Card>
      <CardHeader className="pb-3">
        <div className="flex items-start justify-between">
          <div className="flex items-center gap-3">
            <span className="inline-flex h-8 w-8 items-center justify-center">
              {renderChannelBrandIcon(channel.type, 'h-7 w-7')}
            </span>
            <div>
              <CardTitle className="text-base">{channel.name}</CardTitle>
              <CardDescription className="text-xs">
                {CHANNEL_NAMES[channel.type]}
              </CardDescription>
            </div>
          </div>
          <StatusBadge status={channel.status as Status} />
        </div>
      </CardHeader>
      <CardContent className="pt-0">
        {channel.error && (
          <p className="text-xs text-destructive mb-3">{channel.error}</p>
        )}
        <div className="flex gap-2">
          <Button
            variant="ghost"
            size="sm"
            className="text-destructive hover:text-destructive"
            onClick={onDelete}
          >
            <Trash2 className="h-4 w-4" />
          </Button>
        </div>
      </CardContent>
    </Card>
  );
}

// ==================== Add Channel Dialog ====================

interface AddChannelDialogProps {
  selectedType: ChannelType | null;
  onSelectType: (type: ChannelType | null) => void;
  onClose: () => void;
  onChannelAdded: () => void;
}

interface ChannelPluginStatus {
  required: boolean;
  channelType: string;
  pluginId?: string;
  installed: boolean;
  enabled: boolean;
  status?: 'loaded' | 'disabled' | 'error' | 'missing' | string;
  origin?: 'bundled' | 'npm' | 'path' | 'legacy-mirror' | 'unknown' | string;
  message?: string;
}

type ConnectStage = 'validating' | 'installingPlugin' | 'saving';
const PLUGIN_STATUS_CHANNEL_TYPES: ChannelType[] = ['matrix', 'dingtalk', 'qqbot', 'wecom', 'wecom-app'];

function AddChannelDialog({ selectedType, onSelectType, onClose, onChannelAdded }: AddChannelDialogProps) {
  const { t } = useTranslation('channels');
  const { addChannel } = useChannelsStore();
  const [configValues, setConfigValues] = useState<Record<string, string>>({});
  const [channelName, setChannelName] = useState('');
  const [connecting, setConnecting] = useState(false);
  const [connectStage, setConnectStage] = useState<ConnectStage>('validating');
  const [showSecrets, setShowSecrets] = useState<Record<string, boolean>>({});
  const [qrCode, setQrCode] = useState<string | null>(null);
  const [validating, setValidating] = useState(false);
  const [loadingConfig, setLoadingConfig] = useState(false);
  const [isExistingConfig, setIsExistingConfig] = useState(false);
  const [pluginStatus, setPluginStatus] = useState<ChannelPluginStatus | null>(null);
  const [pluginStatusLoading, setPluginStatusLoading] = useState(false);
  const firstInputRef = useRef<HTMLInputElement>(null);
  const pluginStatusRequestIdRef = useRef<string | null>(null);
  const [validationResult, setValidationResult] = useState<{
    valid: boolean;
    errors: string[];
    warnings: string[];
  } | null>(null);

  const meta: ChannelMeta | null = selectedType ? CHANNEL_META[selectedType] : null;

  // Load existing config when a channel type is selected
  useEffect(() => {
    if (!selectedType) {
      setConfigValues({});
      setChannelName('');
      setIsExistingConfig(false);
      setPluginStatus(null);
      setPluginStatusLoading(false);
      setChannelName('');
      setIsExistingConfig(false);
      // Ensure we clean up any pending QR session if switching away
      desktopApi.ipcRenderer.invoke('channel:cancelWhatsAppQr').catch(() => { });
      return;
    }

    let cancelled = false;
    setLoadingConfig(true);

    (async () => {
      try {
        const result = await desktopApi.ipcRenderer.invoke(
          'channel:getFormValues',
          selectedType
        ) as { success: boolean; values?: Record<string, string> };
        const defaultConfigValues = getDefaultChannelConfigValues(selectedType);

        if (cancelled) return;

        if (result.success && result.values && Object.keys(result.values).length > 0) {
          setConfigValues({ ...defaultConfigValues, ...result.values });
          setIsExistingConfig(true);
        } else {
          setConfigValues(defaultConfigValues);
          setIsExistingConfig(false);
        }
      } catch {
        if (!cancelled) {
          setConfigValues(getDefaultChannelConfigValues(selectedType));
          setIsExistingConfig(false);
        }
      } finally {
        if (!cancelled) setLoadingConfig(false);
      }
    })();

    return () => { cancelled = true; };
  }, [selectedType]);

  useEffect(() => {
    const unsubscribe = desktopApi.ipcRenderer.on('channel:plugin-status', (payload) => {
      const event = payload as {
        requestId?: string;
        success?: boolean;
        status?: ChannelPluginStatus;
        error?: string;
        channelType?: string;
      };

      if (!event?.requestId || event.requestId !== pluginStatusRequestIdRef.current) {
        return;
      }

      pluginStatusRequestIdRef.current = null;

      if (event.success && event.status) {
        setPluginStatus(event.status);
      } else {
        setPluginStatus({
          required: true,
          channelType: event.channelType || '',
          pluginId: event.channelType || undefined,
          installed: false,
          enabled: false,
          status: 'error',
          origin: 'unknown',
          message: event.error || 'Failed to fetch plugin status',
        });
      }
      setPluginStatusLoading(false);
    });

    return () => {
      if (typeof unsubscribe === 'function') {
        unsubscribe();
      }
    };
  }, []);

  useEffect(() => {
    if (!selectedType || !PLUGIN_STATUS_CHANNEL_TYPES.includes(selectedType)) {
      pluginStatusRequestIdRef.current = null;
      setPluginStatus(null);
      setPluginStatusLoading(false);
      return;
    }

    let cancelled = false;
    setPluginStatusLoading(true);
    const requestId = `${selectedType}-plugin-${Date.now()}-${Math.random().toString(36).slice(2, 10)}`;
    pluginStatusRequestIdRef.current = requestId;

    const timer = window.setTimeout(() => {
      desktopApi.ipcRenderer
        .invoke('channel:getPluginStatusAsync', selectedType, requestId)
        .catch((error) => {
          if (!cancelled && pluginStatusRequestIdRef.current === requestId) {
            pluginStatusRequestIdRef.current = null;
            setPluginStatus({
              required: true,
              channelType: selectedType,
              pluginId: selectedType,
              installed: false,
              enabled: false,
              status: 'error',
              origin: 'unknown',
              message: String(error),
            });
            setPluginStatusLoading(false);
          }
        });
    }, 0);

    return () => {
      cancelled = true;
      window.clearTimeout(timer);
      if (pluginStatusRequestIdRef.current === requestId) {
        pluginStatusRequestIdRef.current = null;
      }
    };
  }, [selectedType]);

  // Focus first input when form is ready (avoids Windows focus loss after native dialogs)
  useEffect(() => {
    if (selectedType && !loadingConfig && firstInputRef.current) {
      firstInputRef.current.focus();
    }
  }, [selectedType, loadingConfig]);

  // Listen for WhatsApp QR events
  useEffect(() => {
    if (selectedType !== 'whatsapp') return;

    const onQr = (...args: unknown[]) => {
      const data = args[0] as { qr: string; raw: string };
      setQrCode(`data:image/png;base64,${data.qr}`);
    };

    const onSuccess = async (...args: unknown[]) => {
      const data = args[0] as { accountId?: string } | undefined;
      toast.success(t('toast.whatsappConnected'));
      const accountId = data?.accountId || channelName.trim() || 'default';
      try {
        const saveResult = await desktopApi.ipcRenderer.invoke(
          'channel:saveConfig',
          'whatsapp',
          { enabled: true }
        ) as { success?: boolean; error?: string };
        if (!saveResult?.success) {
          console.error('Failed to save WhatsApp config:', saveResult?.error);
        } else {
          console.info('Saved WhatsApp config for account:', accountId);
        }
      } catch (error) {
        console.error('Failed to save WhatsApp config:', error);
      }
      // Register the channel locally so it shows up immediately
      addChannel({
        type: 'whatsapp',
        name: channelName || 'WhatsApp',
      }).then(() => {
        // Restart gateway to pick up the new session
        desktopApi.ipcRenderer.invoke('gateway:restart').catch(console.error);
        onChannelAdded();
      });
    };

    const onError = (...args: unknown[]) => {
      const err = args[0] as string;
      console.error('WhatsApp Login Error:', err);
      toast.error(t('toast.whatsappFailed', { error: err }));
      setQrCode(null);
      setConnecting(false);
    };

    const removeQrListener = desktopApi.ipcRenderer.on('channel:whatsapp-qr', onQr);
    const removeSuccessListener = desktopApi.ipcRenderer.on('channel:whatsapp-success', onSuccess);
    const removeErrorListener = desktopApi.ipcRenderer.on('channel:whatsapp-error', onError);

    return () => {
      if (typeof removeQrListener === 'function') removeQrListener();
      if (typeof removeSuccessListener === 'function') removeSuccessListener();
      if (typeof removeErrorListener === 'function') removeErrorListener();
      // Cancel when unmounting or switching types
      desktopApi.ipcRenderer.invoke('channel:cancelWhatsAppQr').catch(() => { });
    };
  }, [selectedType, addChannel, channelName, onChannelAdded, t]);

  const handleValidate = async () => {
    if (!selectedType) return;

    setValidating(true);
    setValidationResult(null);

    try {
      const result = await desktopApi.ipcRenderer.invoke(
        'channel:validateCredentials',
        selectedType,
        configValues
      ) as {
        success: boolean;
        valid?: boolean;
        errors?: string[];
        warnings?: string[];
        details?: Record<string, string>;
      };

      const warnings = result.warnings || [];
      if (result.valid && result.details) {
        const details = result.details;
        if (details.botUsername) warnings.push(`Bot: @${details.botUsername}`);
        if (details.guildName) warnings.push(`Server: ${details.guildName}`);
        if (details.channelName) warnings.push(`Channel: #${details.channelName}`);
      }

      setValidationResult({
        valid: result.valid || false,
        errors: result.errors || [],
        warnings,
      });
    } catch (error) {
      setValidationResult({
        valid: false,
        errors: [String(error)],
        warnings: [],
      });
    } finally {
      setValidating(false);
    }
  };


  const handleConnect = async () => {
    if (!selectedType || !meta) return;

    setConnecting(true);
    setConnectStage('validating');
    setValidationResult(null);

    try {
      // For QR-based channels, request QR code
      if (meta.connectionType === 'qr') {
        const accountId = channelName.trim() || 'default';
        await desktopApi.ipcRenderer.invoke('channel:requestWhatsAppQr', accountId);
        // The QR code will be set via event listener
        return;
      }

      // Step 1: Validate credentials against the actual service API
      if (meta.connectionType === 'token') {
        const validationResponse = await desktopApi.ipcRenderer.invoke(
          'channel:validateCredentials',
          selectedType,
          configValues
        ) as {
          success: boolean;
          valid?: boolean;
          errors?: string[];
          warnings?: string[];
          details?: Record<string, string>;
        };

        if (!validationResponse.valid) {
          setValidationResult({
            valid: false,
            errors: validationResponse.errors || ['Validation failed'],
            warnings: validationResponse.warnings || [],
          });
          setConnecting(false);
          return;
        }

        // Show success details (bot name, guild name, etc.) as warnings/info
        const warnings = validationResponse.warnings || [];
        if (validationResponse.details) {
          const details = validationResponse.details;
          if (details.botUsername) {
            warnings.push(`Bot: @${details.botUsername}`);
          }
          if (details.guildName) {
            warnings.push(`Server: ${details.guildName}`);
          }
          if (details.channelName) {
            warnings.push(`Channel: #${details.channelName}`);
          }
        }

        // Show validation success with details
        setValidationResult({
          valid: true,
          errors: [],
          warnings,
        });
      }

      // Step 2: Save channel configuration via IPC
      const config: Record<string, unknown> = { ...configValues };
      if (selectedType && PLUGIN_STATUS_CHANNEL_TYPES.includes(selectedType)) {
        setConnectStage(pluginStatus?.status === 'loaded' ? 'saving' : 'installingPlugin');
      } else {
        setConnectStage('saving');
      }
      const saveResult = await desktopApi.ipcRenderer.invoke('channel:saveConfig', selectedType, config) as {
        success?: boolean;
        error?: string;
        warning?: string;
        pluginEnsured?: boolean;
        pluginAction?: 'none' | 'enabled' | 'installed' | 'legacy-installed';
      };
      if (!saveResult?.success) {
        throw new Error(saveResult?.error || 'Failed to save channel config');
      }
      setConnectStage('saving');
      if (typeof saveResult.warning === 'string' && saveResult.warning) {
        toast.warning(saveResult.warning);
      }

      // Step 3: Add a local channel entry for the UI
      await addChannel({
        type: selectedType,
        name: channelName || CHANNEL_NAMES[selectedType],
        token: configValues[meta.configFields[0]?.key] || undefined,
      });

      toast.success(t('toast.channelSaved', { name: meta.name }));

      // Gateway restart is now handled server-side via debouncedRestart()
      // inside the channel:saveConfig IPC handler, so we don't need to
      // trigger it explicitly here.  This avoids cascading restarts when
      // multiple config changes happen in quick succession (e.g. during
      // the setup wizard).
      toast.success(t('toast.channelConnecting', { name: meta.name }));

      // Brief delay so user can see the success state before dialog closes
      await new Promise((resolve) => setTimeout(resolve, 800));
      if (selectedType && PLUGIN_STATUS_CHANNEL_TYPES.includes(selectedType)) {
        setPluginStatus({
          required: true,
          channelType: selectedType,
          pluginId: selectedType,
          installed: true,
          enabled: true,
          status: 'loaded',
          origin: saveResult.pluginAction === 'installed' ? 'npm' : (pluginStatus?.origin || 'unknown'),
        });
      }
      onChannelAdded();
    } catch (error) {
      toast.error(t('toast.configFailed', { error }));
      setConnecting(false);
      setConnectStage('validating');
    }
  };

  const openDocs = () => {
    if (meta?.docsUrl) {
      const url = t(meta.docsUrl);
      try {
        if (desktopApi.openExternal) {
          desktopApi.openExternal(url);
        } else {
          // Fallback: open in new window
          window.open(url, '_blank');
        }
      } catch (error) {
        console.error('Failed to open docs:', error);
        // Fallback: open in new window
        window.open(url, '_blank');
      }
    }
  };


  const isFormValid = () => {
    if (!meta) return false;

    // Check all required fields are filled
    return meta.configFields
      .filter((field) => field.required)
      .every((field) => configValues[field.key]?.trim());
  };

  const updateConfigValue = (key: string, value: string) => {
    setConfigValues((prev) => ({ ...prev, [key]: value }));
  };

  const toggleSecretVisibility = (key: string) => {
    setShowSecrets((prev) => ({ ...prev, [key]: !prev[key] }));
  };

  const matrixNeedsInstallFromBundled =
    selectedType === 'matrix' &&
    pluginStatus?.origin === 'bundled' &&
    pluginStatus?.status !== 'loaded';

  const pluginStatusTone = (() => {
    switch (pluginStatus?.status) {
      case 'loaded':
        return 'bg-green-500/10 text-green-600 dark:text-green-400';
      case 'error':
        return 'bg-destructive/10 text-destructive';
      default:
        return 'bg-blue-500/10 text-blue-600 dark:text-blue-400';
    }
  })();

  const pluginStatusMessage = (() => {
    if (!selectedType || !pluginStatus?.required) {
      return null;
    }

    if (matrixNeedsInstallFromBundled) {
      return t('dialog.pluginStatus.missing', { name: CHANNEL_NAMES[selectedType] });
    }

    switch (pluginStatus?.status) {
      case 'missing':
        return t('dialog.pluginStatus.missing', { name: CHANNEL_NAMES[selectedType] });
      case 'disabled':
        return t('dialog.pluginStatus.disabled', { name: CHANNEL_NAMES[selectedType] });
      case 'loaded':
        return t('dialog.pluginStatus.loaded', { name: CHANNEL_NAMES[selectedType] });
      case 'error':
        return pluginStatus.message || t('dialog.pluginStatus.error', { name: CHANNEL_NAMES[selectedType] });
      default:
        return null;
    }
  })();

  const connectButtonLabel = (() => {
    if (meta?.connectionType === 'qr') {
      return t('dialog.generatingQR');
    }
    if (connectStage === 'installingPlugin') {
      return t('dialog.installingPlugin');
    }
    if (connectStage === 'saving') {
      return t('dialog.savingAndRestarting');
    }
    return t('dialog.validating');
  })();

  return (
    <div className="fixed inset-0 z-50 bg-black/50 flex items-center justify-center p-4">
      <Card className="w-full max-w-lg max-h-[90vh] overflow-y-auto">
        <CardHeader className="flex flex-row items-start justify-between">
          <div>
            <CardTitle>
              {selectedType
                ? isExistingConfig
                  ? t('dialog.updateTitle', { name: CHANNEL_NAMES[selectedType] })
                  : t('dialog.configureTitle', { name: CHANNEL_NAMES[selectedType] })
                : t('dialog.addTitle')}
            </CardTitle>
            <CardDescription>
              {selectedType && isExistingConfig
                ? t('dialog.existingDesc')
                : meta ? t(meta.description) : t('dialog.selectDesc')}
            </CardDescription>
          </div>
          <Button variant="ghost" size="icon" onClick={onClose}>
            <X className="h-4 w-4" />
          </Button>
        </CardHeader>
        <CardContent className="space-y-4">
          {!selectedType ? (
            // Channel type selection
            <div className="grid grid-cols-2 gap-4">
              {getPrimaryChannels().map((type) => {
                const channelMeta = CHANNEL_META[type];
                return (
                  <button
                    key={type}
                    onClick={() => onSelectType(type)}
                    className="p-4 rounded-lg border hover:bg-accent transition-colors text-left"
                  >
                    <span className="text-3xl">{channelMeta.icon}</span>
                    <p className="font-medium mt-2">{channelMeta.name}</p>
                    <p className="text-xs text-muted-foreground mt-1">
                      {channelMeta.connectionType === 'qr' ? t('dialog.qrCode') : t('dialog.token')}
                    </p>
                  </button>
                );
              })}
            </div>
          ) : qrCode ? (
            // QR Code display
            <div className="text-center space-y-4">
              <div className="bg-white p-4 rounded-lg inline-block shadow-sm border">
                {qrCode.startsWith('data:image') ? (
                  <img src={qrCode} alt="Scan QR Code" className="w-64 h-64 object-contain" />
                ) : (
                  <div className="w-64 h-64 bg-gray-100 flex items-center justify-center">
                    <QrCode className="h-32 w-32 text-gray-400" />
                  </div>
                )}
              </div>
              <p className="text-sm text-muted-foreground">
                {t('dialog.scanQR', { name: meta?.name })}
              </p>
              <div className="flex justify-center gap-2">
                <Button variant="outline" onClick={() => {
                  setQrCode(null);
                  handleConnect(); // Retry
                }}>
                  {t('dialog.refreshCode')}
                </Button>
              </div>
            </div>
          ) : loadingConfig ? (
            // Loading saved config
            <div className="flex items-center justify-center py-8">
              <Loader2 className="h-6 w-6 animate-spin text-muted-foreground" />
              <span className="ml-2 text-sm text-muted-foreground">{t('dialog.loadingConfig')}</span>
            </div>
          ) : (
            // Connection form
            <div className="space-y-4">
              {/* Existing config hint */}
              {isExistingConfig && (
                <div className="bg-blue-500/10 text-blue-600 dark:text-blue-400 p-3 rounded-lg text-sm flex items-center gap-2">
                  <CheckCircle className="h-4 w-4 shrink-0" />
                  <span>{t('dialog.existingHint')}</span>
                </div>
              )}

              {/* Matrix plugin status */}
              {selectedType && PLUGIN_STATUS_CHANNEL_TYPES.includes(selectedType) && pluginStatus?.required && (
                pluginStatusLoading ? (
                  <div className="bg-muted p-3 rounded-lg text-sm flex items-center gap-2 text-muted-foreground">
                    <Loader2 className="h-4 w-4 animate-spin shrink-0" />
                    <span>{t('dialog.pluginStatus.loading', { name: CHANNEL_NAMES[selectedType] })}</span>
                  </div>
                ) : pluginStatusMessage ? (
                  <div className={`p-3 rounded-lg text-sm flex items-start gap-2 ${pluginStatusTone}`}>
                    {pluginStatus?.status === 'loaded' ? (
                      <CheckCircle className="h-4 w-4 mt-0.5 shrink-0" />
                    ) : (
                      <AlertCircle className="h-4 w-4 mt-0.5 shrink-0" />
                    )}
                    <div className="min-w-0">
                      <p>{pluginStatusMessage}</p>
                      {pluginStatus?.message && pluginStatus.status !== 'error' && (
                        <p className="text-xs mt-1 opacity-80">{pluginStatus.message}</p>
                      )}
                    </div>
                  </div>
                ) : null
              )}

              {/* Instructions */}
              <div className="bg-muted p-4 rounded-lg space-y-3">
                <div className="flex items-center justify-between">
                  <p className="font-medium text-sm">{t('dialog.howToConnect')}</p>
                  <Button
                    variant="link"
                    className="p-0 h-auto text-sm"
                    onClick={openDocs}
                  >
                    <BookOpen className="h-3 w-3 mr-1" />
                    {t('dialog.viewDocs')}
                    <ExternalLink className="h-3 w-3 ml-1" />
                  </Button>
                </div>
                <ol className="list-decimal list-inside text-sm text-muted-foreground space-y-1">
                  {meta?.instructions.map((instruction, i) => (
                    <li key={i}>{t(instruction)}</li>
                  ))}
                </ol>
              </div>

              {/* Channel name */}
              <div className="space-y-2">
                <Label htmlFor="name">{t('dialog.channelName')}</Label>
                <Input
                  ref={firstInputRef}
                  id="name"
                  placeholder={t('dialog.channelNamePlaceholder', { name: meta?.name })}
                  value={channelName}
                  onChange={(e) => setChannelName(e.target.value)}
                />
              </div>

              {/* Configuration fields */}
              {meta?.configFields.map((field) => (
                <ConfigField
                  key={field.key}
                  field={field}
                  value={configValues[field.key] || ''}
                  onChange={(value) => updateConfigValue(field.key, value)}
                  showSecret={showSecrets[field.key] || false}
                  onToggleSecret={() => toggleSecretVisibility(field.key)}
                />
              ))}

              {/* Validation Results */}
              {validationResult && (
                <div className={`p-4 rounded-lg text-sm ${validationResult.valid ? 'bg-green-500/10 text-green-600 dark:text-green-400' : 'bg-destructive/10 text-destructive'
                  }`}>
                  <div className="flex items-start gap-2">
                    {validationResult.valid ? (
                      <CheckCircle className="h-4 w-4 mt-0.5 shrink-0" />
                    ) : (
                      <AlertCircle className="h-4 w-4 mt-0.5 shrink-0" />
                    )}
                    <div className="min-w-0">
                      <h4 className="font-medium mb-1">
                        {validationResult.valid ? t('dialog.credentialsVerified') : t('dialog.validationFailed')}
                      </h4>
                      {validationResult.errors.length > 0 && (
                        <ul className="list-disc list-inside space-y-0.5">
                          {validationResult.errors.map((err, i) => (
                            <li key={i}>{err}</li>
                          ))}
                        </ul>
                      )}
                      {validationResult.valid && validationResult.warnings.length > 0 && (
                        <div className="mt-1 text-green-600 dark:text-green-400 space-y-0.5">
                          {validationResult.warnings.map((info, i) => (
                            <p key={i} className="text-xs">{info}</p>
                          ))}
                        </div>
                      )}
                      {!validationResult.valid && validationResult.warnings.length > 0 && (
                        <div className="mt-2 text-yellow-600 dark:text-yellow-500">
                          <p className="font-medium text-xs uppercase mb-1">{t('dialog.warnings')}</p>
                          <ul className="list-disc list-inside space-y-0.5">
                            {validationResult.warnings.map((warn, i) => (
                              <li key={i}>{warn}</li>
                            ))}
                          </ul>
                        </div>
                      )}
                    </div>
                  </div>
                </div>
              )}

              <Separator />

              <div className="flex justify-between">
                <Button variant="outline" onClick={() => onSelectType(null)}>
                  {t('dialog.back')}
                </Button>
                <div className="flex gap-2">
                  {/* Validation Button - Only for token-based channels for now */}
                  {meta?.connectionType === 'token' && (
                    <Button
                      variant="secondary"
                      onClick={handleValidate}
                      disabled={validating}
                    >
                      {validating ? (
                        <>
                          <Loader2 className="h-4 w-4 mr-2 animate-spin" />
                          {t('dialog.validating')}
                        </>
                      ) : (
                        <>
                          <ShieldCheck className="h-4 w-4 mr-2" />
                          {t('dialog.validateConfig')}
                        </>
                      )}
                    </Button>
                  )}
                  <Button
                    onClick={handleConnect}
                    disabled={connecting || !isFormValid()}
                  >
                    {connecting ? (
                      <>
                        <Loader2 className="h-4 w-4 mr-2 animate-spin" />
                        {connectButtonLabel}
                      </>
                    ) : meta?.connectionType === 'qr' ? (
                      t('dialog.generateQRCode')
                    ) : (
                      <>
                        <Check className="h-4 w-4 mr-2" />
                        {isExistingConfig ? t('dialog.updateAndReconnect') : t('dialog.saveAndConnect')}
                      </>
                    )}
                  </Button>
                </div>
              </div>
            </div>
          )}
        </CardContent>
      </Card>
    </div >
  );
}

// ==================== Config Field Component ====================

interface ConfigFieldProps {
  field: ChannelConfigField;
  value: string;
  onChange: (value: string) => void;
  showSecret: boolean;
  onToggleSecret: () => void;
}

function ConfigField({ field, value, onChange, showSecret, onToggleSecret }: ConfigFieldProps) {
  const { t } = useTranslation('channels');
  const isPassword = field.type === 'password';
  const isSelect = field.type === 'select';
  const isTextarea = field.type === 'textarea';

  return (
    <div className="space-y-2">
      <Label htmlFor={field.key}>
        {t(field.label)}
        {field.required && <span className="text-destructive ml-1">*</span>}
      </Label>
      {isSelect ? (
        <Select
          id={field.key}
          value={value}
          onChange={(e) => onChange(e.target.value)}
        >
          {field.options?.map((option) => (
            <option key={option.value} value={option.value}>
              {t(option.label)}
            </option>
          ))}
        </Select>
      ) : isTextarea ? (
        <Textarea
          id={field.key}
          placeholder={field.placeholder ? t(field.placeholder) : undefined}
          value={value}
          onChange={(e) => onChange(e.target.value)}
          className="font-mono text-sm min-h-24"
        />
      ) : (
        <div className="flex gap-2">
          <Input
            id={field.key}
            type={isPassword && !showSecret ? 'password' : 'text'}
            placeholder={field.placeholder ? t(field.placeholder) : undefined}
            value={value}
            onChange={(e) => onChange(e.target.value)}
            className="font-mono text-sm"
          />
          {isPassword && (
            <Button
              type="button"
              variant="outline"
              size="icon"
              onClick={onToggleSecret}
            >
              {showSecret ? <EyeOff className="h-4 w-4" /> : <Eye className="h-4 w-4" />}
            </Button>
          )}
        </div>
      )}
      {field.description && (
        <p className="text-xs text-muted-foreground">
          {t(field.description)}
        </p>
      )}
      {field.envVar && (
        <p className="text-xs text-muted-foreground">
          {t('dialog.envVar', { var: field.envVar })}
        </p>
      )}
    </div>
  );
}

export default Channels;
