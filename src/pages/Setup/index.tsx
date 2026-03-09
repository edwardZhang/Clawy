import { desktopApi } from '@/lib/desktop/api';
/**
 * Setup Wizard Page
 * First-time setup experience for new users
 */
import { useState, useEffect, useCallback, useRef, useMemo, useReducer } from 'react';
import { useNavigate } from 'react-router-dom';
import { motion, AnimatePresence } from 'framer-motion';
import {
  Check,
  ChevronDown,
  ChevronLeft,
  ChevronRight,
  Loader2,
  AlertCircle,
  Eye,
  EyeOff,
  RefreshCw,
  CheckCircle2,
  XCircle,
  ExternalLink,
  Copy,
} from 'lucide-react';
import { TitleBar } from '@/components/layout/TitleBar';
import { Button } from '@/components/ui/button';
import { Badge } from '@/components/ui/badge';
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card';
import { Input } from '@/components/ui/input';
import { Label } from '@/components/ui/label';
import { Progress } from '@/components/ui/progress';
import { cn } from '@/lib/utils';
import {
  createRuntimeCheckMachineState,
  runtimeCheckMachineReducer,
  runtimeChecksReady,
  runtimePrerequisitesReady,
  type RuntimeCheckState,
  type RuntimeCheckStatus,
} from './runtime-check-machine';
import { useGatewayStore } from '@/stores/gateway';
import { useSettingsStore } from '@/stores/settings';
import { useTranslation } from 'react-i18next';
import { SUPPORTED_LANGUAGES } from '@/i18n';
import { toast } from 'sonner';
import type { GatewayStatus } from '@/types/gateway';
interface SetupStep {
  id: string;
  title: string;
  description: string;
}

const STEP = {
  WELCOME: 0,
  RUNTIME: 1,
  PROVIDER: 2,
  INSTALLING: 3,
  COMPLETE: 4,
} as const;

const steps: SetupStep[] = [
  {
    id: 'welcome',
    title: 'Welcome to Clawy',
    description: 'Your AI assistant is ready to be configured',
  },
  {
    id: 'runtime',
    title: 'Environment Check',
    description: 'Verifying system requirements',
  },
  {
    id: 'provider',
    title: 'AI Provider',
    description: 'Configure your AI service',
  },
  {
    id: 'installing',
    title: 'Setting Up',
    description: 'Installing essential components',
  },
  {
    id: 'complete',
    title: 'All Set!',
    description: 'Clawy is ready to use',
  },
];

// Default skills to auto-install (no additional API keys required)
interface DefaultSkill {
  id: string;
  name: string;
  description: string;
}

const defaultSkills: DefaultSkill[] = [
  { id: 'opencode', name: 'OpenCode', description: 'AI coding assistant backend' },
  { id: 'python-env', name: 'Python Environment', description: 'Python runtime for skills' },
  { id: 'code-assist', name: 'Code Assist', description: 'Code analysis and suggestions' },
  { id: 'file-tools', name: 'File Tools', description: 'File operations and management' },
  { id: 'terminal', name: 'Terminal', description: 'Shell command execution' },
];

import {
  defaultAuthModeForProvider,
  SETUP_PROVIDERS,
  isProviderAuthModeAvailable,
  resolveProviderTypeForAuth,
  type ProviderAuthMode,
  type ProviderTypeInfo,
  getProviderIconUrl,
  resolveProviderApiKeyForSave,
  resolveProviderModelForSave,
  shouldHideProviderTypeInPicker,
  shouldInvertInDark,
  shouldShowProviderModelId,
} from '@/lib/providers';
import clawyIcon from '@/assets/logo.svg';

// Use the shared provider registry for setup providers
const providers = SETUP_PROVIDERS;

// NOTE: Channel types moved to Settings > Channels page
// NOTE: Skill bundles moved to Settings > Skills page - auto-install essential skills during setup

export function Setup() {
  const { t } = useTranslation(['setup', 'channels']);
  const navigate = useNavigate();
  const [currentStep, setCurrentStep] = useState<number>(STEP.WELCOME);

  // Setup state
  const [selectedProvider, setSelectedProvider] = useState<string | null>(null);
  const [providerConfigured, setProviderConfigured] = useState(false);
  const [apiKey, setApiKey] = useState('');
  // Installation state for the Installing step
  const [installedSkills, setInstalledSkills] = useState<string[]>([]);
  // Runtime check status
  const [runtimeChecksPassed, setRuntimeChecksPassed] = useState(false);

  const safeStepIndex = Number.isInteger(currentStep)
    ? Math.min(Math.max(currentStep, STEP.WELCOME), steps.length - 1)
    : STEP.WELCOME;
  const step = steps[safeStepIndex] ?? steps[STEP.WELCOME];
  const isFirstStep = safeStepIndex === STEP.WELCOME;
  const isLastStep = safeStepIndex === steps.length - 1;

  const markSetupComplete = useSettingsStore((state) => state.markSetupComplete);

  // Derive canProceed based on current step - computed directly to avoid useEffect
  const canProceed = useMemo(() => {
    switch (safeStepIndex) {
      case STEP.WELCOME:
        return true;
      case STEP.RUNTIME:
        return runtimeChecksPassed;
      case STEP.PROVIDER:
        return providerConfigured;
      case STEP.INSTALLING:
        return false; // Cannot manually proceed, auto-proceeds when done
      case STEP.COMPLETE:
        return true;
      default:
        return true;
    }
  }, [safeStepIndex, providerConfigured, runtimeChecksPassed]);

  const handleNext = async () => {
    if (isLastStep) {
      // Complete setup
      markSetupComplete();
      toast.success(t('complete.title'));
      navigate('/');
    } else {
      setCurrentStep((i) => i + 1);
    }
  };

  const handleBack = () => {
    setCurrentStep((i) => Math.max(i - 1, 0));
  };

  const handleSkip = () => {
    markSetupComplete();
    navigate('/');
  };

  // Auto-proceed when installation is complete
  const handleInstallationComplete = useCallback((skills: string[]) => {
    setInstalledSkills(skills);
    // Auto-proceed to next step after a short delay
    setTimeout(() => {
      setCurrentStep((i) => i + 1);
    }, 1000);
  }, []);


  return (
    <div className="flex h-screen flex-col overflow-hidden bg-background text-foreground">
      <TitleBar />
      <div className="flex-1 overflow-auto">
        {/* Progress Indicator */}
        <div className="flex justify-center pt-8">
          <div className="flex items-center gap-2">
            {steps.map((s, i) => (
              <div key={s.id} className="flex items-center">
                <div
                  className={cn(
                    'flex h-8 w-8 items-center justify-center rounded-full border-2 transition-colors',
                    i < safeStepIndex
                      ? 'border-primary bg-primary text-primary-foreground'
                      : i === safeStepIndex
                        ? 'border-primary text-primary'
                        : 'border-slate-600 text-slate-600'
                  )}
                >
                  {i < safeStepIndex ? (
                    <Check className="h-4 w-4" />
                  ) : (
                    <span className="text-sm">{i + 1}</span>
                  )}
                </div>
                {i < steps.length - 1 && (
                  <div
                    className={cn(
                      'h-0.5 w-8 transition-colors',
                      i < safeStepIndex ? 'bg-primary' : 'bg-slate-600'
                    )}
                  />
                )}
              </div>
            ))}
          </div>
        </div>

        {/* Step Content */}
        <AnimatePresence mode="wait">
          <motion.div
            key={step.id}
            initial={{ opacity: 0, x: 20 }}
            animate={{ opacity: 1, x: 0 }}
            exit={{ opacity: 0, x: -20 }}
            className="mx-auto max-w-2xl p-8"
          >
            <div className="text-center mb-8">
              <h1 className="text-3xl font-bold mb-2">{t(`steps.${step.id}.title`)}</h1>
              <p className="text-slate-400">{t(`steps.${step.id}.description`)}</p>
            </div>

            {/* Step-specific content */}
            <div className="rounded-xl bg-card text-card-foreground border shadow-sm p-8 mb-8">
              {safeStepIndex === STEP.WELCOME && <WelcomeContent />}
              {safeStepIndex === STEP.RUNTIME && <RuntimeContent onStatusChange={setRuntimeChecksPassed} />}
              {safeStepIndex === STEP.PROVIDER && (
                <ProviderContent
                  providers={providers}
                  selectedProvider={selectedProvider}
                  onSelectProvider={setSelectedProvider}
                  apiKey={apiKey}
                  onApiKeyChange={setApiKey}
                  onConfiguredChange={setProviderConfigured}
                />
              )}
              {safeStepIndex === STEP.INSTALLING && (
                <InstallingContent
                  skills={defaultSkills}
                  onComplete={handleInstallationComplete}
                  onSkip={() => setCurrentStep((i) => i + 1)}
                />
              )}
              {safeStepIndex === STEP.COMPLETE && (
                <CompleteContent
                  selectedProvider={selectedProvider}
                  installedSkills={installedSkills}
                />
              )}
            </div>

            {/* Navigation - hidden during installation step */}
            {safeStepIndex !== STEP.INSTALLING && (
              <div className="flex justify-between">
                <div>
                  {!isFirstStep && (
                    <Button variant="ghost" onClick={handleBack}>
                      <ChevronLeft className="h-4 w-4 mr-2" />
                      {t('nav.back')}
                    </Button>
                  )}
                </div>
                <div className="flex gap-2">
                  {!isLastStep && safeStepIndex !== STEP.RUNTIME && (
                    <Button variant="ghost" onClick={handleSkip}>
                      {t('nav.skipSetup')}
                    </Button>
                  )}
                  <Button onClick={handleNext} disabled={!canProceed}>
                    {isLastStep ? (
                      t('nav.getStarted')
                    ) : (
                      <>
                        {t('nav.next')}
                        <ChevronRight className="h-4 w-4 ml-2" />
                      </>
                    )}
                  </Button>
                </div>
              </div>
            )}
          </motion.div>
        </AnimatePresence>
      </div>
    </div>
  );
}

// ==================== Step Content Components ====================

