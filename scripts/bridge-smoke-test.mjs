#!/usr/bin/env node

import process from 'node:process';
import { URL } from 'node:url';
import WebSocket from 'ws';

function parseArgs(argv) {
  const result = {
    baseUrl: process.env.CLAWY_BRIDGE_BASE_URL ?? '',
    token: process.env.CLAWY_BRIDGE_TOKEN ?? '',
    sessionId: process.env.CLAWY_BRIDGE_SESSION_ID ?? '',
    callerId: process.env.CLAWY_BRIDGE_CALLER_ID ?? '',
    withWrite: true,
    message:
      process.env.CLAWY_BRIDGE_SMOKE_MESSAGE ??
      'Bridge API smoke test. Reply with a short ok.',
    abortMessage:
      process.env.CLAWY_BRIDGE_ABORT_MESSAGE ??
      'Start a long task and do not finish quickly. This is an abort smoke test.',
    abortDelayMs: parseInteger(process.env.CLAWY_BRIDGE_ABORT_DELAY_MS, 200),
    historyLimit: parseInteger(process.env.CLAWY_BRIDGE_HISTORY_LIMIT, 2),
    wsTimeoutMs: parseInteger(process.env.CLAWY_BRIDGE_WS_TIMEOUT_MS, 2500),
  };

  for (let index = 0; index < argv.length; index += 1) {
    const arg = argv[index];
    const next = argv[index + 1];
    switch (arg) {
      case '--':
        break;
      case '--base-url':
        result.baseUrl = next ?? '';
        index += 1;
        break;
      case '--token':
        result.token = next ?? '';
        index += 1;
        break;
      case '--session-id':
        result.sessionId = next ?? '';
        index += 1;
        break;
      case '--caller-id':
        result.callerId = next ?? '';
        index += 1;
        break;
      case '--message':
        result.message = next ?? result.message;
        index += 1;
        break;
      case '--abort-message':
        result.abortMessage = next ?? result.abortMessage;
        index += 1;
        break;
      case '--abort-delay-ms':
        result.abortDelayMs = parseInteger(next, result.abortDelayMs);
        index += 1;
        break;
      case '--history-limit':
        result.historyLimit = parseInteger(next, result.historyLimit);
        index += 1;
        break;
      case '--ws-timeout-ms':
        result.wsTimeoutMs = parseInteger(next, result.wsTimeoutMs);
        index += 1;
        break;
      case '--read-only':
        result.withWrite = false;
        break;
      case '--with-write':
        result.withWrite = true;
        break;
      case '--help':
      case '-h':
        printHelpAndExit(0);
        break;
      default:
        throw new Error(`Unknown argument: ${arg}`);
    }
  }

  if (!result.baseUrl.trim()) {
    throw new Error('Missing Bridge base URL. Use --base-url or CLAWY_BRIDGE_BASE_URL.');
  }
  if (!result.token.trim()) {
    throw new Error('Missing Bridge token. Use --token or CLAWY_BRIDGE_TOKEN.');
  }

  return result;
}

function parseInteger(value, fallback) {
  if (value == null || value === '') return fallback;
  const parsed = Number.parseInt(String(value), 10);
  return Number.isFinite(parsed) ? parsed : fallback;
}

function printHelpAndExit(code) {
  console.log(`Clawy Bridge smoke test

Usage:
  node scripts/bridge-smoke-test.mjs --base-url http://127.0.0.1:18790/api/v1 --token bridge_xxx

Options:
  --base-url <url>         Bridge API base URL. Supports /api or /api/v1.
  --token <token>          Bridge Bearer token.
  --session-id <id>        Canonical session id. If omitted, uses the first visible session.
  --caller-id <id>         Optional x-clawy-caller-id header.
  --read-only              Skip send/abort tests.
  --with-write             Run send/abort tests (default).
  --message <text>         Text for the normal send smoke test.
  --abort-message <text>   Text for the abort smoke test.
  --abort-delay-ms <ms>    Delay between send and abort.
  --history-limit <n>      History limit for GET /history.
  --ws-timeout-ms <ms>     Timeout for WS initial/replay collection.

Env aliases:
  CLAWY_BRIDGE_BASE_URL
  CLAWY_BRIDGE_TOKEN
  CLAWY_BRIDGE_SESSION_ID
  CLAWY_BRIDGE_CALLER_ID
  CLAWY_BRIDGE_SMOKE_MESSAGE
  CLAWY_BRIDGE_ABORT_MESSAGE
  CLAWY_BRIDGE_ABORT_DELAY_MS
  CLAWY_BRIDGE_HISTORY_LIMIT
  CLAWY_BRIDGE_WS_TIMEOUT_MS
`);
  process.exit(code);
}

