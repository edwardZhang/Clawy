import { invoke, isTauri } from '@tauri-apps/api/core';
import { listen, once } from '@tauri-apps/api/event';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { message as dialogMessage, open as dialogOpen, save as dialogSave } from '@tauri-apps/plugin-dialog';
import { openUrl, revealItemInDir } from '@tauri-apps/plugin-opener';

type ListenerCallback = (...args: unknown[]) => void;
type UnlistenFn = () => void;

type ListenerRecord = {
  original: ListenerCallback;
  unlisten: Promise<UnlistenFn>;
};

type DesktopBridgeApi = {
  ipcRenderer: {
    invoke: (channel: string, ...args: unknown[]) => Promise<unknown>;
    on: (channel: string, callback: ListenerCallback) => (() => void) | void;
    once: (channel: string, callback: ListenerCallback) => void;
    off: (channel: string, callback?: ListenerCallback) => void;
  };
  openExternal: (url: string) => Promise<void>;
  platform: NodeJS.Platform;
  isDev: boolean;
};

type GatewayStatus = {
  state: string;
  port: number;
  pid?: number;
  uptime?: number;
  error?: string;
  connectedAt?: number;
  version?: string;
  reconnectAttempts?: number;
};

type GatewayRpcSuccess<T> = {
  success: true;
  result: T;
};

type GatewayRpcFailure = {
  success: false;
  error: string;
};

type GatewayRpcResult<T> = GatewayRpcSuccess<T> | GatewayRpcFailure;
type BrowserTimer = number;

type PendingRequest = {
  reject: (error: Error) => void;
  resolve: (value: unknown) => void;
  timer: BrowserTimer;
};

type ConnectFrame = {
  type: 'req';
  id: string;
  method: 'connect';
  params: Record<string, unknown>;
};

type GatewaySocketEnvelope =
  | {
      type: 'event';
      event: string;
      payload?: unknown;
      seq?: number;
      stateVersion?: number;
    }
  | {
      type: 'req';
      id?: string | number;
      method: string;
      params?: unknown;
    }
  | {
      type: 'res';
      id: string | number;
      ok?: boolean;
      payload?: unknown;
      error?: { message?: string } | string;
    };

type GatewayCronJob = {
  id: string;
  name: string;
  description?: string;
  enabled: boolean;
  createdAtMs: number;
  updatedAtMs: number;
  schedule: {
    kind: string;
    expr?: string;
    everyMs?: number;
    at?: string;
    tz?: string;
  };
  payload?: {
    kind?: string;
    message?: string;
    text?: string;
  };
  delivery?: {
    mode?: string;
    channel?: string;
    to?: string;
  };
  sessionTarget?: string;
  state?: {
    nextRunAtMs?: number;
    lastRunAtMs?: number;
    lastStatus?: string;
    lastError?: string;
    lastDurationMs?: number;
  };
};

const listenerRegistry = new Map<string, ListenerRecord[]>();
const gatewayEventChannels = new Set([
  'gateway:status-changed',
  'gateway:message',
  'gateway:notification',
  'gateway:channel-status',
  'gateway:chat-message',
  'gateway:exit',
  'gateway:error',
]);

function dispatchCallback(callback: ListenerCallback, payload: unknown): void {
  if (Array.isArray(payload)) {
    callback(...payload);
    return;
  }
  callback(payload);
}

function rememberListener(channel: string, original: ListenerCallback, unlisten: Promise<UnlistenFn>): void {
  const records = listenerRegistry.get(channel) ?? [];
  records.push({ original, unlisten });
  listenerRegistry.set(channel, records);
}

function forgetListener(channel: string, callback?: ListenerCallback): void {
  const records = listenerRegistry.get(channel);
  if (!records?.length) return;

  const next: ListenerRecord[] = [];
  for (const record of records) {
    if (callback && record.original !== callback) {
      next.push(record);
      continue;
    }
    void record.unlisten.then((unlisten) => unlisten()).catch(() => {});
  }

  if (next.length > 0) {
    listenerRegistry.set(channel, next);
  } else {
    listenerRegistry.delete(channel);
  }
}

class LocalEventBus {
  private listeners = new Map<string, Set<ListenerCallback>>();

  on(channel: string, callback: ListenerCallback): () => void {
    const records = this.listeners.get(channel) ?? new Set<ListenerCallback>();
    records.add(callback);
    this.listeners.set(channel, records);
    return () => {
      this.off(channel, callback);
    };
  }