function WelcomeContent() {
  const { t } = useTranslation(['setup', 'settings']);
  const { language, setLanguage } = useSettingsStore();

  return (
    <div className="text-center space-y-4">
      <div className="mb-4 flex justify-center">
        <img src={clawyIcon} alt="Clawy" className="h-16 w-16" />
      </div>
      <h2 className="text-xl font-semibold">{t('welcome.title')}</h2>
      <p className="text-muted-foreground">
        {t('welcome.description')}
      </p>

      {/* Language Selector */}
      <div className="flex justify-center gap-2 py-2">
        {SUPPORTED_LANGUAGES.map((lang) => (
          <Button
            key={lang.code}
            variant={language === lang.code ? 'secondary' : 'ghost'}
            size="sm"
            onClick={() => setLanguage(lang.code)}
            className="h-7 text-xs"
          >
            {lang.label}
          </Button>
        ))}
      </div>

      <ul className="text-left space-y-2 text-muted-foreground pt-2">
        <li className="flex items-center gap-2">
          <CheckCircle2 className="h-5 w-5 text-green-400" />
          {t('welcome.features.noCommand')}
        </li>
        <li className="flex items-center gap-2">
          <CheckCircle2 className="h-5 w-5 text-green-400" />
          {t('welcome.features.modernUI')}
        </li>
        <li className="flex items-center gap-2">
          <CheckCircle2 className="h-5 w-5 text-green-400" />
          {t('welcome.features.bundles')}
        </li>
        <li className="flex items-center gap-2">
          <CheckCircle2 className="h-5 w-5 text-green-400" />
          {t('welcome.features.crossPlatform')}
        </li>
      </ul>
    </div>
  );
}

interface RuntimeContentProps {
  onStatusChange: (canProceed: boolean) => void;
}

interface RuntimeStatusPayload {
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
}

interface RuntimeInstallResponse {
  success?: boolean;
  error?: string;
  started?: boolean;
  alreadyRunning?: boolean;
  runtime?: 'nodejs' | 'openclaw';
  version?: string;
  result?: {
    version?: string;
  };
}

interface RuntimeInstallEventPayload {
  runtime?: 'nodejs' | 'openclaw';
  phase?: 'preparing' | 'downloading' | 'installing' | 'verifying' | 'completed' | 'failed';
  status?: 'running' | 'completed' | 'failed';
  percent?: number;
  version?: string;
  detail?: string;
  error?: string;
  progress?: {
    total?: number;
    delta?: number;
    transferred?: number;
    percent?: number;
    bytesPerSecond?: number;
  };
}

interface GatewayDiagnosticView {
  message: string;
  detail?: string;
}
function formatRuntimeSource(source?: 'path' | 'managed' | 'bundled' | 'nodeModules') {
  switch (source) {
    case 'path':
      return 'system Node.js';
    case 'managed':
      return 'managed runtime';
    case 'nodeModules':
      return 'workspace package';
    case 'bundled':
      return 'bundled runtime';
    default:
      return 'runtime';
  }
}

function firstDiagnosticDetail(diagnostics?: Array<{ detail?: string }>) {
  return diagnostics?.find((diagnostic) => diagnostic.detail)?.detail;
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
  const progress = payload.progress;
  if (!progress) return payload.detail;
  if (progress.total && progress.total > 0) {
    const speed = progress.bytesPerSecond ? `${formatBytes(progress.bytesPerSecond)}/s` : undefined;
    return [ `${formatBytes(progress.transferred)} / ${formatBytes(progress.total)}`, speed ].filter(Boolean).join(' • ');
  }
  if (progress.transferred && progress.transferred > 0) {
    const speed = progress.bytesPerSecond ? ` at ${formatBytes(progress.bytesPerSecond)}/s` : '';
    return `${formatBytes(progress.transferred)} downloaded${speed}`;
  }
  return payload.detail;
}

function describeGatewayDiagnostic(
  error: string | undefined,
  status: GatewayStatus,
  t: (key: string, options?: Record<string, unknown>) => string,
): GatewayDiagnosticView {
  const normalized = (error || '').toLowerCase();
  const detail = error?.trim();

  if (!detail) {
    return {
      message: t('runtime.status.gatewayFailed'),
    };
  }

  if (normalized.includes('token mismatch') || normalized.includes('failed authentication attempts')) {
    return {
      message: t('runtime.status.gatewayTokenMismatch'),
      detail: t('runtime.status.gatewayTokenMismatchDetail', {
        port: status.port,
        error: detail,
      }),
    };
  }

  if (normalized.includes('pairing required')) {
    return {
      message: t('runtime.status.gatewayPairingRequired'),
      detail: t('runtime.status.gatewayPairingRequiredDetail', {
        port: status.port,
        error: detail,
      }),
    };
  }

  if (
    normalized.includes('already in use') ||
    normalized.includes('address in use') ||
    normalized.includes('port ') && normalized.includes('in use')
  ) {
    return {
      message: t('runtime.status.gatewayPortInUse', { port: status.port }),
      detail: t('runtime.status.gatewayPortInUseDetail', {
        port: status.port,
        error: detail,
      }),
    };
  }

  if (
    normalized.includes('no compatible node.js runtime available') ||
    normalized.includes('no compatible openclaw runtime available') ||
    normalized.includes('failed to launch openclaw gateway')
  ) {
    return {
      message: t('runtime.status.gatewayRuntimeMissing'),
      detail: t('runtime.status.gatewayRuntimeMissingDetail', {
        error: detail,
      }),
    };
  }

  if (
    normalized.includes('invalid config') ||
    normalized.includes('configuration') ||
    normalized.includes('unable to resolve runtime') ||
    normalized.includes('failed to launch')
  ) {
    return {
      message: t('runtime.status.gatewayConfigFailed'),
      detail: t('runtime.status.gatewayConfigFailedDetail', {
        error: detail,
      }),
    };
  }

  if (normalized.includes('timed out') || normalized.includes('timeout')) {
    return {
      message: t('runtime.status.gatewayTimedOut'),
      detail: t('runtime.status.gatewayTimedOutDetail', {
        error: detail,
      }),
    };
  }

  if (normalized.includes('not reachable') || normalized.includes('socket is not connected')) {
    return {
      message: t('runtime.status.gatewayUnavailable'),
      detail: t('runtime.status.gatewayUnavailableDetail', {
        port: status.port,
        error: detail,
      }),
    };
  }

  return {
    message: t('runtime.status.gatewayFailed'),
    detail,
  };
}

function runtimeCheckBadgeVariant(status: RuntimeCheckStatus) {
  switch (status) {
    case 'checking':
      return 'warning' as const;
    case 'success':
      return 'success' as const;
    case 'error':
      return 'destructive' as const;
    case 'idle':
    default:
      return 'outline' as const;
  }
}

function runtimeCheckStatusIcon(status: RuntimeCheckStatus) {
  switch (status) {
    case 'checking':
      return <Loader2 className="h-4 w-4 animate-spin text-yellow-400" />;
    case 'success':
      return <CheckCircle2 className="h-4 w-4 text-green-400" />;
    case 'error':
      return <XCircle className="h-4 w-4 text-red-400" />;
    case 'idle':
    default:
      return <AlertCircle className="h-4 w-4 text-slate-400" />;
  }
}

interface RuntimeCheckCardProps {
  title: string;
  description: string;
  state: RuntimeCheckState;
  statusLabel: string;
  actionLabel?: string;
  onAction?: () => void;
  actionDisabled?: boolean;
}

function RuntimeCheckCard({
  title,
  description,
  state,
  statusLabel,
  actionLabel,
  onAction,
  actionDisabled,
}: RuntimeCheckCardProps) {
  return (
    <Card
      className={cn(
        'border-border/70 transition-colors',
        state.status === 'success' && 'border-green-500/40 bg-green-500/5',
        state.status === 'error' && 'border-red-500/40 bg-red-500/5',
        state.status === 'checking' && 'border-yellow-500/40 bg-yellow-500/5',
      )}
    >
      <CardHeader className="pb-4">
        <div className="flex items-start justify-between gap-4">
          <div className="space-y-1">
            <CardTitle className="text-base">{title}</CardTitle>
            <CardDescription>{description}</CardDescription>
          </div>
          <Badge variant={runtimeCheckBadgeVariant(state.status)}>{statusLabel}</Badge>
        </div>
      </CardHeader>
      <CardContent className="space-y-4">
        <div className="flex items-start gap-3 rounded-lg bg-muted/50 p-3">
          <div className="mt-0.5">{runtimeCheckStatusIcon(state.status)}</div>
          <div className="min-w-0 space-y-1">
            <p className="text-sm font-medium leading-6 break-words">{state.message}</p>
            {state.detail && (
              <p className="text-sm text-muted-foreground break-words">{state.detail}</p>
            )}
          </div>
        </div>

        {state.path && (
          <div className="rounded-md border border-border/60 bg-background/70 px-3 py-2 font-mono text-xs text-muted-foreground break-all">
            {state.path}
          </div>
        )}

        {state.progress && (
          <div className="space-y-2">
            <div className="flex items-center justify-between text-xs text-muted-foreground">
              <span>{Math.round(state.progress.percent)}%</span>
              {state.progress.bytesPerSecond ? (
                <span>{formatBytes(state.progress.bytesPerSecond)}/s</span>
              ) : null}
            </div>
            <Progress value={state.progress.percent} className="h-2" />
            {(state.progress.total || state.progress.transferred) ? (
              <p className="text-xs text-muted-foreground">
                {formatBytes(state.progress.transferred)} / {formatBytes(state.progress.total)}
              </p>
            ) : null}
          </div>
        )}

        {actionLabel && onAction && (
          <div className="flex justify-end">
            <Button
              variant={state.status === 'error' ? 'outline' : 'ghost'}
              size="sm"
              onClick={onAction}
              disabled={actionDisabled}
            >
              {actionLabel}
            </Button>
          </div>
        )}
      </CardContent>
    </Card>
  );
}

