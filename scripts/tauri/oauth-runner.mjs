import { createHash, randomBytes, randomUUID } from 'node:crypto';
import { existsSync } from 'node:fs';
import http from 'node:http';
import path from 'node:path';
import { pathToFileURL } from 'node:url';

const MINIMAX_SCOPE = 'group_id profile model.completion';
const MINIMAX_GRANT_TYPE = 'urn:ietf:params:oauth:grant-type:user_code';
const QWEN_BASE_URL = 'https://chat.qwen.ai';
const QWEN_DEVICE_CODE_ENDPOINT = `${QWEN_BASE_URL}/api/v1/oauth2/device/code`;
const QWEN_TOKEN_ENDPOINT = `${QWEN_BASE_URL}/api/v1/oauth2/token`;
const QWEN_CLIENT_ID = 'f0304373b74a44d2b584a3fb70ca9e56';
const QWEN_SCOPE = 'openid profile email model.completion';
const QWEN_GRANT_TYPE = 'urn:ietf:params:oauth:grant-type:device_code';
const OPENAI_CODEX_CLIENT_ID = 'app_EMoamEEZ73f0CkXaXp7hrann';
const OPENAI_CODEX_AUTHORIZE_URL = 'https://auth.openai.com/oauth/authorize';
const OPENAI_CODEX_TOKEN_URL = 'https://auth.openai.com/oauth/token';
const OPENAI_CODEX_REDIRECT_URI = 'http://localhost:1455/auth/callback';
const OPENAI_CODEX_SCOPE = 'openid profile email offline_access';
const OPENAI_CODEX_JWT_CLAIM_PATH = 'https://api.openai.com/auth';
const OPENAI_CODEX_PRECHECK_URL =
  'https://auth.openai.com/oauth/authorize?response_type=code&client_id=clawy-preflight&redirect_uri=http%3A%2F%2Flocalhost%3A1455%2Fauth%2Fcallback&scope=openid+profile+email';
const OPENAI_CODEX_SUCCESS_HTML = `<!doctype html>
<html lang="en">
<head>
  <meta charset="utf-8" />
  <meta name="viewport" content="width=device-width, initial-scale=1" />
  <title>Authentication successful</title>
</head>
<body>
  <p>Authentication successful. Return to Clawy to continue.</p>
</body>
</html>`;
const TLS_CERT_ERROR_CODES = new Set([
  'UNABLE_TO_GET_ISSUER_CERT_LOCALLY',
  'UNABLE_TO_VERIFY_LEAF_SIGNATURE',
  'CERT_HAS_EXPIRED',
  'DEPTH_ZERO_SELF_SIGNED_CERT',
  'SELF_SIGNED_CERT_IN_CHAIN',
  'ERR_TLS_CERT_ALTNAME_INVALID',
]);
const TLS_CERT_ERROR_PATTERNS = [
  /unable to get local issuer certificate/i,
  /unable to verify the first certificate/i,
  /self[- ]signed certificate/i,
  /certificate has expired/i,
];

const provider = process.argv[2];
const minimaxRegion = process.argv[3] || (provider === 'minimax-portal-cn' ? 'cn' : 'global');
let pendingPromptResolve = null;
let pendingPromptReject = null;
let promptBuffer = '';

function emit(type, payload = {}) {
  process.stdout.write(`${JSON.stringify({ type, ...payload })}\n`);
}

function fail(message) {
  emit('error', { message });
  process.exitCode = 1;
}

function attachPromptInputBridge() {
  if (provider !== 'openai-codex') {
    return;
  }

  process.stdin.setEncoding('utf8');
  process.stdin.on('data', (chunk) => {
    promptBuffer += chunk;
    let newlineIndex = promptBuffer.indexOf('\n');
    while (newlineIndex !== -1) {
      const line = promptBuffer.slice(0, newlineIndex).replace(/\r$/, '');
      promptBuffer = promptBuffer.slice(newlineIndex + 1);
      if (pendingPromptResolve) {
        const resolve = pendingPromptResolve;
        pendingPromptResolve = null;
        pendingPromptReject = null;
        resolve(line);
      }
      newlineIndex = promptBuffer.indexOf('\n');
    }
  });
  process.stdin.on('end', () => {
    if (pendingPromptReject) {
      const reject = pendingPromptReject;
      pendingPromptResolve = null;
      pendingPromptReject = null;
      reject(new Error('OAuth input stream closed before receiving a response.'));
    }
  });
}