function normalizeBaseUrl(value) {
  const url = new URL(value);
  const pathname = url.pathname.replace(/\/+$/, '');
  if (pathname.endsWith('/api/v1')) {
    url.pathname = pathname;
    return url;
  }
  if (pathname.endsWith('/api')) {
    url.pathname = `${pathname}/v1`;
    return url;
  }
  url.pathname = `${pathname}/api/v1`.replace(/\/{2,}/g, '/');
  return url;
}

function endpoint(url, pathWithLeadingSlash) {
  const next = new URL(url.toString());
  const [pathname, search = ''] = pathWithLeadingSlash.split('?');
  next.pathname = `${url.pathname}${pathname}`.replace(/\/{2,}/g, '/');
  next.search = search ? `?${search}` : '';
  return next;
}

async function requestJson(baseUrl, token, callerId, method, path, body) {
  const url = endpoint(baseUrl, path);
  const headers = {
    Authorization: `Bearer ${token}`,
    Accept: 'application/json',
  };
  if (callerId) headers['x-clawy-caller-id'] = callerId;
  if (body != null) headers['Content-Type'] = 'application/json';

  const response = await fetch(url, {
    method,
    headers,
    body: body != null ? JSON.stringify(body) : undefined,
  });

  const text = await response.text();
  let parsed = text;
  if (text) {
    try {
      parsed = JSON.parse(text);
    } catch {
      parsed = text;
    }
  }

  return {
    status: response.status,
    headers: Object.fromEntries(response.headers.entries()),
    body: parsed,
  };
}

function encodeSessionId(sessionId) {
  return encodeURIComponent(sessionId);
}

async function collectWsFrames(wsUrl, token, callerId, timeoutMs, maxFrames = 3) {
  return new Promise((resolve, reject) => {
    const headers = {
      Authorization: `Bearer ${token}`,
    };
    if (callerId) headers['x-clawy-caller-id'] = callerId;

    const frames = [];
    const ws = new WebSocket(wsUrl, { headers });
    const timer = setTimeout(() => {
      try {
        ws.close();
      } catch {
        // ignore
      }
    }, timeoutMs);

    ws.on('message', (data) => {
      try {
        frames.push(JSON.parse(data.toString()));
      } catch {
        frames.push(data.toString());
      }
      if (frames.length >= maxFrames) {
        try {
          ws.close();
        } catch {
          // ignore
        }
      }
    });

    ws.on('close', () => {
      clearTimeout(timer);
      resolve(frames);
    });

    ws.on('error', (error) => {
      clearTimeout(timer);
      reject(error);
    });
  });
}

async function collectWsFramesAfterAction(
  wsUrl,
  token,
  callerId,
  timeoutMs,
  action,
  maxFrames = 4,
) {
  return new Promise((resolve, reject) => {
    const headers = {
      Authorization: `Bearer ${token}`,
    };
    if (callerId) headers['x-clawy-caller-id'] = callerId;

    const frames = [];
    let finished = false;
    let actionError = null;
    const ws = new WebSocket(wsUrl, { headers });
    const timer = setTimeout(() => finish(), timeoutMs);

    function finish(error) {
      if (finished) return;
      finished = true;
      clearTimeout(timer);
      try {
        ws.close();
      } catch {
        // ignore
      }
      if (error) {
        reject(error);
      } else if (actionError) {
        reject(actionError);
      } else {
        resolve(frames);
      }
    }

    ws.on('open', async () => {
      try {
        await action();
      } catch (error) {
        actionError = error instanceof Error ? error : new Error(String(error));
        finish(actionError);
      }
    });

    ws.on('message', (data) => {
      try {
        frames.push(JSON.parse(data.toString()));
      } catch {
        frames.push(data.toString());
      }
      if (frames.length >= maxFrames) {
        finish();
      }
    });

    ws.on('close', () => finish());
    ws.on('error', (error) => finish(error));
  });
}

function summarizeFailure(result) {
  if (!result) return 'missing result';
  if (typeof result.body === 'string') return result.body;
  return result.body?.error ?? result.body;
}

function hasVisibleSessions(sessionsResult) {
  return Array.isArray(sessionsResult?.body?.data?.sessions)
    && sessionsResult.body.data.sessions.length > 0;
}