function RuntimeContent({ onStatusChange }: RuntimeContentProps) {
  const { t } = useTranslation('setup');
  const gatewayStatus = useGatewayStore((state) => state.status);
  const gatewayLastError = useGatewayStore((state) => state.lastError);
  const startGateway = useGatewayStore((state) => state.start);
  const restartGateway = useGatewayStore((state) => state.restart);
  const checkGatewayHealth = useGatewayStore((state) => state.checkHealth);

  const [checks, dispatchChecks] = useReducer(
    runtimeCheckMachineReducer,
    undefined,
    createRuntimeCheckMachineState
  );
  const [showLogs, setShowLogs] = useState(false);
  const [logContent, setLogContent] = useState('');
  const gatewayTimeoutRef = useRef<number | null>(null);
  const runtimeReady = useMemo(() => runtimePrerequisitesReady(checks), [checks]);
  const allChecksPassed = useMemo(() => runtimeChecksReady(checks), [checks]);

  const runtimeInstallMessage = useCallback((payload: RuntimeInstallEventPayload) => {
    const phase = payload.phase ?? 'installing';
    return t(`runtime.phases.${phase}`, { defaultValue: payload.detail || phase });
  }, [t]);

  const evaluateGatewayAvailability = useCallback(async (runtimePrereqsReady: boolean) => {
    const currentGateway = useGatewayStore.getState().status;

    if (!runtimePrereqsReady && currentGateway.state !== 'running') {
      dispatchChecks({
        type: 'set',
        key: 'gateway',
        patch: {
          status: 'idle',
          message: t('runtime.status.waitingForDependencies'),
        },
      });
      return;
    }

    if (currentGateway.state === 'starting') {
      dispatchChecks({
        type: 'set',
        key: 'gateway',
        patch: {
          status: 'checking',
          message: t('runtime.status.gatewayStarting'),
        },
      });
      return;
    }

    if (currentGateway.state === 'reconnecting') {
      dispatchChecks({
        type: 'set',
        key: 'gateway',
        patch: {
          status: 'checking',
          message: t('runtime.status.gatewayReconnecting'),
        },
      });
      return;
    }

    if (currentGateway.state === 'stopped') {
      dispatchChecks({
        type: 'set',
        key: 'gateway',
        patch: {
          status: 'idle',
          message: t('runtime.status.gatewayStopped'),
          detail: t('runtime.status.gatewayStoppedDetail'),
        },
      });
      return;
    }

    try {
      const health = await checkGatewayHealth();
      if (health.ok) {
        dispatchChecks({
          type: 'set',
          key: 'gateway',
          patch: {
            status: 'success',
            message: t('runtime.status.gatewayRunning', { port: currentGateway.port }),
            detail: t('runtime.status.gatewayRunningDetail', {
              port: currentGateway.port,
              uptime: health.uptime ? Math.round(health.uptime / 1000) : 0,
            }),
          },
        });
        return;
      }

      const diagnostic = describeGatewayDiagnostic(health.error || currentGateway.error || undefined, currentGateway, t);
      dispatchChecks({
        type: 'set',
        key: 'gateway',
        patch: {
          status: 'error',
          message: diagnostic.message,
          detail: diagnostic.detail,
        },
      });
      return;
    } catch {
      const diagnostic = describeGatewayDiagnostic(currentGateway.error || gatewayLastError || undefined, currentGateway, t);
      dispatchChecks({
        type: 'set',
        key: 'gateway',
        patch: {
          status: 'error',
          message: diagnostic.message,
          detail: diagnostic.detail,
        },
      });
    }
  }, [checkGatewayHealth, gatewayLastError, t]);

  const runChecks = useCallback(async () => {
    dispatchChecks({
      type: 'merge',
      updates: {
        nodejs: { status: 'checking', message: t('runtime.status.checking') },
        openclaw: { status: 'checking', message: t('runtime.status.checking'), path: undefined },
        gateway: { status: 'idle', message: t('runtime.status.waitingForDependencies') },
      },
    });

    try {
      const runtimeStatus = await desktopApi.ipcRenderer.invoke('runtime:status') as RuntimeStatusPayload;
      const nodeReady = Boolean(runtimeStatus.node.path);
      const openclawReady = Boolean(runtimeStatus.openclaw.dir);
      const nextRuntimeReady = nodeReady && openclawReady;

      dispatchChecks({
        type: 'merge',
        updates: {
          nodejs: {
            status: nodeReady ? 'success' : 'error',
            message: nodeReady
              ? `Node.js ready via ${formatRuntimeSource(runtimeStatus.node.source)}${runtimeStatus.node.version ? ` v${runtimeStatus.node.version}` : ''}`
              : firstDiagnosticDetail(runtimeStatus.node.diagnostics) || t('runtime.status.nodeMissing'),
          },
          openclaw: {
            status: openclawReady ? 'success' : 'error',
            message: openclawReady
              ? `OpenClaw ready via ${formatRuntimeSource(runtimeStatus.openclaw.source)}${runtimeStatus.openclaw.version ? ` v${runtimeStatus.openclaw.version}` : ''}`
              : firstDiagnosticDetail(runtimeStatus.openclaw.diagnostics) || t('runtime.status.openclawMissing'),
            path: runtimeStatus.openclaw.dir || undefined,
          },
          gateway: nextRuntimeReady
            ? { status: 'checking', message: t('runtime.status.checkingGateway'), detail: t('runtime.status.gatewayAutoStartDetail') }
            : { status: 'idle', message: t('runtime.status.waitingForDependencies') },
        },
      });

      if (nextRuntimeReady) {
        const latestGateway = useGatewayStore.getState().status;
        if (latestGateway.state === 'stopped') {
          dispatchChecks({
            type: 'set',
            key: 'gateway',
            patch: {
              status: 'checking',
              message: t('runtime.status.gatewayStarting'),
              detail: t('runtime.status.gatewayAutoStartDetail'),
            },
          });
          await startGateway();
        } else if (latestGateway.state === 'error') {
          dispatchChecks({
            type: 'set',
            key: 'gateway',
            patch: {
              status: 'checking',
              message: t('runtime.status.gatewayRechecking'),
              detail: t('runtime.status.gatewayAutoRestartDetail'),
            },
          });
          await restartGateway();
        }
      }

      await evaluateGatewayAvailability(nextRuntimeReady);
    } catch (error) {
      const message = error instanceof Error ? error.message : String(error);
      dispatchChecks({
        type: 'merge',
        updates: {
          nodejs: {
            status: 'error',
            message: t('runtime.status.checkFailed', { error: message }),
          },
          openclaw: {
            status: 'error',
            message: t('runtime.status.checkFailed', { error: message }),
          },
          gateway: {
            status: 'idle',
            message: t('runtime.status.waitingForDependencies'),
          },
        },
      });
    }
  }, [evaluateGatewayAvailability, restartGateway, startGateway, t]);

  useEffect(() => {
    void runChecks();
  }, [runChecks]);

  useEffect(() => {
    const unlisten = desktopApi.ipcRenderer.on('runtime:install-progress', (payload) => {
      const event = payload as RuntimeInstallEventPayload;
      const key = event.runtime;
      if (key !== 'nodejs' && key !== 'openclaw') {
        return;
      }

      const progress = typeof event.percent === 'number'
        ? {
            phase: event.phase,
            percent: event.percent,
            transferred: event.progress?.transferred,
            total: event.progress?.total,
            bytesPerSecond: event.progress?.bytesPerSecond,
          }
        : undefined;

      if (event.status === 'failed') {
        dispatchChecks({
          type: 'set',
          key,
          patch: {
            status: 'error',
            message: event.error || runtimeInstallMessage(event),
            detail: describeInstallProgress(event),
            progress: undefined,
          },
        });
        return;
      }

      if (event.status === 'completed') {
        dispatchChecks({
          type: 'set',
          key,
          patch: {
            status: 'checking',
            message: runtimeInstallMessage(event),
            detail: describeInstallProgress(event),
            progress,
          },
        });
        if (key === 'nodejs') {
          toast.success(t('runtime.toast.nodeInstalled', { version: event.version || '' }));
        } else {
          toast.success(t('runtime.toast.openclawInstalled', { version: event.version || '' }));
        }
        void runChecks();
        return;
      }

      dispatchChecks({
        type: 'set',
        key,
        patch: {
          status: 'checking',
          message: runtimeInstallMessage(event),
          detail: describeInstallProgress(event),
          progress,
        },
      });
    });

    return () => {
      if (typeof unlisten === 'function') {
        unlisten();
      }
    };
  }, [runChecks, runtimeInstallMessage, t]);

  useEffect(() => {
    onStatusChange(allChecksPassed);
  }, [allChecksPassed, onStatusChange]);

  useEffect(() => {
    void evaluateGatewayAvailability(runtimeReady);
  }, [
    evaluateGatewayAvailability,
    gatewayStatus.error,
    gatewayStatus.port,
    gatewayStatus.state,
    runtimeReady,
  ]);

  useEffect(() => {
    if (checks.checks.gateway.status !== 'checking' || !runtimeReady) {
      return;
    }

    let cancelled = false;
    const poll = async () => {
      if (cancelled) return;
      await evaluateGatewayAvailability(true);
    };

    void poll();
    const intervalId = window.setInterval(() => {
      void poll();
    }, 1500);

    return () => {
      cancelled = true;
      window.clearInterval(intervalId);
    };
  }, [checks.checks.gateway.status, evaluateGatewayAvailability, runtimeReady]);

  useEffect(() => {
    if (gatewayTimeoutRef.current) {
      window.clearTimeout(gatewayTimeoutRef.current);
      gatewayTimeoutRef.current = null;
    }

    if (!runtimeReady || (gatewayStatus.state !== 'starting' && gatewayStatus.state !== 'reconnecting')) {
      return;
    }

    gatewayTimeoutRef.current = window.setTimeout(() => {
      dispatchChecks({
        type: 'set',
        key: 'gateway',
        patch: {
          status: 'error',
          message: t('runtime.status.gatewayTimedOut'),
          detail: t('runtime.status.gatewayTimedOutDetail'),
        },
      });
    }, 600 * 1000);

    return () => {
      if (gatewayTimeoutRef.current) {
        window.clearTimeout(gatewayTimeoutRef.current);
        gatewayTimeoutRef.current = null;
      }
    };
  }, [gatewayStatus.state, runtimeReady, t]);
  const handleInstallNode = async () => {
    dispatchChecks({
      type: 'set',
      key: 'nodejs',
      patch: {
        status: 'checking',
        message: t('runtime.status.installingNode'),
      },
    });

    try {
      const response = await desktopApi.ipcRenderer.invoke('runtime:installRecommendedNode') as RuntimeInstallResponse;
      if (response.success === false) {
        throw new Error(response.error || t('runtime.status.nodeInstallFailed'));
      }
      if (response.started === false && response.alreadyRunning) {
        toast.info(t('runtime.status.installingNode'));
      }
    } catch (error) {
      dispatchChecks({
        type: 'set',
        key: 'nodejs',
        patch: {
          status: 'error',
          message: error instanceof Error ? error.message : t('runtime.status.nodeInstallFailed'),
        },
      });
    }
  };

  const handleInstallOpenClaw = async () => {
    dispatchChecks({
      type: 'set',
      key: 'openclaw',
      patch: {
        status: 'checking',
        message: t('runtime.status.installingOpenClaw'),
        path: undefined,
      },
    });

    try {
      const response = await desktopApi.ipcRenderer.invoke('runtime:installRecommendedOpenClaw') as RuntimeInstallResponse;
      if (response.success === false) {
        throw new Error(response.error || t('runtime.status.openclawInstallFailed'));
      }
      if (response.started === false && response.alreadyRunning) {
        toast.info(t('runtime.status.installingOpenClaw'));
      }
    } catch (error) {
      dispatchChecks({
        type: 'set',
        key: 'openclaw',
        patch: {
          status: 'error',
          message: error instanceof Error ? error.message : t('runtime.status.openclawInstallFailed'),
          path: undefined,
        },
      });
    }
  };

  const handleStartGateway = async () => {
    dispatchChecks({
      type: 'set',
      key: 'gateway',
      patch: {
        status: 'checking',
        message: gatewayStatus.state === 'error' ? t('runtime.status.gatewayRechecking') : t('runtime.status.gatewayStarting'),
        detail: gatewayStatus.state === 'error'
          ? t('runtime.status.gatewayAutoRestartDetail')
          : t('runtime.status.gatewayAutoStartDetail'),
      },
    });

    try {
      if (gatewayStatus.state === 'error') {
        await restartGateway();
      } else {
        await startGateway();
      }
      await evaluateGatewayAvailability(true);
    } catch (error) {
      const diagnostic = describeGatewayDiagnostic(
        error instanceof Error ? error.message : String(error),
        useGatewayStore.getState().status,
        t,
      );
      dispatchChecks({
        type: 'set',
        key: 'gateway',
        patch: {
          status: 'error',
          message: diagnostic.message,
          detail: diagnostic.detail,
        },
      });
    }
  };

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

  const checkValues = useMemo(() => Object.values(checks.checks), [checks.checks]);
  const hasError = checkValues.some((check) => check.status === 'error');
  const hasChecking = checkValues.some((check) => check.status === 'checking');
  const nodeActionLabel = checks.checks.nodejs.status === 'error'
    ? t('runtime.installNode')
    : undefined;
  const nodeAction = checks.checks.nodejs.status === 'error'
    ? handleInstallNode
    : undefined;
  const openclawActionLabel = checks.checks.openclaw.status === 'error'
    ? t('runtime.installOpenClaw')
    : undefined;
  const openclawAction = checks.checks.openclaw.status === 'error'
    ? handleInstallOpenClaw
    : undefined;
  const gatewayActionLabel = runtimeReady && (checks.checks.gateway.status === 'error' || checks.checks.gateway.status === 'idle')
    ? gatewayStatus.state === 'error'
      ? t('runtime.restartGateway')
      : t('runtime.startGateway')
    : undefined;
  const gatewayAction = runtimeReady && (checks.checks.gateway.status === 'error' || checks.checks.gateway.status === 'idle')
    ? handleStartGateway
    : undefined;

  return (
    <div className="space-y-4">
      <div className="flex items-center justify-between mb-4">
        <h2 className="text-xl font-semibold">{t('runtime.title')}</h2>
        <div className="flex gap-2">
          <Button variant="ghost" size="sm" onClick={handleShowLogs}>
            {t('runtime.viewLogs')}
          </Button>
          <Button variant="ghost" size="sm" onClick={() => void runChecks()}>
            <RefreshCw className="h-4 w-4 mr-2" />
            {t('runtime.recheck')}
          </Button>
        </div>
      </div>
      <div className="grid gap-4">
        <RuntimeCheckCard
          title={t('runtime.nodejs')}
          description={t('runtime.cards.node.description')}
          state={checks.checks.nodejs}
          statusLabel={t(`runtime.states.${checks.checks.nodejs.status}`)}
          actionLabel={nodeActionLabel}
          onAction={nodeAction}
          actionDisabled={checks.checks.nodejs.status === 'checking'}
        />
        <RuntimeCheckCard
          title={t('runtime.openclaw')}
          description={t('runtime.cards.openclaw.description')}
          state={checks.checks.openclaw}
          statusLabel={t(`runtime.states.${checks.checks.openclaw.status}`)}
          actionLabel={openclawActionLabel}
          onAction={openclawAction}
          actionDisabled={checks.checks.openclaw.status === 'checking'}
        />
        <RuntimeCheckCard
          title={t('runtime.gateway')}
          description={t('runtime.cards.gateway.description')}
          state={checks.checks.gateway}
          statusLabel={t(`runtime.states.${checks.checks.gateway.status}`)}
          actionLabel={gatewayActionLabel}
          onAction={gatewayAction}
          actionDisabled={checks.checks.gateway.status === 'checking'}
        />
      </div>

      {!allChecksPassed && (
        <div
          className={cn(
            'mt-4 rounded-lg border p-4',
            hasError && 'border-red-500/20 bg-red-900/20',
            !hasError && 'border-yellow-500/20 bg-yellow-900/20',
          )}
        >
          <div className="flex items-start gap-2">
            {hasError ? (
              <AlertCircle className="mt-0.5 h-5 w-5 text-red-400" />
            ) : (
              <Loader2 className="mt-0.5 h-5 w-5 animate-spin text-yellow-400" />
            )}
            <div>
              <p className={cn('font-medium', hasError ? 'text-red-400' : 'text-yellow-400')}>
                {hasError ? t('runtime.issue.title') : t('runtime.summary.pendingTitle')}
              </p>
              <p className="text-sm text-muted-foreground mt-1">
                {hasError
                  ? t('runtime.issue.desc')
                  : hasChecking
                    ? t('runtime.summary.pending')
                    : t('runtime.summary.readyWhenAllPass')}
              </p>
            </div>
          </div>
        </div>
      )}

      {/* Log viewer panel */}
      {showLogs && (
        <div className="mt-4 p-4 rounded-lg bg-black/40 border border-border">
          <div className="flex items-center justify-between mb-2">
            <p className="font-medium text-foreground text-sm">{t('runtime.logs.title')}</p>
            <div className="flex gap-2">
              <Button variant="ghost" size="sm" className="h-7 text-xs" onClick={handleOpenLogDir}>
                <ExternalLink className="h-3 w-3 mr-1" />
                {t('runtime.logs.openFolder')}
              </Button>
              <Button variant="ghost" size="sm" className="h-7 text-xs" onClick={() => setShowLogs(false)}>
                {t('runtime.logs.close')}
              </Button>
            </div>
          </div>
          <pre className="text-xs text-slate-300 bg-black/50 p-3 rounded max-h-60 overflow-auto whitespace-pre-wrap font-mono">
            {logContent || t('runtime.logs.noLogs')}
          </pre>
        </div>
      )}
    </div>
  );
}