  once(channel: string, callback: ListenerCallback): () => void {
    const wrapped: ListenerCallback = (...args: unknown[]) => {
      this.off(channel, wrapped);
      callback(...args);
    };
    return this.on(channel, wrapped);
  }

  off(channel: string, callback?: ListenerCallback): void {
    const records = this.listeners.get(channel);
    if (!records) return;

    if (!callback) {
      this.listeners.delete(channel);
      return;
    }

    records.delete(callback);
    if (records.size === 0) {
      this.listeners.delete(channel);
    }
  }

  emit(channel: string, payload: unknown): void {
    const callbacks = this.listeners.get(channel);
    if (!callbacks?.size) return;
    for (const callback of callbacks) {
      dispatchCallback(callback, payload);
    }
  }

  supports(channel: string): boolean {
    return gatewayEventChannels.has(channel);
  }
}

function normalizeDialogFilters(filters: unknown): Array<{ name: string; extensions: string[] }> | undefined {
  if (!Array.isArray(filters)) return undefined;

  const normalized = filters
    .map((filter) => {
      if (!filter || typeof filter !== 'object') return null;
      const candidate = filter as { name?: unknown; extensions?: unknown };
      if (typeof candidate.name !== 'string' || !Array.isArray(candidate.extensions)) return null;
      const extensions = candidate.extensions.filter((value): value is string => typeof value === 'string');
      return { name: candidate.name, extensions };
    })
    .filter((value): value is { name: string; extensions: string[] } => value !== null);

  return normalized.length > 0 ? normalized : undefined;
}

function normalizeOpenDialogOptions(
  options: Record<string, unknown> = {},
): {
  title?: string;
  defaultPath?: string;
  filters?: Array<{ name: string; extensions: string[] }>;
  multiple?: boolean;
  directory?: boolean;
  canCreateDirectories?: boolean;
} {
  const properties = Array.isArray(options.properties)
    ? options.properties.filter((value): value is string => typeof value === 'string')
    : [];

  return {
    title: typeof options.title === 'string' ? options.title : undefined,
    defaultPath: typeof options.defaultPath === 'string' ? options.defaultPath : undefined,
    filters: normalizeDialogFilters(options.filters),
    multiple: properties.includes('multiSelections'),
    directory: properties.includes('openDirectory'),
    canCreateDirectories: properties.includes('createDirectory'),
  };
}

function normalizeSaveDialogOptions(
  options: Record<string, unknown> = {},
): {
  title?: string;
  defaultPath?: string;
  filters?: Array<{ name: string; extensions: string[] }>;
} {
  return {
    title: typeof options.title === 'string' ? options.title : undefined,
    defaultPath: typeof options.defaultPath === 'string' ? options.defaultPath : undefined,
    filters: normalizeDialogFilters(options.filters),
  };
}

function toErrorMessage(error: unknown, fallback = 'Unknown error'): string {
  if (error instanceof Error) return error.message;
  if (typeof error === 'string') return error;
  if (error && typeof error === 'object' && 'message' in error && typeof error.message === 'string') {
    return error.message;
  }
  return fallback;
}

function isPairingRequiredError(error: unknown): boolean {
  return toErrorMessage(error).toLowerCase().includes('pairing required');
}

function sleep(ms: number): Promise<void> {
  return new Promise((resolve) => {
    window.setTimeout(resolve, ms);
  });
}

function transformCronJob(job: GatewayCronJob): Record<string, unknown> {
  const message = job.payload?.message || job.payload?.text || '';
  const channelType = job.delivery?.channel;
  const target = channelType
    ? { channelType, channelId: channelType, channelName: channelType }
    : undefined;

  const lastRun = job.state?.lastRunAtMs
    ? {
        time: new Date(job.state.lastRunAtMs).toISOString(),
        success: job.state.lastStatus === 'ok',
        error: job.state.lastError,
        duration: job.state.lastDurationMs,
      }
    : undefined;

  const nextRun = job.state?.nextRunAtMs
    ? new Date(job.state.nextRunAtMs).toISOString()
    : undefined;

  return {
    id: job.id,
    name: job.name,
    message,
    schedule: job.schedule,
    target,
    enabled: job.enabled,
    createdAt: new Date(job.createdAtMs).toISOString(),
    updatedAt: new Date(job.updatedAtMs).toISOString(),
    lastRun,
    nextRun,
  };
}

class GatewayBridge {
  private connectPromise: Promise<void> | null = null;
  private connectRequestId: string | null = null;
  private localBus: LocalEventBus;
  private pendingRequests = new Map<string, PendingRequest>();
  private processEventUnlisteners: Array<UnlistenFn> = [];
  private reconnectAttempts = 0;
  private reconnectTimer: BrowserTimer | null = null;
  private status: GatewayStatus = { state: 'stopped', port: 18_789 };
  private stopping = false;
  private wantedRunning = false;
  private ws: WebSocket | null = null;