function pickSessionId(configuredSessionId, sessionsResult) {
  if (configuredSessionId) return configuredSessionId;
  return sessionsResult?.body?.data?.sessions?.[0]?.session_id ?? '';
}

function isAcceptableAbortStatus(result) {
  if (!result) return false;
  if (result.status === 202) return true;
  return result.status === 404 && result.body?.error?.code === 'RUN_NOT_FOUND';
}

function isGatewayNotRunning(result) {
  return result?.body?.error?.code === 'GATEWAY_NOT_RUNNING';
}

function sleep(ms) {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

function printStep(name, result) {
  const ok = result.ok ? 'PASS' : 'FAIL';
  const extra = result.note ? ` - ${result.note}` : '';
  console.log(`[${ok}] ${name}${extra}`);
}

async function main() {
  const options = parseArgs(process.argv.slice(2));
  const baseUrl = normalizeBaseUrl(options.baseUrl);
  const wsBase = new URL(baseUrl.toString());
  wsBase.protocol = wsBase.protocol === 'https:' ? 'wss:' : 'ws:';

  const summary = [];
  const results = {};

  const nodeInfo = await requestJson(baseUrl, options.token, options.callerId, 'GET', '/node/info');
  results.nodeInfo = nodeInfo;
  summary.push({
    name: 'GET /node/info',
    ok: nodeInfo.status === 200,
    note: nodeInfo.status === 200 ? nodeInfo.body?.data?.node_id : JSON.stringify(summarizeFailure(nodeInfo)),
  });

  const nodeHealth = await requestJson(baseUrl, options.token, options.callerId, 'GET', '/node/health');
  results.nodeHealth = nodeHealth;
  summary.push({
    name: 'GET /node/health',
    ok: nodeHealth.status === 200,
    note: nodeHealth.status === 200
      ? `gateway_running=${nodeHealth.body?.data?.gateway_running} runtime_ready=${nodeHealth.body?.data?.runtime_ready}`
      : JSON.stringify(summarizeFailure(nodeHealth)),
  });

  const runtimeStatus = await requestJson(baseUrl, options.token, options.callerId, 'GET', '/runtime/status');
  results.runtimeStatus = runtimeStatus;
  summary.push({
    name: 'GET /runtime/status',
    ok: runtimeStatus.status === 200,
    note: runtimeStatus.status === 200
      ? `connection_status=${runtimeStatus.body?.data?.connection_status}`
      : JSON.stringify(summarizeFailure(runtimeStatus)),
  });

  const runtimeCapabilities = await requestJson(baseUrl, options.token, options.callerId, 'GET', '/runtime/capabilities');
  results.runtimeCapabilities = runtimeCapabilities;
  summary.push({
    name: 'GET /runtime/capabilities',
    ok: runtimeCapabilities.status === 200,
    note: runtimeCapabilities.status === 200
      ? `tools=${runtimeCapabilities.body?.data?.enabled_tools?.length ?? 0}`
      : JSON.stringify(summarizeFailure(runtimeCapabilities)),
  });

  let sessions = await requestJson(baseUrl, options.token, options.callerId, 'GET', '/sessions');
  let sessionId = pickSessionId(options.sessionId, sessions);
  results.resolvedSessionId = sessionId || null;

  if (sessionId && options.withWrite && isGatewayNotRunning(sessions)) {
    const warmupSend = await requestJson(
      baseUrl,
      options.token,
      options.callerId,
      'POST',
      `/sessions/${encodeSessionId(sessionId)}/send`,
      { message: 'Bridge warmup. Reply with a short ok.' },
    );
    results.warmupSend = warmupSend;
    summary.push({
      name: 'POST /sessions/{id}/send (warmup)',
      ok: warmupSend.status === 202,
      note: warmupSend.status === 202
        ? `run_id=${warmupSend.body?.data?.run_id ?? '-'}`
        : JSON.stringify(summarizeFailure(warmupSend)),
    });
    await sleep(600);
    sessions = await requestJson(baseUrl, options.token, options.callerId, 'GET', '/sessions');
  }

  results.sessions = sessions;
  summary.push({
    name: 'GET /sessions',
    ok: sessions.status === 200,
    note: sessions.status === 200
      ? `count=${sessions.body?.data?.sessions?.length ?? 0}`
      : JSON.stringify(summarizeFailure(sessions)),
  });

  sessionId = pickSessionId(sessionId, sessions);
  results.resolvedSessionId = sessionId || null;

  if (sessionId) {
    const detail = await requestJson(
      baseUrl,
      options.token,
      options.callerId,
      'GET',
      `/sessions/${encodeSessionId(sessionId)}`,
    );
    results.sessionDetail = detail;
    summary.push({
      name: 'GET /sessions/{id}',
      ok: detail.status === 200,
      note: detail.status === 200
        ? detail.body?.data?.session?.state
        : JSON.stringify(summarizeFailure(detail)),
    });

    const history = await requestJson(
      baseUrl,
      options.token,
      options.callerId,
      'GET',
      `/sessions/${encodeSessionId(sessionId)}/history?limit=${options.historyLimit}`,
    );
    results.sessionHistory = history;
    summary.push({
      name: 'GET /sessions/{id}/history',
      ok: history.status === 200,
      note: history.status === 200
        ? `messages=${history.body?.data?.messages?.length ?? 0}`
        : JSON.stringify(summarizeFailure(history)),
    });

    if (options.withWrite) {
      let send;
      const wsInitial = await collectWsFramesAfterAction(
        `${endpoint(wsBase, '/events').toString()}?session_id=${encodeURIComponent(sessionId)}`,
        options.token,
        options.callerId,
        options.wsTimeoutMs,
        async () => {
          send = await requestJson(
            baseUrl,
            options.token,
            options.callerId,
            'POST',
            `/sessions/${encodeSessionId(sessionId)}/send`,
            { message: options.message },
          );
        },
        4,
      );

      results.send = send;
      summary.push({
        name: 'POST /sessions/{id}/send',
        ok: send.status === 202,
        note: send.status === 202
          ? `run_id=${send.body?.data?.run_id ?? '-'}`
          : JSON.stringify(summarizeFailure(send)),
      });

      results.wsInitial = wsInitial;
      summary.push({
        name: 'WS /events',
        ok: wsInitial.length > 0,
        note: wsInitial.length > 0 ? `frames=${wsInitial.length}` : 'no frames',
      });

      const replayCursor = wsInitial[0]?.event_id ?? null;
      results.wsReplayCursor = replayCursor;
      if (replayCursor) {
        const wsReplay = await collectWsFrames(
          `${endpoint(wsBase, '/events').toString()}?session_id=${encodeURIComponent(sessionId)}&last_event_id=${encodeURIComponent(replayCursor)}`,
          options.token,
          options.callerId,
          options.wsTimeoutMs,
          2,
        );
        results.wsReplay = wsReplay;
        summary.push({
          name: 'WS /events?last_event_id=...',
          ok: true,
          note: `frames=${wsReplay.length}`,
        });
      }

      const abortSend = await requestJson(
        baseUrl,
        options.token,
        options.callerId,
        'POST',
        `/sessions/${encodeSessionId(sessionId)}/send`,
        { message: options.abortMessage },
      );
      results.abortSend = abortSend;
      summary.push({
        name: 'POST /sessions/{id}/send (abort smoke)',
        ok: abortSend.status === 202,
        note: abortSend.status === 202
          ? `run_id=${abortSend.body?.data?.run_id ?? '-'}`
          : JSON.stringify(summarizeFailure(abortSend)),
      });

      await sleep(options.abortDelayMs);

      const abort = await requestJson(
        baseUrl,
        options.token,
        options.callerId,
        'POST',
        `/sessions/${encodeSessionId(sessionId)}/abort`,
      );
      results.abort = abort;
      summary.push({
        name: 'POST /sessions/{id}/abort',
        ok: isAcceptableAbortStatus(abort),
        note: abort.status === 202
          ? 'accepted'
          : abort.body?.error?.code ?? JSON.stringify(summarizeFailure(abort)),
      });
    }
  } else {
    summary.push({
      name: 'Session-dependent endpoints',
      ok: false,
      note: 'No visible session available. Provide --session-id or create a session first.',
    });
  }

  console.log('\nClawy Bridge smoke summary');
  console.log('==========================');
  for (const step of summary) {
    printStep(step.name, step);
  }

  console.log('\nRaw results');
  console.log('===========');
  console.log(JSON.stringify(results, null, 2));

  const failed = summary.filter((step) => !step.ok);
  process.exit(failed.length > 0 ? 1 : 0);
}

main().catch((error) => {
  console.error(error instanceof Error ? error.message : String(error));
  process.exit(1);
});
