import { createHash, randomBytes, randomUUID } from 'node:crypto';
import { existsSync } from 'node:fs';
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
  const { loginOpenAICodex } = await loadCodexOAuthModule();
  const token = await loginOpenAICodex({
    originator: 'pi',
    onAuth(info) {
      emit('open-url', { url: info.url });
      emit('code', {
        provider: 'openai-codex',
        authKind: 'browser-callback',
        verificationUri: info.url,
        expiresIn: 600,
        instructions:
          info.instructions
          || 'Complete sign-in in your browser. If the callback does not finish automatically, paste the redirect URL below.',
      });
    },
    onProgress(message) {
      emit('progress', { provider: 'openai-codex', message });
    },
    async onPrompt(prompt) {
      emit('prompt', {
        provider: 'openai-codex',
        message: prompt.message,
        placeholder: prompt.placeholder || 'http://localhost:1455/auth/callback?code=...',
      });
      return waitForPromptInput();
    },
  });

  emit('success', {
    provider: 'openai-codex',
    token: {
      access: token.access,
      refresh: token.refresh,
      expires: token.expires,
      accountId: token.accountId,
      api: 'openai-codex-responses',
    },
  });
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