  constructor(localBus: LocalEventBus) {
    this.localBus = localBus;
  }

  async initialize(): Promise<void> {
    if (this.processEventUnlisteners.length === 0) {
      const exitUnlisten = await listen<number | null>('gateway:exit', (event) => {
        const code = event.payload ?? null;
        this.cleanupSocket(`Gateway exited${code == null ? '' : ` (${code})`}`, true);
        this.setStatus({
          ...this.status,
          connectedAt: undefined,
          error: code == null ? undefined : `Gateway exited (${code})`,
          pid: undefined,
          reconnectAttempts: this.reconnectAttempts,
          state: this.wantedRunning ? 'error' : 'stopped',
          uptime: undefined,
        });
        this.localBus.emit('gateway:exit', code);
      });
      const errorUnlisten = await listen<string>('gateway:error', (event) => {
        const message = String(event.payload || 'Gateway error');
        this.localBus.emit('gateway:error', message);
        if (this.status.state !== 'running' && this.status.state !== 'reconnecting') {
          this.setStatus({
            ...this.status,
            error: message,
            reconnectAttempts: this.reconnectAttempts,
            state: 'error',
          });
        }
      });
      this.processEventUnlisteners.push(exitUnlisten, errorUnlisten);
    }

    const status = await this.fetchRemoteStatus();
    this.status = status;

    let gatewayAutoStart = false;
    try {
      const settings = await this.invokeRemote<{ gatewayAutoStart?: boolean }>('settings:getAll');
      gatewayAutoStart = settings.gatewayAutoStart === true;
    } catch {
      // Ignore settings fetch failures and fall back to the remote status only.
    }

    const shouldAutoStart =
      gatewayAutoStart && (status.state === 'stopped' || status.state === 'error');

    this.wantedRunning =
      shouldAutoStart ||
      status.state === 'starting' ||
      status.state === 'reconnecting' ||
      status.state === 'running';

    if (this.wantedRunning) {
      void this.ensureConnected(shouldAutoStart, 45_000).catch((error) => {
        const message = toErrorMessage(error, 'Failed to connect to Gateway');
        this.setStatus({
          ...this.status,
          error: message,
          reconnectAttempts: this.reconnectAttempts,
          state: 'error',
        });
        this.localBus.emit('gateway:error', message);
      });
    }
  }

  async getStatus(): Promise<GatewayStatus> {
    return { ...this.status };
  }

  isConnected(): boolean {
    return this.status.state === 'running' && this.ws?.readyState === WebSocket.OPEN;
  }

  async start(): Promise<{ success: boolean; error?: string }> {
    this.wantedRunning = true;
    this.stopping = false;
    this.cancelReconnect();
    this.setStatus({
      ...this.status,
      error: undefined,
      reconnectAttempts: 0,
      state: 'starting',
    });

    const result = await this.invokeRemote<{ success: boolean; error?: string }>('gateway:start');
    if (!result.success) {
      this.wantedRunning = false;
      this.setStatus({
        ...this.status,
        error: result.error,
        reconnectAttempts: 0,
        state: 'error',
      });
      return result;
    }

    try {
      await this.ensureConnected(false, 45_000);
      return { success: true };
    } catch (error) {
      const message = toErrorMessage(error, 'Failed to connect to Gateway');
      this.setStatus({
        ...this.status,
        error: message,
        reconnectAttempts: this.reconnectAttempts,
        state: 'error',
      });
      return { success: false, error: message };
    }
  }

  async stop(): Promise<{ success: boolean; error?: string }> {
    this.stopping = true;
    this.wantedRunning = false;
    this.cancelReconnect();
    this.cleanupSocket('Gateway stopped', true);

    const result = await this.invokeRemote<{ success: boolean; error?: string }>('gateway:stop');
    this.stopping = false;
    if (result.success) {
      this.reconnectAttempts = 0;
      this.setStatus({
        ...this.status,
        connectedAt: undefined,
        error: undefined,
        pid: undefined,
        reconnectAttempts: 0,
        state: 'stopped',
        uptime: undefined,
        version: undefined,
      });
    } else {
      this.setStatus({
        ...this.status,
        error: result.error,
        reconnectAttempts: this.reconnectAttempts,
        state: 'error',
      });
    }
    return result;
  }