function waitForPromptInput() {
  return new Promise((resolve, reject) => {
    pendingPromptResolve = resolve;
    pendingPromptReject = reject;
  });
}

async function loadCodexOAuthModule() {
  const runtimeDir = process.env.OPENCLAW_RUNTIME_DIR;
  const candidatePaths = [
    runtimeDir
      ? path.join(runtimeDir, 'node_modules/@mariozechner/pi-ai/dist/utils/oauth/index.js')
      : null,
    path.join(process.cwd(), 'node_modules/openclaw/node_modules/@mariozechner/pi-ai/dist/utils/oauth/index.js'),
    path.join(process.cwd(), 'node_modules/@mariozechner/pi-ai/dist/utils/oauth/index.js'),
  ].filter(Boolean);

  for (const candidate of candidatePaths) {
    if (existsSync(candidate)) {
      return import(pathToFileURL(candidate).href);
    }
  }

  throw new Error(
    `Unable to locate the OpenAI Codex OAuth module. Tried: ${candidatePaths.join(', ')}`
  );
}

function sleep(ms) {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

function toFormUrlEncoded(data) {
  return Object.entries(data)
    .map(([key, value]) => `${encodeURIComponent(key)}=${encodeURIComponent(value)}`)
    .join('&');
}

function generatePkce() {
  const verifier = randomBytes(32).toString('base64url');
  const challenge = createHash('sha256').update(verifier).digest('base64url');
  return { verifier, challenge };
}

function generatePkceWithState() {
  const { verifier, challenge } = generatePkce();
  return {
    verifier,
    challenge,
    state: randomBytes(16).toString('base64url'),
  };
}

function createOpenAICodexState() {
  return randomBytes(16).toString('hex');
}

function decodeJwt(token) {
  try {
    const parts = token.split('.');
    if (parts.length !== 3) {
      return null;
    }
    const normalized = parts[1].replace(/-/g, '+').replace(/_/g, '/');
    const padded = normalized + '='.repeat((4 - (normalized.length % 4 || 4)) % 4);
    return JSON.parse(Buffer.from(padded, 'base64').toString('utf8'));
  } catch {
    return null;
  }
}

function extractOpenAICodexAccountId(accessToken) {
  const payload = decodeJwt(accessToken);
  const auth = payload?.[OPENAI_CODEX_JWT_CLAIM_PATH];
  const accountId = auth?.chatgpt_account_id;
  return typeof accountId === 'string' && accountId.length > 0 ? accountId : null;
}

function extractFailure(error) {
  const root = error && typeof error === 'object' ? error : null;
  const rootCause = root?.cause && typeof root.cause === 'object' ? root.cause : null;
  const code = typeof rootCause?.code === 'string' ? rootCause.code : undefined;
  const message =
    typeof rootCause?.message === 'string'
      ? rootCause.message
      : typeof root?.message === 'string'
        ? root.message
        : String(error);

  return {
    code,
    message,
    kind:
      (code ? TLS_CERT_ERROR_CODES.has(code) : false)
      || TLS_CERT_ERROR_PATTERNS.some((pattern) => pattern.test(message))
        ? 'tls-cert'
        : 'network',
  };
}

async function runOpenAICodexTlsPreflight() {
  try {
    await fetch(OPENAI_CODEX_PRECHECK_URL, {
      method: 'GET',
      redirect: 'manual',
      signal: AbortSignal.timeout(5000),
    });
    return { ok: true };
  } catch (error) {
    const failure = extractFailure(error);
    return { ok: false, ...failure };
  }
}

function formatOpenAICodexTlsFix(result) {
  if (result.kind !== 'tls-cert') {
    return `OpenAI OAuth prerequisites check failed before the browser flow: ${result.message}. Verify DNS, firewall, or proxy access to auth.openai.com and try again.`;
  }

  return `OpenAI OAuth prerequisites check failed because Node/OpenSSL could not validate TLS certificates (${result.code || 'tls-cert'}: ${result.message}). On Homebrew Node, run: brew postinstall ca-certificates && brew postinstall openssl@3, then try again.`;
}

function createOpenAICodexAuthorizationFlow(originator = 'pi') {
  const { verifier, challenge } = generatePkce();
  const state = createOpenAICodexState();
  const url = new URL(OPENAI_CODEX_AUTHORIZE_URL);
  url.searchParams.set('response_type', 'code');
  url.searchParams.set('client_id', OPENAI_CODEX_CLIENT_ID);
  url.searchParams.set('redirect_uri', OPENAI_CODEX_REDIRECT_URI);
  url.searchParams.set('scope', OPENAI_CODEX_SCOPE);
  url.searchParams.set('code_challenge', challenge);
  url.searchParams.set('code_challenge_method', 'S256');
  url.searchParams.set('state', state);
  url.searchParams.set('id_token_add_organizations', 'true');
  url.searchParams.set('codex_cli_simplified_flow', 'true');
  url.searchParams.set('originator', originator);
  return { verifier, state, url: url.toString() };
}

function parseAuthorizationInput(input) {
  const value = input.trim();
  if (!value) {
    return {};
  }

  try {
    const url = new URL(value);
    return {
      code: url.searchParams.get('code') ?? undefined,
      state: url.searchParams.get('state') ?? undefined,
    };
  } catch {
    // Ignore malformed URLs and fall back below.
  }

  if (value.includes('#')) {
    const [code, state] = value.split('#', 2);
    return { code, state };
  }

  if (value.includes('code=')) {
    const params = new URLSearchParams(value);
    return {
      code: params.get('code') ?? undefined,
      state: params.get('state') ?? undefined,
    };
  }

  return { code: value };
}

function startOpenAICodexCallbackServer(expectedState) {
  let lastCode = null;
  let lastCallbackUrl = null;
  let cancelled = false;
  const server = http.createServer((req, res) => {
    try {
      const url = new URL(req.url || '', 'http://localhost');
      if (url.pathname !== '/auth/callback') {
        res.statusCode = 404;
        res.end('Not found');
        return;
      }
      if (url.searchParams.get('state') !== expectedState) {
        res.statusCode = 400;
        res.end('State mismatch');
        return;
      }

      const code = url.searchParams.get('code');
      if (!code) {
        res.statusCode = 400;
        res.end('Missing authorization code');
        return;
      }

      lastCode = code;
      lastCallbackUrl = `${OPENAI_CODEX_REDIRECT_URI}?${url.searchParams.toString()}`;
      res.statusCode = 200;
      res.setHeader('Content-Type', 'text/html; charset=utf-8');
      res.end(OPENAI_CODEX_SUCCESS_HTML);
    } catch {
      res.statusCode = 500;
      res.end('Internal error');
    }
  });

  return new Promise((resolve) => {
    server
      .listen(1455, '127.0.0.1', () => {
        resolve({
          close: () => server.close(),
          cancelWait: () => {
            cancelled = true;
          },
          waitForCode: async () => {
            const startedAt = Date.now();
            while (Date.now() - startedAt < 600_000) {
              if (lastCode) {
                return { code: lastCode, callbackUrl: lastCallbackUrl };
              }
              if (cancelled) {
                return null;
              }
              await sleep(100);
            }
            return null;
          },
        });
      })
      .on('error', (err) => {
        emit('progress', {
          provider: 'openai-codex',
          message: `Local callback unavailable (${err.code || 'listen-failed'}). Waiting for pasted redirect URL...`,
        });
        resolve({
          close: () => {
            try {
              server.close();
            } catch {
              // Ignore close errors.
            }
          },
          cancelWait: () => {},
          waitForCode: async () => null,
        });
      });
  });
}

async function exchangeOpenAICodexAuthorizationCode(code, verifier) {
  const maxAttempts = 3;
  let lastError = 'Token exchange failed';

  for (let attempt = 1; attempt <= maxAttempts; attempt += 1) {
    let response;
    try {
      response = await fetch(OPENAI_CODEX_TOKEN_URL, {
        method: 'POST',
        headers: { 'Content-Type': 'application/x-www-form-urlencoded' },
        body: new URLSearchParams({
          grant_type: 'authorization_code',
          client_id: OPENAI_CODEX_CLIENT_ID,
          code,
          code_verifier: verifier,
          redirect_uri: OPENAI_CODEX_REDIRECT_URI,
        }),
      });
    } catch (error) {
      const detail = extractFailure(error);
      if (attempt === maxAttempts) {
        return { ok: false, error: `Token exchange failed: ${detail.message}` };
      }
      await sleep(250 * attempt);
      continue;
    }

    const text = await response.text().catch(() => '');
    let payload = null;
    if (text) {
      try {
        payload = JSON.parse(text);
      } catch {
        payload = null;
      }
    }

    if (!response.ok) {
      const detail =
        payload?.error_description
        || payload?.error
        || text
        || response.statusText
        || `HTTP ${response.status}`;
      lastError = `Token exchange failed (${response.status}): ${detail}`;
      if (response.status >= 500 || response.status === 429) {
        if (attempt < maxAttempts) {
          await sleep(250 * attempt);
          continue;
        }
      }
      return { ok: false, error: lastError };
    }

    if (!payload?.access_token || !payload?.refresh_token || typeof payload?.expires_in !== 'number') {
      lastError = 'Token exchange failed: OAuth token response was missing required fields';
      if (attempt < maxAttempts) {
        await sleep(250 * attempt);
        continue;
      }
      return { ok: false, error: lastError };
    }

    return {
      ok: true,
      token: {
        access: payload.access_token,
        refresh: payload.refresh_token,
        expires: Date.now() + payload.expires_in * 1000,
      },
    };
  }

  return { ok: false, error: lastError };
}

function minimaxEndpoints(region) {
  const baseUrl = region === 'cn' ? 'https://api.minimaxi.com' : 'https://api.minimax.io';
  return {
    baseUrl,
    clientId: '78257093-7e40-4613-99e0-527b14b39113',
    codeEndpoint: `${baseUrl}/oauth/code`,
    tokenEndpoint: `${baseUrl}/oauth/token`,
  };
}

async function requestMiniMaxCode(region, challenge, state) {
  const endpoints = minimaxEndpoints(region);
  const response = await fetch(endpoints.codeEndpoint, {
    method: 'POST',
    headers: {
      'Content-Type': 'application/x-www-form-urlencoded',
      Accept: 'application/json',
      'x-request-id': randomUUID(),
    },
    body: toFormUrlEncoded({
      response_type: 'code',
      client_id: endpoints.clientId,
      scope: MINIMAX_SCOPE,
      code_challenge: challenge,
      code_challenge_method: 'S256',
      state,
    }),
  });

  if (!response.ok) {
    const text = await response.text();
    throw new Error(`MiniMax OAuth authorization failed: ${text || response.statusText}`);
  }

  const payload = await response.json();
  if (!payload.user_code || !payload.verification_uri) {
    throw new Error(
      payload.error || 'MiniMax OAuth authorization returned an incomplete payload.',
    );
  }
  if (payload.state !== state) {
    throw new Error('MiniMax OAuth state mismatch.');
  }
  return payload;
}

async function pollMiniMaxToken(region, userCode, verifier) {
  const endpoints = minimaxEndpoints(region);
  const response = await fetch(endpoints.tokenEndpoint, {
    method: 'POST',
    headers: {
      'Content-Type': 'application/x-www-form-urlencoded',
      Accept: 'application/json',
    },
    body: toFormUrlEncoded({
      grant_type: MINIMAX_GRANT_TYPE,
      client_id: endpoints.clientId,
      user_code: userCode,
      code_verifier: verifier,
    }),
  });

  const text = await response.text();
  let payload;
  if (text) {
    try {
      payload = JSON.parse(text);
    } catch {
      payload = undefined;
    }
  }

  if (!response.ok) {
    return {
      status: 'error',
      message: payload?.base_resp?.status_msg || text || 'MiniMax OAuth failed.',
    };
  }

  if (!payload) {
    return { status: 'error', message: 'MiniMax OAuth failed to parse response.' };
  }

  if (payload.status === 'error') {
    return { status: 'error', message: 'An error occurred. Please try again later.' };
  }

  if (payload.status !== 'success') {
    return { status: 'pending', message: 'current user code is not authorized' };
  }

  if (!payload.access_token || !payload.refresh_token || !payload.expired_in) {
    return { status: 'error', message: 'MiniMax OAuth returned incomplete token payload.' };
  }

  return {
    status: 'success',
    token: {
      access: payload.access_token,
      refresh: payload.refresh_token,
      expires: payload.expired_in,
      resourceUrl: payload.resource_url,
      api: 'anthropic-messages',
      region,
    },
  };
}

async function runMiniMax(region) {
  const { verifier, challenge, state } = generatePkceWithState();
  const oauth = await requestMiniMaxCode(region, challenge, state);
  const expiresIn = oauth.expired_in > Date.now()
    ? Math.max(1, Math.round((oauth.expired_in - Date.now()) / 1000))
    : 300;

  emit('open-url', { url: oauth.verification_uri });
  emit('code', {
    provider,
    verificationUri: oauth.verification_uri,
    userCode: oauth.user_code,
    expiresIn,
  });

  let pollIntervalMs = oauth.interval || 2000;
  const expireTimeMs = oauth.expired_in || Date.now() + 300_000;

  while (Date.now() < expireTimeMs) {
    const result = await pollMiniMaxToken(region, oauth.user_code, verifier);
    if (result.status === 'success') {
      emit('success', { provider, token: result.token });
      return;
    }
    if (result.status === 'error') {
      throw new Error(`MiniMax OAuth failed: ${result.message}`);
    }

    pollIntervalMs = Math.min(Math.round(pollIntervalMs * 1.5), 10_000);
    await sleep(pollIntervalMs);
  }

  throw new Error('MiniMax OAuth timed out waiting for authorization.');
}

async function requestQwenCode(challenge) {
  const response = await fetch(QWEN_DEVICE_CODE_ENDPOINT, {
    method: 'POST',
    headers: {
      'Content-Type': 'application/x-www-form-urlencoded',
      Accept: 'application/json',
      'x-request-id': randomUUID(),
    },
    body: toFormUrlEncoded({
      client_id: QWEN_CLIENT_ID,
      scope: QWEN_SCOPE,
      code_challenge: challenge,
      code_challenge_method: 'S256',
    }),
  });

  if (!response.ok) {
    const text = await response.text();
    throw new Error(`Qwen device authorization failed: ${text || response.statusText}`);
  }

  const payload = await response.json();
  if (!payload.device_code || !payload.user_code || !payload.verification_uri) {
    throw new Error(payload.error || 'Qwen device authorization returned an incomplete payload.');
  }
  return payload;
}

async function pollQwenToken(deviceCode, verifier) {
  const response = await fetch(QWEN_TOKEN_ENDPOINT, {
    method: 'POST',
    headers: {
      'Content-Type': 'application/x-www-form-urlencoded',
      Accept: 'application/json',
    },
    body: toFormUrlEncoded({
      grant_type: QWEN_GRANT_TYPE,
      client_id: QWEN_CLIENT_ID,
      device_code: deviceCode,
      code_verifier: verifier,
    }),
  });

  if (!response.ok) {
    let payload;
    try {
      payload = await response.json();
    } catch {
      payload = undefined;
    }

    if (payload?.error === 'authorization_pending') {
      return { status: 'pending', slowDown: false };
    }
    if (payload?.error === 'slow_down') {
      return { status: 'pending', slowDown: true };
    }
    return {
      status: 'error',
      message: payload?.error_description || payload?.error || response.statusText,
    };
  }

  const payload = await response.json();
  if (!payload.access_token || !payload.refresh_token || !payload.expires_in) {
    return { status: 'error', message: 'Qwen OAuth returned incomplete token payload.' };
  }

  return {
    status: 'success',
    token: {
      access: payload.access_token,
      refresh: payload.refresh_token,
      expires: Date.now() + payload.expires_in * 1000,
      resourceUrl: payload.resource_url,
      api: 'openai-completions',
    },
  };
}

async function runQwen() {
  const { verifier, challenge } = generatePkce();
  const device = await requestQwenCode(challenge);
  const verificationUrl = device.verification_uri_complete || device.verification_uri;

  emit('open-url', { url: verificationUrl });
  emit('code', {
    provider,
    verificationUri: verificationUrl,
    userCode: device.user_code,
    expiresIn: device.expires_in || 300,
  });

  const startedAtMs = Date.now();
  const timeoutMs = (device.expires_in || 300) * 1000;
  let pollIntervalMs = (device.interval || 2) * 1000;

  while (Date.now() - startedAtMs < timeoutMs) {
    const result = await pollQwenToken(device.device_code, verifier);
    if (result.status === 'success') {
      emit('success', { provider: 'qwen-portal', token: result.token });
      return;
    }
    if (result.status === 'error') {
      throw new Error(`Qwen OAuth failed: ${result.message}`);
    }

    if (result.slowDown) {
      pollIntervalMs = Math.min(Math.round(pollIntervalMs * 1.5), 10_000);
    }
    await sleep(pollIntervalMs);
  }

  throw new Error('Qwen OAuth timed out waiting for authorization.');
}

async function runOpenAICodex() {
  attachPromptInputBridge();
  // Keep the runtime module lookup so we fail early if the managed OpenClaw runtime
  // does not contain the OAuth implementation Clawy expects.
  await loadCodexOAuthModule();

  const preflight = await runOpenAICodexTlsPreflight();
  if (!preflight.ok) {
    throw new Error(formatOpenAICodexTlsFix(preflight));
  }

  const { verifier, state, url } = createOpenAICodexAuthorizationFlow('pi');
  const server = await startOpenAICodexCallbackServer(state);

  emit('open-url', { url });
  emit('code', {
    provider: 'openai-codex',
    authKind: 'browser-callback',
    verificationUri: url,
    expiresIn: 600,
    instructions:
      'Complete sign-in in your browser. If the callback does not finish automatically, paste the redirect URL below.',
  });

  let code = null;
  try {
    emit('progress', {
      provider: 'openai-codex',
      message: 'Waiting for browser callback…',
    });

    const result = await server.waitForCode();
    if (result?.code) {
      code = result.code;
    }

    if (!code) {
      emit('prompt', {
        provider: 'openai-codex',
        message: 'Paste the authorization code (or full redirect URL):',
        placeholder: 'http://localhost:1455/auth/callback?code=...',
      });
      const input = await waitForPromptInput();
      const parsed = parseAuthorizationInput(input);
      if (parsed.state && parsed.state !== state) {
        throw new Error('State mismatch');
      }
      code = parsed.code ?? null;
    }

    if (!code) {
      throw new Error('Missing authorization code');
    }

    emit('progress', {
      provider: 'openai-codex',
      message: 'Exchanging authorization code…',
    });

    const exchange = await exchangeOpenAICodexAuthorizationCode(code, verifier);
    if (!exchange.ok) {
      throw new Error(exchange.error);
    }

    const accountId = extractOpenAICodexAccountId(exchange.token.access);
    if (!accountId) {
      throw new Error('Failed to extract accountId from token');
    }

    emit('success', {
      provider: 'openai-codex',
      token: {
        access: exchange.token.access,
        refresh: exchange.token.refresh,
        expires: exchange.token.expires,
        accountId,
        api: 'openai-codex-responses',
      },
    });
  } finally {
    server.close();
  }
}

async function main() {
  if (provider === 'minimax-portal' || provider === 'minimax-portal-cn') {
    await runMiniMax(minimaxRegion === 'cn' ? 'cn' : 'global');
    return;
  }
  if (provider === 'qwen-portal') {
    await runQwen();
    return;
  }
  if (provider === 'openai-codex') {
    await runOpenAICodex();
    return;
  }
  throw new Error(`Unsupported OAuth provider: ${provider || 'unknown'}`);
}

main().catch((error) => {
  fail(error instanceof Error ? error.message : String(error));
});