interface ProviderContentProps {
  providers: ProviderTypeInfo[];
  selectedProvider: string | null;
  onSelectProvider: (id: string | null) => void;
  apiKey: string;
  onApiKeyChange: (key: string) => void;
  onConfiguredChange: (configured: boolean) => void;
}

function ProviderContent({
  providers,
  selectedProvider,
  onSelectProvider,
  apiKey,
  onApiKeyChange,
  onConfiguredChange,
}: ProviderContentProps) {
  const { t } = useTranslation(['setup', 'settings']);
  const devModeUnlocked = useSettingsStore((state) => state.devModeUnlocked);
  const [showKey, setShowKey] = useState(false);
  const [validating, setValidating] = useState(false);
  const [keyValid, setKeyValid] = useState<boolean | null>(null);
  const [selectedProviderConfigId, setSelectedProviderConfigId] = useState<string | null>(null);
  const [baseUrl, setBaseUrl] = useState('');
  const [modelId, setModelId] = useState('');
  const [tokenValue, setTokenValue] = useState('');
  const [providerMenuOpen, setProviderMenuOpen] = useState(false);
  const [configuredTypes, setConfiguredTypes] = useState<Set<string>>(new Set());
  const providerMenuRef = useRef<HTMLDivElement | null>(null);

  const [authMode, setAuthMode] = useState<ProviderAuthMode>('oauth');

  // OAuth Flow State
  const [oauthFlowing, setOauthFlowing] = useState(false);
  const [oauthData, setOauthData] = useState<{
    authKind?: string;
    verificationUri: string;
    userCode?: string | null;
    expiresIn: number;
    instructions?: string | null;
  } | null>(null);
  const [oauthPrompt, setOauthPrompt] = useState<{ message: string; placeholder?: string | null } | null>(null);
  const [oauthPromptInput, setOauthPromptInput] = useState('');
  const [oauthProgressMessage, setOauthProgressMessage] = useState<string | null>(null);
  const [oauthError, setOauthError] = useState<string | null>(null);
  const latestProviderSelectionRef = useRef<{
    selectedProvider: string | null;
    authMode: ProviderAuthMode;
  }>({
    selectedProvider: null,
    authMode: 'oauth',
  });
  const providerSelectionTouchedRef = useRef(false);
  const authModeTouchedRef = useRef(false);
  const effectiveSelectedProviderType = selectedProvider
    ? resolveProviderTypeForAuth(selectedProvider, authMode)
    : null;
  const availableProviders = providers.filter((provider) => {
    if (shouldHideProviderTypeInPicker(provider.id)) {
      return false;
    }
    if (provider.id === 'openai') {
      return !(configuredTypes.has('openai') && configuredTypes.has('openai-codex'));
    }
    return provider.id === 'custom' || !configuredTypes.has(provider.id);
  });

  // Manage OAuth events
  useEffect(() => {
    const handleCode = (data: unknown) => {
      setOauthData(data as {
        authKind?: string;
        verificationUri: string;
        userCode?: string | null;
        expiresIn: number;
        instructions?: string | null;
      });
      setOauthPrompt(null);
      setOauthPromptInput('');
      setOauthError(null);
    };

    const handlePrompt = (data: unknown) => {
      const payload = data as { message: string; placeholder?: string | null };
      setOauthPrompt(payload);
      setOauthProgressMessage(payload.message);
    };

    const handleProgress = (data: unknown) => {
      const payload = data as { message?: string | null };
      setOauthProgressMessage(payload.message || null);
    };

    const handleSuccess = async () => {
      setOauthFlowing(false);
      setOauthData(null);
      setOauthPrompt(null);
      setOauthPromptInput('');
      setOauthProgressMessage(null);
      setKeyValid(true);

      if (effectiveSelectedProviderType) {
        try {
          await desktopApi.ipcRenderer.invoke('provider:setDefault', effectiveSelectedProviderType);
          const refreshedList = await desktopApi.ipcRenderer.invoke('provider:list') as Array<{ type: string }>;
          setConfiguredTypes(new Set(refreshedList.map((item) => item.type)));
        } catch (error) {
          console.error('Failed to set default provider:', error);
        }
      }

      onConfiguredChange(true);
      toast.success(t('provider.valid'));
    };

    const handleError = (data: unknown) => {
      setOauthError((data as { message: string }).message);
      setOauthProgressMessage(null);
    };

    desktopApi.ipcRenderer.on('oauth:code', handleCode);
    desktopApi.ipcRenderer.on('oauth:prompt', handlePrompt);
    desktopApi.ipcRenderer.on('oauth:progress', handleProgress);
    desktopApi.ipcRenderer.on('oauth:success', handleSuccess);
    desktopApi.ipcRenderer.on('oauth:error', handleError);

    return () => {
      // Clean up manually if the API provides removeListener, though `on` in preloads might not return an unsub.
      // Easiest is to just let it be, or if they have `off`:
      if (typeof desktopApi.ipcRenderer.off === 'function') {
        desktopApi.ipcRenderer.off('oauth:code', handleCode);
        desktopApi.ipcRenderer.off('oauth:prompt', handlePrompt);
        desktopApi.ipcRenderer.off('oauth:progress', handleProgress);
        desktopApi.ipcRenderer.off('oauth:success', handleSuccess);
        desktopApi.ipcRenderer.off('oauth:error', handleError);
      }
    };
  }, [onConfiguredChange, t, effectiveSelectedProviderType]);

  useEffect(() => {
    latestProviderSelectionRef.current = {
      selectedProvider,
      authMode,
    };
  }, [authMode, selectedProvider]);

  const handleStartOAuth = async () => {
    if (!effectiveSelectedProviderType || !selectedProvider) return;

    try {
      const list = await desktopApi.ipcRenderer.invoke('provider:list') as Array<{ type: string }>;
      const existingTypes = new Set(list.map(l => l.type));
      if (selectedProvider === 'minimax-portal' && existingTypes.has('minimax-portal-cn')) {
        toast.error(t('settings:aiProviders.toast.minimaxConflict'));
        return;
      }
      if (selectedProvider === 'minimax-portal-cn' && existingTypes.has('minimax-portal')) {
        toast.error(t('settings:aiProviders.toast.minimaxConflict'));
        return;
      }
      if (!isProviderAuthModeAvailable(selectedProvider, 'oauth', existingTypes)) {
        toast.error(t('settings:aiProviders.toast.failedAdd'));
        return;
      }
    } catch {
      // ignore check failure
    }

    setOauthFlowing(true);
    setOauthData(null);
    setOauthPrompt(null);
    setOauthPromptInput('');
    setOauthProgressMessage(null);
    setOauthError(null);

    try {
      await desktopApi.ipcRenderer.invoke('provider:requestOAuth', effectiveSelectedProviderType);
    } catch (e) {
      setOauthError(String(e));
      setOauthFlowing(false);
    }
  };

  const handleCancelOAuth = async () => {
    setOauthFlowing(false);
    setOauthData(null);
    setOauthPrompt(null);
    setOauthPromptInput('');
    setOauthProgressMessage(null);
    setOauthError(null);
    await desktopApi.ipcRenderer.invoke('provider:cancelOAuth');
  };

  const handleSubmitOAuthPrompt = async () => {
    if (!oauthPromptInput.trim()) return;

    try {
      await desktopApi.ipcRenderer.invoke('provider:submitOAuthInput', oauthPromptInput.trim());
      setOauthPromptInput('');
      setOauthProgressMessage(t('settings:aiProviders.oauth.waitingApproval'));
    } catch (error) {
      setOauthError(String(error));
    }
  };

  // On mount, try to restore previously configured provider
  useEffect(() => {
    let cancelled = false;
    (async () => {
      try {
        const list = await desktopApi.ipcRenderer.invoke('provider:list') as Array<{ id: string; type: string; hasKey: boolean }>;
        const defaultId = await desktopApi.ipcRenderer.invoke('provider:getDefault') as string | null;
        const nextConfiguredTypes = new Set(list.map((item) => item.type));
        setConfiguredTypes(nextConfiguredTypes);
        const setupProviderTypes = new Set<string>(providers.map((p) => p.id));
        const setupCandidates = list.filter((p) => setupProviderTypes.has(p.type) || p.type === 'openai-codex');
        const preferred =
          (defaultId && setupCandidates.find((p) => p.id === defaultId))
          || setupCandidates.find((p) => p.hasKey)
          || setupCandidates[0];
        if (preferred && !cancelled) {
          const latestSelection = latestProviderSelectionRef.current;
          if (providerSelectionTouchedRef.current && latestSelection.selectedProvider) {
            return;
          }
          const visibleType = preferred.type === 'openai-codex' ? 'openai' : preferred.type;
          onSelectProvider(visibleType);
          const savedProvider = await desktopApi.ipcRenderer.invoke(
            'provider:get',
            preferred.id
          ) as { authMode?: ProviderAuthMode | null } | null;
          if (!authModeTouchedRef.current) {
            setAuthMode(defaultAuthModeForProvider(
              visibleType,
              nextConfiguredTypes,
              providers.find((p) => p.id === visibleType),
              savedProvider?.authMode ?? null
            ));
          }
          setSelectedProviderConfigId(preferred.id);
          const typeInfo = providers.find((p) => p.id === visibleType);
          const providerRequiresKey = visibleType === 'openai'
            ? preferred.type === 'openai'
            : (typeInfo?.requiresApiKey ?? false);
          onConfiguredChange(!providerRequiresKey || preferred.hasKey || preferred.type === 'openai-codex');
          const storedKey = await desktopApi.ipcRenderer.invoke('provider:getApiKey', preferred.id) as string | null;
          if (storedKey) {
            onApiKeyChange(storedKey);
          }
        } else if (!cancelled) {
          onConfiguredChange(false);
        }
      } catch (error) {
        if (!cancelled) {
          console.error('Failed to load provider list:', error);
        }
      }
    })();
    return () => { cancelled = true; };
  }, [onApiKeyChange, onConfiguredChange, onSelectProvider, providers]);

  // When provider changes, load stored key + reset base URL
  useEffect(() => {
    let cancelled = false;
    (async () => {
      if (!effectiveSelectedProviderType || !selectedProvider) return;
      try {
        const list = await desktopApi.ipcRenderer.invoke('provider:list') as Array<{ id: string; type: string; hasKey: boolean }>;
        const defaultId = await desktopApi.ipcRenderer.invoke('provider:getDefault') as string | null;
        setConfiguredTypes(new Set(list.map((item) => item.type)));
        const sameType = list.filter((p) => p.type === effectiveSelectedProviderType);
        const preferredInstance =
          (defaultId && sameType.find((p) => p.id === defaultId))
          || sameType.find((p) => p.hasKey)
          || sameType[0];
        const providerIdForLoad = preferredInstance?.id || effectiveSelectedProviderType;
        setSelectedProviderConfigId(providerIdForLoad);

        const savedProvider = await desktopApi.ipcRenderer.invoke(
          'provider:get',
          providerIdForLoad
        ) as { baseUrl?: string; model?: string; authMode?: ProviderAuthMode | null } | null;
        const storedKey = await desktopApi.ipcRenderer.invoke('provider:getApiKey', providerIdForLoad) as string | null;
        if (!cancelled) {
          const nextExistingTypes = new Set(list.map((item) => item.type));
          const nextAuthMode = savedProvider?.authMode
            ? defaultAuthModeForProvider(
              selectedProvider,
              nextExistingTypes,
              providers.find((p) => p.id === selectedProvider),
              savedProvider.authMode
            )
            : preferredInstance
              ? defaultAuthModeForProvider(
                selectedProvider,
                nextExistingTypes,
                providers.find((p) => p.id === selectedProvider)
              )
              : authMode;
          setAuthMode(nextAuthMode);
          if (storedKey) {
            onApiKeyChange(storedKey);
          } else {
            onApiKeyChange('');
          }
          setTokenValue('');

          const info = providers.find((p) => p.id === selectedProvider);
          setBaseUrl(savedProvider?.baseUrl || info?.defaultBaseUrl || '');
          setModelId(savedProvider?.model || info?.defaultModelId || '');
        }
      } catch (error) {
        if (!cancelled) {
          console.error('Failed to load provider key:', error);
        }
      }
    })();
    return () => { cancelled = true; };
  }, [authMode, effectiveSelectedProviderType, onApiKeyChange, selectedProvider, providers]);

  useEffect(() => {
    if (!providerMenuOpen) return;

    const handlePointerDown = (event: MouseEvent) => {
      if (providerMenuRef.current && !providerMenuRef.current.contains(event.target as Node)) {
        setProviderMenuOpen(false);
      }
    };

    const handleEscape = (event: KeyboardEvent) => {
      if (event.key === 'Escape') {
        setProviderMenuOpen(false);
      }
    };

    document.addEventListener('mousedown', handlePointerDown);
    document.addEventListener('keydown', handleEscape);
    return () => {
      document.removeEventListener('mousedown', handlePointerDown);
      document.removeEventListener('keydown', handleEscape);
    };
  }, [providerMenuOpen]);

  const selectedProviderData = providers.find((p) => p.id === selectedProvider);
  const selectedProviderIconUrl = selectedProviderData
    ? getProviderIconUrl(selectedProviderData.id)
    : undefined;
  const showBaseUrlField = selectedProviderData?.showBaseUrl ?? false;
  const showModelIdField = shouldShowProviderModelId(selectedProviderData, devModeUnlocked);
  const requiresKey = selectedProviderData?.requiresApiKey ?? false;
  const isOAuth = selectedProviderData?.isOAuth ?? false;
  const supportsApiKey = selectedProviderData?.supportsApiKey ?? false;
  const supportsTokenAuth = selectedProviderData?.supportsTokenAuth ?? false;
  const oauthModeAvailable = selectedProvider
    ? isProviderAuthModeAvailable(selectedProvider, 'oauth', configuredTypes)
    : false;
  const apiKeyModeAvailable = selectedProvider
    ? isProviderAuthModeAvailable(selectedProvider, 'apikey', configuredTypes)
    : false;
  const tokenModeAvailable = selectedProvider
    ? isProviderAuthModeAvailable(selectedProvider, 'token', configuredTypes)
    : false;
  const useOAuthFlow = isOAuth && (!supportsApiKey || authMode === 'oauth');
  const useTokenMode = supportsTokenAuth && authMode === 'token';
  const showAuthModeToggle = (isOAuth || supportsTokenAuth) && (supportsApiKey || supportsTokenAuth);

  const handleValidateAndSave = async () => {
    if (!selectedProvider) return;

    try {
      const list = await desktopApi.ipcRenderer.invoke('provider:list') as Array<{ type: string }>;
      const existingTypes = new Set(list.map(l => l.type));
      if (selectedProvider === 'minimax-portal' && existingTypes.has('minimax-portal-cn')) {
        toast.error(t('settings:aiProviders.toast.minimaxConflict'));
        return;
      }
      if (selectedProvider === 'minimax-portal-cn' && existingTypes.has('minimax-portal')) {
        toast.error(t('settings:aiProviders.toast.minimaxConflict'));
        return;
      }
      if (selectedProvider === 'openai' && !apiKeyModeAvailable) {
        toast.error(t('settings:aiProviders.toast.failedAdd'));
        return;
      }
      if (selectedProvider === 'anthropic' && useTokenMode && !tokenModeAvailable) {
        toast.error(t('settings:aiProviders.toast.failedAdd'));
        return;
      }
    } catch {
      // ignore check failure
    }

    setValidating(true);
    setKeyValid(null);

    try {
      // Validate key if the provider requires one and a key was entered
      const isApiKeyRequired = requiresKey || (supportsApiKey && authMode === 'apikey');
      if (isApiKeyRequired && apiKey) {
        const result = await desktopApi.ipcRenderer.invoke(
          'provider:validateKey',
          selectedProviderConfigId || selectedProvider,
          apiKey,
          { baseUrl: baseUrl.trim() || undefined }
        ) as { valid: boolean; error?: string };

        setKeyValid(result.valid);

        if (!result.valid) {
          toast.error(result.error || t('provider.invalid'));
          setValidating(false);
          return;
        }
      } else {
        setKeyValid(true);
      }

      const effectiveModelId = resolveProviderModelForSave(
        selectedProviderData,
        modelId,
        devModeUnlocked
      );

      const providerIdForSave =
        selectedProvider === 'custom'
          ? (selectedProviderConfigId?.startsWith('custom-')
            ? selectedProviderConfigId
            : `custom-${crypto.randomUUID()}`)
          : (effectiveSelectedProviderType || selectedProvider);

      const effectiveApiKey = resolveProviderApiKeyForSave(selectedProvider, apiKey);
      const providerPayload = {
        id: providerIdForSave,
        name: selectedProvider === 'custom' ? t('settings:aiProviders.custom') : (selectedProviderData?.name || selectedProvider),
        type: providerIdForSave,
        authMode,
        baseUrl: baseUrl.trim() || undefined,
        model: effectiveModelId,
        enabled: true,
        createdAt: new Date().toISOString(),
        updatedAt: new Date().toISOString(),
      };

      let saveResult: { success: boolean; error?: string };
      if (useTokenMode) {
        if (!tokenValue.trim()) {
          toast.error(t('settings:aiProviders.oauth.setupTokenRequired'));
          setValidating(false);
          return;
        }

        saveResult = await desktopApi.ipcRenderer.invoke(
          'provider:saveTokenAuth',
          providerPayload,
          tokenValue.trim()
        ) as { success: boolean; error?: string };
      } else {
        saveResult = await desktopApi.ipcRenderer.invoke(
          'provider:save',
          providerPayload,
          effectiveApiKey
        ) as { success: boolean; error?: string };
      }

      if (!saveResult.success) {
        throw new Error(saveResult.error || 'Failed to save provider config');
      }

      const defaultResult = await desktopApi.ipcRenderer.invoke(
        'provider:setDefault',
        providerIdForSave
      ) as { success: boolean; error?: string };

      if (!defaultResult.success) {
        throw new Error(defaultResult.error || 'Failed to set default provider');
      }

      setSelectedProviderConfigId(providerIdForSave);
      setConfiguredTypes((previous) => new Set(previous).add(providerIdForSave));
      onConfiguredChange(true);
      toast.success(t('provider.valid'));
    } catch (error) {
      setKeyValid(false);
      onConfiguredChange(false);
      toast.error('Configuration failed: ' + String(error));
    } finally {
      setValidating(false);
    }
  };

  // Can the user submit?
  const isApiKeyRequired = requiresKey || (supportsApiKey && authMode === 'apikey');
  const canSubmit =
    selectedProvider
    && (useTokenMode ? tokenValue.trim().length > 0 : (isApiKeyRequired ? apiKey.length > 0 : true))
    && (showModelIdField ? modelId.trim().length > 0 : true)
    && !useOAuthFlow;

  const handleSelectProvider = (providerId: string) => {
    providerSelectionTouchedRef.current = true;
    authModeTouchedRef.current = false;
    onSelectProvider(providerId);
    setSelectedProviderConfigId(null);
    onConfiguredChange(false);
    onApiKeyChange('');
    setTokenValue('');
    setKeyValid(null);
    setProviderMenuOpen(false);
    setAuthMode(defaultAuthModeForProvider(providerId, configuredTypes, providers.find((provider) => provider.id === providerId)));
  };

  return (
    <div className="space-y-6">
      {/* Provider selector — dropdown */}
      <div className="space-y-2">
        <Label>{t('provider.label')}</Label>
        <div className="relative" ref={providerMenuRef}>
          <button
            type="button"
            aria-haspopup="listbox"
            aria-expanded={providerMenuOpen}
            onClick={() => setProviderMenuOpen((open) => !open)}
            className={cn(
              'w-full rounded-md border border-input bg-background px-3 py-2 text-sm',
              'flex items-center justify-between gap-2',
              'focus:outline-none focus:ring-2 focus:ring-ring'
            )}
          >
            <div className="flex items-center gap-2 min-w-0">
              {selectedProvider && selectedProviderData ? (
                selectedProviderIconUrl ? (
                  <img
                    src={selectedProviderIconUrl}
                    alt={selectedProviderData.name}
                    className={cn('h-4 w-4 shrink-0', shouldInvertInDark(selectedProviderData.id) && 'dark:invert')}
                  />
                ) : (
                  <span className="text-sm leading-none shrink-0">{selectedProviderData.icon}</span>
                )
              ) : (
                <span className="text-xs text-muted-foreground shrink-0">—</span>
              )}
              <span className={cn('truncate text-left', !selectedProvider && 'text-muted-foreground')}>
                {selectedProviderData
                  ? `${selectedProviderData.id === 'custom' ? t('settings:aiProviders.custom') : selectedProviderData.name}${selectedProviderData.model ? ` — ${selectedProviderData.model}` : ''}`
                  : t('provider.selectPlaceholder')}
              </span>
            </div>
            <ChevronDown className={cn('h-3.5 w-3.5 text-muted-foreground shrink-0 transition-transform', providerMenuOpen && 'rotate-180')} />
          </button>

          {providerMenuOpen && (
            <div
              role="listbox"
              className="absolute z-20 mt-1 w-full rounded-md border border-border bg-popover shadow-md max-h-64 overflow-auto"
            >
              {availableProviders.map((p) => {
                const iconUrl = getProviderIconUrl(p.id);
                const isSelected = selectedProvider === p.id;

                return (
                  <button
                    key={p.id}
                    type="button"
                    role="option"
                    aria-selected={isSelected}
                    onClick={() => handleSelectProvider(p.id)}
                    className={cn(
                      'w-full px-3 py-2 text-left text-sm flex items-center justify-between gap-2',
                      'hover:bg-accent transition-colors',
                      isSelected && 'bg-accent/60'
                    )}
                  >
                    <div className="flex items-center gap-2 min-w-0">
                      {iconUrl ? (
                        <img
                          src={iconUrl}
                          alt={p.name}
                          className={cn('h-4 w-4 shrink-0', shouldInvertInDark(p.id) && 'dark:invert')}
                        />
                      ) : (
                        <span className="text-sm leading-none shrink-0">{p.icon}</span>
                      )}
                      <span className="truncate">{p.id === 'custom' ? t('settings:aiProviders.custom') : p.name}{p.model ? ` — ${p.model}` : ''}</span>
                    </div>
                    {isSelected && <Check className="h-4 w-4 text-primary shrink-0" />}
                  </button>
                );
              })}
            </div>
          )}
        </div>
      </div>

      {/* Dynamic config fields based on selected provider */}
      {selectedProvider && (
        <motion.div
          key={selectedProvider}
          initial={{ opacity: 0, y: 8 }}
          animate={{ opacity: 1, y: 0 }}
          className="space-y-4"
        >
          {/* Base URL field (for siliconflow, ollama, custom) */}
          {showBaseUrlField && (
            <div className="space-y-2">
              <Label htmlFor="baseUrl">{t('provider.baseUrl')}</Label>
              <Input
                id="baseUrl"
                type="text"
                placeholder="https://api.example.com/v1"
                value={baseUrl}
                onChange={(e) => {
                  setBaseUrl(e.target.value);
                  onConfiguredChange(false);
                }}
                autoComplete="off"
                className="bg-background border-input"
              />
            </div>
          )}

          {/* Model ID field (for siliconflow etc.) */}
          {showModelIdField && (
            <div className="space-y-2">
              <Label htmlFor="modelId">{t('provider.modelId')}</Label>
              <Input
                id="modelId"
                type="text"
                placeholder={selectedProviderData?.modelIdPlaceholder || 'e.g. deepseek-ai/DeepSeek-V3'}
                value={modelId}
                onChange={(e) => {
                  setModelId(e.target.value);
                  onConfiguredChange(false);
                }}
                autoComplete="off"
                className="bg-background border-input"
              />
              <p className="text-xs text-muted-foreground">
                {t('provider.modelIdDesc')}
              </p>
            </div>
          )}

          {/* Auth mode toggle for providers supporting both */}
          {showAuthModeToggle && (
            <div className="space-y-2">
              <div className="flex rounded-lg border overflow-hidden text-sm">
                {(isOAuth || supportsTokenAuth) && (
                  <button
                    onClick={() => {
                      if ((supportsTokenAuth && tokenModeAvailable) || (isOAuth && oauthModeAvailable)) {
                        authModeTouchedRef.current = true;
                        setAuthMode(supportsTokenAuth ? 'token' : 'oauth');
                      }
                    }}
                    disabled={supportsTokenAuth ? !tokenModeAvailable : !oauthModeAvailable}
                    className={cn(
                      'flex-1 py-2 px-3 transition-colors',
                      (supportsTokenAuth ? authMode === 'token' : authMode === 'oauth')
                        ? 'bg-primary text-primary-foreground'
                        : 'hover:bg-muted text-muted-foreground',
                      (supportsTokenAuth ? !tokenModeAvailable : !oauthModeAvailable)
                        && 'cursor-not-allowed opacity-50 hover:bg-transparent'
                    )}
                  >
                    {selectedProvider === 'openai'
                      ? t('settings:aiProviders.oauth.codexMode')
                      : supportsTokenAuth
                        ? t('settings:aiProviders.oauth.setupTokenMode')
                        : t('settings:aiProviders.oauth.loginMode')}
                  </button>
                )}
                <button
                  onClick={() => {
                    if (apiKeyModeAvailable) {
                      authModeTouchedRef.current = true;
                      setAuthMode('apikey');
                    }
                  }}
                  disabled={!apiKeyModeAvailable}
                  className={cn(
                    'flex-1 py-2 px-3 transition-colors',
                    authMode === 'apikey' ? 'bg-primary text-primary-foreground' : 'hover:bg-muted text-muted-foreground',
                    !apiKeyModeAvailable && 'cursor-not-allowed opacity-50 hover:bg-transparent'
                  )}
                >
                  {t('settings:aiProviders.oauth.apikeyMode')}
                </button>
              </div>
              {selectedProvider === 'anthropic' && authMode === 'token' && (
                <div className="rounded-md border border-blue-200 bg-blue-50 px-3 py-2 text-sm text-blue-700">
                  {t('settings:aiProviders.oauth.setupTokenHelp')}
                </div>
              )}
            </div>
          )}

          {/* API Key field (hidden for ollama) */}
          {(!isOAuth || (supportsApiKey && authMode === 'apikey')) && (
            <div className="space-y-2">
              <Label htmlFor="apiKey">{t('provider.apiKey')}</Label>
              <div className="relative">
                <Input
                  id="apiKey"
                  type={showKey ? 'text' : 'password'}
                  placeholder={selectedProviderData?.placeholder}
                  value={apiKey}
                  onChange={(e) => {
                    onApiKeyChange(e.target.value);
                    onConfiguredChange(false);
                    setKeyValid(null);
                  }}
                  autoComplete="off"
                  className="pr-10 bg-background border-input"
                />
                <button
                  type="button"
                  onClick={() => setShowKey(!showKey)}
                  className="absolute right-3 top-1/2 -translate-y-1/2 text-muted-foreground hover:text-foreground"
                >
                  {showKey ? <EyeOff className="h-4 w-4" /> : <Eye className="h-4 w-4" />}
                </button>
              </div>
            </div>
          )}

          {useTokenMode && (
            <div className="space-y-2">
              <Label htmlFor="setupToken">{t('settings:aiProviders.oauth.setupTokenLabel')}</Label>
              <div className="relative">
                <Input
                  id="setupToken"
                  type={showKey ? 'text' : 'password'}
                  placeholder={t('settings:aiProviders.oauth.setupTokenPlaceholder')}
                  value={tokenValue}
                  onChange={(e) => {
                    setTokenValue(e.target.value);
                    onConfiguredChange(false);
                    setKeyValid(null);
                  }}
                  autoComplete="off"
                  className="pr-10 bg-background border-input"
                />
                <button
                  type="button"
                  onClick={() => setShowKey(!showKey)}
                  className="absolute right-3 top-1/2 -translate-y-1/2 text-muted-foreground hover:text-foreground"
                >
                  {showKey ? <EyeOff className="h-4 w-4" /> : <Eye className="h-4 w-4" />}
                </button>
              </div>
              <p className="text-xs text-muted-foreground">
                {t('settings:aiProviders.oauth.setupTokenHelp')}
              </p>
            </div>
          )}

          {/* Device OAuth Trigger */}
          {useOAuthFlow && (
            <div className="space-y-4 pt-2">
              <div className="rounded-lg bg-blue-500/10 border border-blue-500/20 p-4 text-center">
                <p className="text-sm text-blue-200 mb-3 block">
                  {selectedProvider === 'openai'
                    ? t('settings:aiProviders.oauth.codexPrompt')
                    : t('settings:aiProviders.oauth.loginPrompt')}
                </p>
                <Button
                  onClick={handleStartOAuth}
                  disabled={oauthFlowing}
                  className="w-full bg-blue-600 hover:bg-blue-700 text-white"
                >
                  {oauthFlowing ? (
                    <><Loader2 className="h-4 w-4 mr-2 animate-spin" /> Waiting...</>
                  ) : (
                    'Login with Browser'
                  )}
                </Button>
              </div>

              {/* OAuth Active State Modal / Inline View */}
              {oauthFlowing && (
                <div className="mt-4 p-4 border rounded-xl bg-card relative overflow-hidden">
                  {/* Background pulse effect */}
                  <div className="absolute inset-0 bg-primary/5 animate-pulse" />

                  <div className="relative z-10 flex flex-col items-center justify-center text-center space-y-4">
                    {oauthError ? (
                      <div className="text-red-400 space-y-2">
                        <XCircle className="h-8 w-8 mx-auto" />
                        <p className="font-medium">Authentication Failed</p>
                        <p className="text-sm opacity-80">{oauthError}</p>
                        <Button variant="outline" size="sm" onClick={handleCancelOAuth} className="mt-2">
                          Try Again
                        </Button>
                      </div>
                    ) : !oauthData ? (
                      <div className="space-y-3 py-4">
                        <Loader2 className="h-8 w-8 animate-spin text-primary mx-auto" />
                        <p className="text-sm text-muted-foreground animate-pulse">
                          {oauthProgressMessage || t('settings:aiProviders.oauth.requestingCode')}
                        </p>
                      </div>
                    ) : oauthData.authKind === 'browser-callback' ? (
                      <div className="space-y-4 w-full">
                        <div className="space-y-1">
                          <h3 className="font-medium text-lg">{t('settings:aiProviders.oauth.browserTitle')}</h3>
                          <div className="text-sm text-muted-foreground text-left mt-2 space-y-1">
                            <p>{oauthData.instructions || t('settings:aiProviders.oauth.browserInstructions')}</p>
                          </div>
                        </div>

                        <Button
                          variant="secondary"
                          className="w-full"
                          onClick={() => desktopApi.ipcRenderer.invoke('shell:openExternal', oauthData.verificationUri)}
                        >
                          <ExternalLink className="h-4 w-4 mr-2" />
                          {t('settings:aiProviders.oauth.openLoginPage')}
                        </Button>

                        <div className="flex items-center justify-center gap-2 text-xs text-muted-foreground pt-1">
                          <Loader2 className="h-3 w-3 animate-spin" />
                          <span>{oauthProgressMessage || t('settings:aiProviders.oauth.waitingApproval')}</span>
                        </div>

                        {oauthPrompt && (
                          <div className="space-y-2 rounded-lg border bg-background/60 p-3 text-left">
                            <p className="text-xs text-muted-foreground">{oauthPrompt.message}</p>
                            <Input
                              value={oauthPromptInput}
                              onChange={(event) => setOauthPromptInput(event.target.value)}
                              placeholder={oauthPrompt.placeholder || t('settings:aiProviders.oauth.manualPromptPlaceholder')}
                            />
                            <Button
                              variant="outline"
                              className="w-full"
                              onClick={handleSubmitOAuthPrompt}
                              disabled={!oauthPromptInput.trim()}
                            >
                              {t('settings:aiProviders.oauth.submitManualCode')}
                            </Button>
                          </div>
                        )}

                        <Button variant="ghost" size="sm" className="w-full mt-2" onClick={handleCancelOAuth}>
                          {t('settings:aiProviders.oauth.cancel')}
                        </Button>
                      </div>
                    ) : (
                      <div className="space-y-4 w-full">
                        <div className="space-y-1">
                          <h3 className="font-medium text-lg">Approve Login</h3>
                          <div className="text-sm text-muted-foreground text-left mt-2 space-y-1">
                            <p>1. Copy the authorization code below.</p>
                            <p>2. Open the login page in your browser.</p>
                            <p>3. Paste the code to approve access.</p>
                          </div>
                        </div>

                        <div className="flex items-center justify-center gap-2 p-3 bg-background border rounded-lg">
                          <code className="text-2xl font-mono tracking-widest font-bold text-primary">
                            {oauthData.userCode}
                          </code>
                          <Button
                            variant="ghost"
                            size="icon"
                            onClick={() => {
                              if (oauthData.userCode) {
                                navigator.clipboard.writeText(oauthData.userCode);
                                toast.success(t('settings:aiProviders.oauth.codeCopied'));
                              }
                            }}
                          >
                            <Copy className="h-4 w-4" />
                          </Button>
                        </div>

                        <Button
                          variant="secondary"
                          className="w-full"
                          onClick={() => desktopApi.ipcRenderer.invoke('shell:openExternal', oauthData.verificationUri)}
                        >
                          <ExternalLink className="h-4 w-4 mr-2" />
                          {t('settings:aiProviders.oauth.openLoginPage')}
                        </Button>

                        <div className="flex items-center justify-center gap-2 text-xs text-muted-foreground pt-2">
                          <Loader2 className="h-3 w-3 animate-spin" />
                          <span>{t('settings:aiProviders.oauth.waitingApproval')}</span>
                        </div>

                        <Button variant="ghost" size="sm" className="w-full mt-2" onClick={handleCancelOAuth}>
                          {t('settings:aiProviders.oauth.cancel')}
                        </Button>
                      </div>
                    )}
                  </div>
                </div>
              )}
            </div>
          )}

          {/* Validate & Save */}
          <Button
            onClick={handleValidateAndSave}
            disabled={!canSubmit || validating}
            className={cn("w-full", useOAuthFlow && "hidden")}
          >
            {validating ? (
              <Loader2 className="h-4 w-4 animate-spin mr-2" />
            ) : null}
            {requiresKey ? t('provider.validateSave') : t('provider.save')}
          </Button>

          {keyValid !== null && (
            <p className={cn('text-sm text-center', keyValid ? 'text-green-400' : 'text-red-400')}>
              {keyValid ? `✓ ${t('provider.valid')}` : `✗ ${t('provider.invalid')}`}
            </p>
          )}

          <p className="text-sm text-muted-foreground text-center">
            {t('provider.storedLocally')}
          </p>
        </motion.div>
      )}
    </div>
  );
}