  async restart(): Promise<{ success: boolean; error?: string }> {
    this.wantedRunning = true;
    this.stopping = false;
    this.cancelReconnect();
    this.cleanupSocket('Gateway restarting', true);
    this.setStatus({
      ...this.status,
      connectedAt: undefined,
      error: undefined,
      reconnectAttempts: 0,
      state: 'starting',
      version: undefined,
    });

    const result = await this.invokeRemote<{ success: boolean; error?: string }>('gateway:restart');
    if (!result.success) {
      this.setStatus({
        ...this.status,
        error: result.error,
        reconnectAttempts: 0,
        state: 'error',
      });
      return result;
    }

    try {
      await this.ensureConnected(false, 45_000);
      return { success: true };
    } catch (error) {
      const message = toErrorMessage(error, 'Failed to reconnect to Gateway');
      this.setStatus({
        ...this.status,
        error: message,
        reconnectAttempts: this.reconnectAttempts,
        state: 'error',
      });
      return { success: false, error: message };
    }
  }

  async health(): Promise<{ success: boolean; ok: boolean; error?: string; uptime?: number }> {
    const health = await this.invokeRemote<{
      success: boolean;
      ok: boolean;
      error?: string;
      uptime?: number;
    }>('gateway:health');

    if (!health.ok) {
      return health;
    }

    if (this.isConnected()) {
      return {
        ...health,
        ok: true,
        error: undefined,
      };
    }

    try {
      await this.ensureConnected(false, 12_000);
      return {
        ...health,
        ok: true,
        error: undefined,
      };
    } catch (error) {
      return {
        ...health,
        ok: false,
        error: toErrorMessage(error, 'Gateway connection validation failed'),
      };
    }
  }

  async rpc<T>(method: string, params?: unknown, timeoutMs = 30_000): Promise<T> {
    await this.ensureConnected(true, Math.max(timeoutMs, 30_000));

    const ws = this.ws;
    if (!ws || ws.readyState !== WebSocket.OPEN) {
      throw new Error('Gateway socket is not connected');
    }

    const id = `rpc-${crypto.randomUUID()}`;
    const payload = {
      type: 'req',
      id,
      method,
      params,
    };

    return await new Promise<T>((resolve, reject) => {
      const timer = window.setTimeout(() => {
        this.pendingRequests.delete(id);
        reject(new Error(`Gateway request timed out: ${method}`));
      }, timeoutMs);

      this.pendingRequests.set(id, {
        resolve: (value) => resolve(value as T),
        reject,
        timer,
      });

      try {
        ws.send(JSON.stringify(payload));
      } catch (error) {
        window.clearTimeout(timer);
        this.pendingRequests.delete(id);
        reject(error instanceof Error ? error : new Error(String(error)));
      }
    });
  }

  async invokeRpc<T>(method: string, params?: unknown, timeoutMs?: number): Promise<GatewayRpcResult<T>> {
    try {
      const result = await this.rpc<T>(method, params, timeoutMs);
      return { success: true, result };
    } catch (error) {
      return { success: false, error: toErrorMessage(error, `RPC call failed: ${method}`) };
    }
  }

  async sendWithMedia(params: Record<string, unknown>): Promise<GatewayRpcResult<{ runId?: string }>> {
    try {
      const prepared = await this.invokeRemote<Record<string, unknown>>('chat:prepareWithMedia', params);
      const result = await this.rpc<{ runId?: string }>('chat.send', prepared, 120_000);
      return { success: true, result };
    } catch (error) {
      return {
        success: false,
        error: toErrorMessage(error, 'Failed to send chat message with media'),
      };
    }
  }

  private async ensureConnected(allowStart: boolean, timeoutMs: number): Promise<void> {
    if (this.ws?.readyState === WebSocket.OPEN && this.status.state === 'running') {
      return;
    }
    if (this.connectPromise) {
      await this.connectPromise;
      return;
    }

    this.connectPromise = this.connectLoop(allowStart, timeoutMs).finally(() => {
      this.connectPromise = null;
    });
    await this.connectPromise;
  }

  private async connectLoop(allowStart: boolean, timeoutMs: number): Promise<void> {
    if (allowStart && (this.status.state === 'stopped' || this.status.state === 'error')) {
      const startResult = await this.invokeRemote<{ success: boolean; error?: string }>('gateway:start');
      if (!startResult.success) {
        throw new Error(startResult.error || 'Failed to start Gateway');
      }
      this.wantedRunning = true;
      this.setStatus({
        ...this.status,
        error: undefined,
        state: 'starting',
      });
    }

    const deadline = Date.now() + timeoutMs;
    let lastError: Error | null = null;
    let attemptedAutoPair = false;

    while (Date.now() < deadline) {
      try {
        await this.connectOnce(Math.min(12_000, deadline - Date.now()));
        return;
      } catch (error) {
        lastError = error instanceof Error ? error : new Error(String(error));

        if (!attemptedAutoPair && isPairingRequiredError(lastError)) {
          attemptedAutoPair = true;
          try {
            const approval = await this.invokeRemote<{ success: boolean; approved?: boolean; error?: string }>(
              'gateway:autoApprovePairing',
            );
            if (approval.success && approval.approved) {
              lastError = null;
              await sleep(300);
              continue;
            }
            if (approval.error) {
              lastError = new Error(approval.error);
            }
          } catch (approvalError) {
            lastError = approvalError instanceof Error ? approvalError : new Error(String(approvalError));
          }
        }

        if (!this.wantedRunning && !allowStart) {
          break;
        }
        await sleep(500);
      }
    }

    throw lastError ?? new Error('Gateway connection timed out');
  }

  private async connectOnce(timeoutMs: number): Promise<void> {
    if (timeoutMs <= 0) {
      throw new Error('Gateway connection timed out');
    }

    const remoteStatus = await this.fetchRemoteStatus();
    this.status = remoteStatus;
    const url = `ws://127.0.0.1:${remoteStatus.port}/ws`;

    await new Promise<void>((resolve, reject) => {
      let connectResolved = false;
      let challengeTimer: BrowserTimer | null = null;
      const ws = new WebSocket(url);
      this.ws = ws;

      const rejectOnce = (error: Error) => {
        if (connectResolved) return;
        connectResolved = true;
        this.connectRequestId = null;
        if (challengeTimer) {
          window.clearTimeout(challengeTimer);
          challengeTimer = null;
        }
        if (this.ws === ws) {
          this.ws = null;
        }
        try {
          ws.close();
        } catch {
          // ignore
        }
        reject(error);
      };

      const resolveOnce = () => {
        if (connectResolved) return;
        connectResolved = true;
        if (challengeTimer) {
          window.clearTimeout(challengeTimer);
          challengeTimer = null;
        }
        resolve();
      };

      const sendConnectHandshake = async (nonce: string) => {
        try {
          const connectParams = await this.invokeRemote<Record<string, unknown>>('gateway:buildConnectParams', {
            clientId: 'gateway-client',
            clientMode: 'ui',
            nonce,
            role: 'operator',
            scopes: ['operator.admin'],
          });

          this.connectRequestId = `connect-${crypto.randomUUID()}`;
          const connectFrame: ConnectFrame = {
            type: 'req',
            id: this.connectRequestId,
            method: 'connect',
            params: connectParams,
          };
          ws.send(JSON.stringify(connectFrame));
        } catch (error) {
          rejectOnce(new Error(toErrorMessage(error, 'Failed to build Gateway connect payload')));
        }
      };

      challengeTimer = window.setTimeout(() => {
        rejectOnce(new Error('Timed out waiting for connect.challenge from Gateway'));
      }, timeoutMs);

      ws.onopen = () => {
        // The server will send connect.challenge immediately after open.
      };

      ws.onerror = () => {
        rejectOnce(new Error('WebSocket connection failed'));
      };

      ws.onclose = (event) => {
        if (!connectResolved) {
          rejectOnce(new Error(`WebSocket closed before handshake (${event.code} ${event.reason || 'no reason'})`));
          return;
        }
        this.handleSocketClosed(`Gateway socket closed (${event.code} ${event.reason || 'no reason'})`, false);
      };

      ws.onmessage = (event) => {
        void this.handleSocketMessage(String(event.data || ''), ws, resolveOnce, rejectOnce, sendConnectHandshake);
      };
    });
  }