// NOTE: SkillsContent component removed - auto-install essential skills

// Installation status for each skill
type InstallStatus = 'pending' | 'installing' | 'completed' | 'failed';

interface SkillInstallState {
  id: string;
  name: string;
  description: string;
  status: InstallStatus;
}

interface InstallingContentProps {
  skills: DefaultSkill[];
  onComplete: (installedSkills: string[]) => void;
  onSkip: () => void;
}

function InstallingContent({ skills, onComplete, onSkip }: InstallingContentProps) {
  const { t } = useTranslation('setup');
  const [skillStates, setSkillStates] = useState<SkillInstallState[]>(
    skills.map((s) => ({ ...s, status: 'pending' as InstallStatus }))
  );
  const [overallProgress, setOverallProgress] = useState(0);
  const [errorMessage, setErrorMessage] = useState<string | null>(null);
  const installStarted = useRef(false);

  // Real installation process
  useEffect(() => {
    if (installStarted.current) return;
    installStarted.current = true;

    const runRealInstall = async () => {
      try {
        // Step 1: Initialize all skills to 'installing' state for UI
        setSkillStates(prev => prev.map(s => ({ ...s, status: 'installing' })));
        setOverallProgress(10);

        // Step 2: Call the backend to install uv and setup Python
        const result = await desktopApi.ipcRenderer.invoke('uv:install-all') as {
          success: boolean;
          error?: string
        };

        if (result.success) {
          setSkillStates(prev => prev.map(s => ({ ...s, status: 'completed' })));
          setOverallProgress(100);

          await new Promise((resolve) => setTimeout(resolve, 800));
          onComplete(skills.map(s => s.id));
        } else {
          setSkillStates(prev => prev.map(s => ({ ...s, status: 'failed' })));
          setErrorMessage(result.error || 'Unknown error during installation');
          toast.error('Environment setup failed');
        }
      } catch (err) {
        setSkillStates(prev => prev.map(s => ({ ...s, status: 'failed' })));
        setErrorMessage(String(err));
        toast.error('Installation error');
      }
    };

    runRealInstall();
  }, [skills, onComplete]);

  const getStatusIcon = (status: InstallStatus) => {
    switch (status) {
      case 'pending':
        return <div className="h-5 w-5 rounded-full border-2 border-slate-500" />;
      case 'installing':
        return <Loader2 className="h-5 w-5 text-primary animate-spin" />;
      case 'completed':
        return <CheckCircle2 className="h-5 w-5 text-green-400" />;
      case 'failed':
        return <XCircle className="h-5 w-5 text-red-400" />;
    }
  };

  const getStatusText = (skill: SkillInstallState) => {
    switch (skill.status) {
      case 'pending':
        return <span className="text-muted-foreground">{t('installing.status.pending')}</span>;
      case 'installing':
        return <span className="text-primary">{t('installing.status.installing')}</span>;
      case 'completed':
        return <span className="text-green-400">{t('installing.status.installed')}</span>;
      case 'failed':
        return <span className="text-red-400">{t('installing.status.failed')}</span>;
    }
  };

  return (
    <div className="space-y-6">
      <div className="text-center">
        <div className="text-4xl mb-4">⚙️</div>
        <h2 className="text-xl font-semibold mb-2">{t('installing.title')}</h2>
        <p className="text-muted-foreground">
          {t('installing.subtitle')}
        </p>
      </div>

      {/* Progress bar */}
      <div className="space-y-2">
        <div className="flex justify-between text-sm">
          <span className="text-muted-foreground">{t('installing.progress')}</span>
          <span className="text-primary">{overallProgress}%</span>
        </div>
        <div className="h-2 bg-secondary rounded-full overflow-hidden">
          <motion.div
            className="h-full bg-primary"
            initial={{ width: 0 }}
            animate={{ width: `${overallProgress}%` }}
            transition={{ duration: 0.3 }}
          />
        </div>
      </div>

      {/* Skill list */}
      <div className="space-y-2 max-h-48 overflow-y-auto">
        {skillStates.map((skill) => (
          <motion.div
            key={skill.id}
            initial={{ opacity: 0, y: 10 }}
            animate={{ opacity: 1, y: 0 }}
            className={cn(
              'flex items-center justify-between p-3 rounded-lg',
              skill.status === 'installing' ? 'bg-muted' : 'bg-muted/50'
            )}
          >
            <div className="flex items-center gap-3">
              {getStatusIcon(skill.status)}
              <div>
                <p className="font-medium">{skill.name}</p>
                <p className="text-xs text-muted-foreground">{skill.description}</p>
              </div>
            </div>
            {getStatusText(skill)}
          </motion.div>
        ))}
      </div>

      {/* Error Message Display */}
      {errorMessage && (
        <motion.div
          initial={{ opacity: 0, scale: 0.95 }}
          animate={{ opacity: 1, scale: 1 }}
          className="p-4 rounded-lg bg-red-900/30 border border-red-500/50 text-red-200 text-sm"
        >
          <div className="flex items-start gap-2">
            <AlertCircle className="h-5 w-5 text-red-400 shrink-0 mt-0.5" />
            <div className="space-y-1">
              <p className="font-semibold">{t('installing.error')}</p>
              <pre className="text-xs bg-black/30 p-2 rounded overflow-x-auto whitespace-pre-wrap font-monospace">
                {errorMessage}
              </pre>
              <Button
                variant="link"
                className="text-red-400 p-0 h-auto text-xs underline"
                onClick={() => window.location.reload()}
              >
                {t('installing.restart')}
              </Button>
            </div>
          </div>
        </motion.div>
      )}

      {!errorMessage && (
        <p className="text-sm text-slate-400 text-center">
          {t('installing.wait')}
        </p>
      )}
      <div className="flex justify-end">
        <Button
          variant="ghost"
          className="text-muted-foreground"
          onClick={onSkip}
        >
          {t('installing.skip')}
        </Button>
      </div>
    </div>
  );
}
interface CompleteContentProps {
  selectedProvider: string | null;
  installedSkills: string[];
}