  private async handleSocketMessage(
    raw: string,
    socket: WebSocket,
    resolveConnect?: () => void,
    rejectConnect?: (error: Error) => void,
    sendConnectHandshake?: (nonce: string) => Promise<void>,
  ): Promise<void> {
    let message: GatewaySocketEnvelope | Record<string, unknown>;
    try {
      message = JSON.parse(raw) as GatewaySocketEnvelope | Record<string, unknown>;
    } catch {
      return;
    }

    this.localBus.emit('gateway:message', message);

    if (
      sendConnectHandshake &&
      'type' in message &&
      message.type === 'event' &&
      message.event === 'connect.challenge'
    ) {
      const nonce = typeof message.payload === 'object' && message.payload && 'nonce' in message.payload
        ? String((message.payload as { nonce?: unknown }).nonce || '')
        : '';

      if (!nonce) {
        rejectConnect?.(new Error('Gateway connect.challenge missing nonce'));
        return;
      }

      await sendConnectHandshake(nonce);
      return;
    }

    if (
      'type' in message &&
      message.type === 'res' &&
      this.connectRequestId &&
      String(message.id) === this.connectRequestId
    ) {
      this.connectRequestId = null;
      if (message.ok === false || message.error) {
        const responseError = message.error as { message?: string } | string | undefined;
        const errorMessage =
          typeof responseError === 'string'
            ? responseError
            : responseError?.message || 'Gateway connect failed';
        rejectConnect?.(new Error(errorMessage));
        return;
      }

      const payload = (message.payload ?? {}) as Record<string, unknown>;
      const protocol = payload.protocol;
      const version = typeof protocol === 'number' ? `protocol-${protocol}` : undefined;

      this.reconnectAttempts = 0;
      this.setStatus({
        ...this.status,
        connectedAt: Date.now(),
        error: undefined,
        reconnectAttempts: 0,
        state: 'running',
        version,
      });

      void this.invokeRemote('gateway:onConnected', {
        version,
      }).catch(() => {});

      resolveConnect?.();
      return;
    }

    if ('type' in message && message.type === 'res' && message.id != null) {
      const requestId = String(message.id);
      const pending = this.pendingRequests.get(requestId);
      if (!pending) return;

      this.pendingRequests.delete(requestId);
      window.clearTimeout(pending.timer);

      if (message.ok === false || message.error) {
        const responseError = message.error as { message?: string } | string | undefined;
        const errorMessage =
          typeof responseError === 'string'
            ? responseError
            : responseError?.message || 'Gateway request failed';
        pending.reject(new Error(errorMessage));
      } else {
        pending.resolve(message.payload);
      }
      return;
    }

    if ('type' in message && message.type === 'event' && typeof message.event === 'string') {
      this.handleProtocolEvent(message.event, message.payload);
      return;
    }

    if ('method' in message && typeof message.method === 'string' && message.method === 'ping') {
      if (socket.readyState === WebSocket.OPEN) {
        socket.send(JSON.stringify({ method: 'pong' }));
      }
      return;
    }
  }

  private handleProtocolEvent(event: string, payload: unknown): void {
    switch (event) {
      case 'tick':
      case 'heartbeat':
        break;
      case 'chat':
        this.localBus.emit('gateway:chat-message', { message: payload });
        break;
      case 'agent': {
        const record = typeof payload === 'object' && payload ? payload as Record<string, unknown> : {};
        const data = record.data && typeof record.data === 'object'
          ? record.data as Record<string, unknown>
          : {};
        const chatEvent: Record<string, unknown> = {
          ...data,
          runId: record.runId ?? data.runId,
          sessionKey: record.sessionKey ?? data.sessionKey,
          state: record.state ?? data.state,
          message: record.message ?? data.message,
        };
        if (chatEvent.state || chatEvent.message) {
          this.localBus.emit('gateway:chat-message', { message: chatEvent });
        }
        this.localBus.emit('gateway:notification', { method: event, params: payload });
        break;
      }
      case 'channel.status':
        this.localBus.emit('gateway:channel-status', payload);
        break;
      default:
        this.localBus.emit('gateway:notification', { method: event, params: payload });
    }
  }

  private handleSocketClosed(reason: string, expected: boolean): void {
    this.cleanupSocket(reason, expected);

    if (expected || this.stopping || !this.wantedRunning) {
      this.setStatus({
        ...this.status,
        connectedAt: undefined,
        error: expected ? undefined : this.status.error,
        reconnectAttempts: 0,
        state: 'stopped',
        version: undefined,
      });
      void this.invokeRemote('gateway:onDisconnected', {
        error: undefined,
        reconnecting: false,
      }).catch(() => {});
      return;
    }

    this.scheduleReconnect(reason);
  }

  private cleanupSocket(reason: string, expected: boolean): void {
    const ws = this.ws;
    this.ws = null;
    this.connectRequestId = null;

    if (ws && ws.readyState === WebSocket.OPEN) {
      try {
        ws.close(expected ? 1000 : 1011, reason);
      } catch {
        // ignore
      }
    }

    for (const [id, pending] of this.pendingRequests) {
      this.pendingRequests.delete(id);
      window.clearTimeout(pending.timer);
      pending.reject(new Error(reason));
    }
  }

  private scheduleReconnect(reason: string): void {
    this.cancelReconnect();
    this.reconnectAttempts += 1;

    this.setStatus({
      ...this.status,
      connectedAt: undefined,
      error: reason,
      reconnectAttempts: this.reconnectAttempts,
      state: 'reconnecting',
      version: undefined,
    });
    this.localBus.emit('gateway:error', reason);

    void this.invokeRemote('gateway:onDisconnected', {
      error: reason,
      reconnecting: true,
    }).catch(() => {});

    const delayMs = Math.min(1_000 * 2 ** Math.max(this.reconnectAttempts - 1, 0), 10_000);
    this.reconnectTimer = window.setTimeout(() => {
      this.reconnectTimer = null;
      void this.ensureConnected(false, 30_000).catch((error) => {
        this.scheduleReconnect(toErrorMessage(error, 'Gateway reconnect failed'));
      });
    }, delayMs);
  }

  private cancelReconnect(): void {
    if (this.reconnectTimer) {
      window.clearTimeout(this.reconnectTimer);
      this.reconnectTimer = null;
    }
  }

  private async fetchRemoteStatus(): Promise<GatewayStatus> {
    return await this.invokeRemote<GatewayStatus>('gateway:status');
  }

  private async invokeRemote<T>(channel: string, ...args: unknown[]): Promise<T> {
    return await invoke<T>('invoke_ipc', { channel, args });
  }

  private setStatus(status: GatewayStatus): void {
    this.status = status;
    this.localBus.emit('gateway:status-changed', { ...status });
  }
}

async function invokeLocalChannel(
  channel: string,
  args: unknown[],
  gatewayBridge: GatewayBridge,
): Promise<unknown> {
  switch (channel) {
    case 'shell:openExternal': {
      const [url] = args as [string];
      await openUrl(url);
      return null;
    }
    case 'shell:showItemInFolder': {
      const [targetPath] = args as [string];
      await revealItemInDir(targetPath);
      return null;
    }
    case 'shell:openPath': {
      await invoke('invoke_ipc', { channel, args });
      return '';
    }
    case 'dialog:open': {
      const [rawOptions] = args as [Record<string, unknown> | undefined];
      const result = await dialogOpen(normalizeOpenDialogOptions(rawOptions));
      if (result == null) {
        return { canceled: true, filePaths: [] };
      }
      if (Array.isArray(result)) {
        return { canceled: result.length === 0, filePaths: result };
      }
      return { canceled: false, filePaths: [result] };
    }
    case 'dialog:save': {
      const [rawOptions] = args as [Record<string, unknown> | undefined];
      const filePath = await dialogSave(normalizeSaveDialogOptions(rawOptions));
      return {
        canceled: filePath == null,
        filePath: filePath ?? undefined,
      };
    }
    case 'dialog:message': {
      const [rawOptions] = args as [Record<string, unknown> | undefined];
      const options = rawOptions ?? {};
      const content = typeof options.message === 'string' ? options.message : '';
      await dialogMessage(content, {
        title: typeof options.title === 'string' ? options.title : undefined,
        kind:
          options.type === 'error' || options.type === 'warning'
            ? options.type
            : 'info',
      });
      return { response: 0, checkboxChecked: false };
    }
    case 'window:minimize':
      await getCurrentWindow().minimize();
      return null;
    case 'window:maximize': {
      const currentWindow = getCurrentWindow();
      const maximized = await currentWindow.isMaximized();
      if (maximized) {
        await currentWindow.unmaximize();
      } else {
        await currentWindow.maximize();
      }
      return null;
    }
    case 'window:close':
      await getCurrentWindow().close();
      return null;
    case 'window:isMaximized':
      return await getCurrentWindow().isMaximized();
    case 'gateway:status':
      return await gatewayBridge.getStatus();
    case 'gateway:isConnected':
      return gatewayBridge.isConnected();
    case 'gateway:start':
      return await gatewayBridge.start();
    case 'gateway:stop':
      return await gatewayBridge.stop();
    case 'gateway:restart':
      return await gatewayBridge.restart();
    case 'gateway:rpc': {
      const [method, params, timeoutMs] = args as [string, unknown, number | undefined];
      return await gatewayBridge.invokeRpc(method, params, timeoutMs);
    }
    case 'gateway:health':
      return await gatewayBridge.health();
    case 'cron:list': {
      const result = await gatewayBridge.rpc<{ jobs?: GatewayCronJob[] }>('cron.list', {
        includeDisabled: true,
      });
      const jobs = result.jobs ?? [];

      for (const job of jobs) {
        const isIsolatedAgent =
          (job.sessionTarget === 'isolated' || !job.sessionTarget) &&
          job.payload?.kind === 'agentTurn';
        const needsRepair =
          isIsolatedAgent &&
          job.delivery?.mode === 'announce' &&
          !job.delivery?.channel;

        if (!needsRepair) {
          continue;
        }

        const repairResult = await gatewayBridge.invokeRpc('cron.update', {
          id: job.id,
          patch: { delivery: { mode: 'none' } },
        });
        if (repairResult.success) {
          job.delivery = { mode: 'none' };
          if (job.state?.lastError?.includes('Channel is required')) {
            job.state.lastError = undefined;
            job.state.lastStatus = 'ok';
          }
        }
      }

      return jobs.map(transformCronJob);
    }
    case 'cron:create': {
      const [input] = args as [Record<string, unknown>];
      const result = await gatewayBridge.rpc<GatewayCronJob>('cron.add', {
        name: typeof input.name === 'string' ? input.name : 'Task',
        schedule: {
          kind: 'cron',
          expr: typeof input.schedule === 'string' ? input.schedule : '* * * * *',
        },
        payload: {
          kind: 'agentTurn',
          message: typeof input.message === 'string' ? input.message : '',
        },
        enabled: typeof input.enabled === 'boolean' ? input.enabled : true,
        wakeMode: 'next-heartbeat',
        sessionTarget: 'isolated',
        delivery: { mode: 'none' },
      });
      return transformCronJob(result);
    }
    case 'cron:update': {
      const [id, input] = args as [string, Record<string, unknown>];
      const patch: Record<string, unknown> = { ...(input ?? {}) };
      if (typeof patch.schedule === 'string') {
        patch.schedule = { kind: 'cron', expr: patch.schedule };
      }
      if (typeof patch.message === 'string') {
        patch.payload = { kind: 'agentTurn', message: patch.message };
        delete patch.message;
      }
      return await gatewayBridge.rpc('cron.update', { id, patch });
    }
    case 'cron:delete': {
      const [id] = args as [string];
      return await gatewayBridge.rpc('cron.remove', { id });
    }
    case 'cron:toggle': {
      const [id, enabled] = args as [string, boolean];
      return await gatewayBridge.rpc('cron.update', {
        id,
        patch: { enabled },
      });
    }
    case 'cron:trigger': {
      const [id] = args as [string];
      return await gatewayBridge.rpc('cron.run', {
        id,
        mode: 'force',
      }, 60_000);
    }
    case 'chat:sendWithMedia': {
      const [params] = args as [Record<string, unknown>];
      return await gatewayBridge.sendWithMedia(params);
    }
    default:
      return await invoke('invoke_ipc', { channel, args });
  }
}