function CompleteContent({ selectedProvider, installedSkills }: CompleteContentProps) {
  const { t } = useTranslation(['setup', 'settings']);
  const gatewayStatus = useGatewayStore((state) => state.status);

  const providerData = providers.find((p) => p.id === selectedProvider);
  const installedSkillNames = defaultSkills
    .filter((s) => installedSkills.includes(s.id))
    .map((s) => s.name)
    .join(', ');

  return (
    <div className="text-center space-y-6">
      <div className="text-6xl mb-4">🎉</div>
      <h2 className="text-xl font-semibold">{t('complete.title')}</h2>
      <p className="text-muted-foreground">
        {t('complete.subtitle')}
      </p>

      <div className="space-y-3 text-left max-w-md mx-auto">
        <div className="flex items-center justify-between p-3 rounded-lg bg-muted/50">
          <span>{t('complete.provider')}</span>
          <span className="text-green-400">
            {providerData ? <span className="flex items-center gap-1.5">{getProviderIconUrl(providerData.id) ? <img src={getProviderIconUrl(providerData.id)} alt={providerData.name} className={`h-4 w-4 inline-block ${shouldInvertInDark(providerData.id) ? 'dark:invert' : ''}`} /> : providerData.icon} {providerData.id === 'custom' ? t('settings:aiProviders.custom') : providerData.name}</span> : '—'}
          </span>
        </div>
        <div className="flex items-center justify-between p-3 rounded-lg bg-muted/50">
          <span>{t('complete.components')}</span>
          <span className="text-green-400">
            {installedSkillNames || `${installedSkills.length} ${t('installing.status.installed')}`}
          </span>
        </div>
        <div className="flex items-center justify-between p-3 rounded-lg bg-muted/50">
          <span>{t('complete.gateway')}</span>
          <span className={gatewayStatus.state === 'running' ? 'text-green-400' : 'text-yellow-400'}>
            {gatewayStatus.state === 'running' ? `✓ ${t('complete.running')}` : gatewayStatus.state}
          </span>
        </div>
      </div>

      <p className="text-sm text-muted-foreground">
        {t('complete.footer')}
      </p>
    </div>
  );
}

export default Setup;