function buildTauriDesktopBridge(
  platform: NodeJS.Platform,
  gatewayBridge: GatewayBridge,
  localBus: LocalEventBus,
): DesktopBridgeApi {
  return {
    ipcRenderer: {
      invoke: (channel: string, ...args: unknown[]) => invokeLocalChannel(channel, args, gatewayBridge),
      on: (channel: string, callback: ListenerCallback) => {
        if (localBus.supports(channel)) {
          return localBus.on(channel, callback);
        }

        const unlisten = listen(channel, (event) => {
          dispatchCallback(callback, event.payload);
        });
        rememberListener(channel, callback, unlisten);
        return () => {
          forgetListener(channel, callback);
        };
      },
      once: (channel: string, callback: ListenerCallback) => {
        if (localBus.supports(channel)) {
          localBus.once(channel, callback);
          return;
        }

        const unlisten = once(channel, (event) => {
          dispatchCallback(callback, event.payload);
        });
        rememberListener(channel, callback, unlisten);
      },
      off: (channel: string, callback?: ListenerCallback) => {
        if (localBus.supports(channel)) {
          localBus.off(channel, callback);
          return;
        }
        forgetListener(channel, callback);
      },
    },
    openExternal: async (url: string) => {
      await openUrl(url);
    },
    platform,
    isDev: import.meta.env.DEV,
  };
}

export async function installDesktopBridge(): Promise<void> {
  if (typeof window === 'undefined') return;
  if (window.desktop) return;
  if (!isTauri()) return;

  const platform = await invoke<NodeJS.Platform>('invoke_ipc', {
    channel: 'app:platform',
    args: [],
  });

  const localBus = new LocalEventBus();
  const gatewayBridge = new GatewayBridge(localBus);
  await gatewayBridge.initialize();

  window.desktop = buildTauriDesktopBridge(platform, gatewayBridge, localBus);
}
