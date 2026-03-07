import { dirname, join } from 'node:path';
import { homedir } from 'node:os';
import { createRequire } from 'node:module';
import { deflateSync } from 'node:zlib';
import { existsSync, mkdirSync, realpathSync, rmSync } from 'node:fs';

const require = createRequire(import.meta.url);
const accountId = process.argv[2] || 'default';

function emit(type, payload = {}) {
  process.stdout.write(`${JSON.stringify({ type, ...payload })}\n`);
}

function log(...args) {
  process.stderr.write(`${args.map(String).join(' ')}\n`);
}

function sleep(ms) {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

function resolveOpenClawDir() {
  const direct = process.env.OPENCLAW_DIR || join(process.cwd(), 'node_modules', 'openclaw');
  try {
    return realpathSync(direct);
  } catch {
    return direct;
  }
}

function resolveOpenClawPackageJson(openclawRequire, packageName) {
  return openclawRequire.resolve(`${packageName}/package.json`);
}

const openclawDir = resolveOpenClawDir();
const openclawRequire = createRequire(join(openclawDir, 'package.json'));
const baileysPath = dirname(resolveOpenClawPackageJson(openclawRequire, '@whiskeysockets/baileys'));
const qrcodeTerminalPath = dirname(resolveOpenClawPackageJson(openclawRequire, 'qrcode-terminal'));
const baileysRequire = createRequire(join(baileysPath, 'package.json'));

const {
  default: makeWASocket,
  useMultiFileAuthState: initAuth,
  DisconnectReason,
  fetchLatestBaileysVersion,
} = require(baileysPath);

const QRCodeModule = require(join(qrcodeTerminalPath, 'vendor', 'QRCode', 'index.js'));
const QRErrorCorrectLevelModule = require(join(
  qrcodeTerminalPath,
  'vendor',
  'QRCode',
  'QRErrorCorrectLevel.js',
));

const QRCode = QRCodeModule;
const QRErrorCorrectLevel = QRErrorCorrectLevelModule;

let active = true;
let retryCount = 0;
let socket = null;
let reconnectTimer = null;

function createQrMatrix(input) {
  const qr = new QRCode(-1, QRErrorCorrectLevel.L);
  qr.addData(input);
  qr.make();
  return qr;
}

function fillPixel(buffer, x, y, width, r, g, b, a = 255) {
  const idx = (y * width + x) * 4;
  buffer[idx] = r;
  buffer[idx + 1] = g;
  buffer[idx + 2] = b;
  buffer[idx + 3] = a;
}

function crcTable() {
  const table = new Uint32Array(256);
  for (let i = 0; i < 256; i += 1) {
    let c = i;
    for (let k = 0; k < 8; k += 1) {
      c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1;
    }
    table[i] = c >>> 0;
  }
  return table;
}

const CRC_TABLE = crcTable();

function crc32(buffer) {
  let crc = 0xffffffff;
  for (let i = 0; i < buffer.length; i += 1) {
    crc = CRC_TABLE[(crc ^ buffer[i]) & 0xff] ^ (crc >>> 8);
  }
  return (crc ^ 0xffffffff) >>> 0;
}

function pngChunk(type, data) {
  const typeBuffer = Buffer.from(type, 'ascii');
  const length = Buffer.alloc(4);
  length.writeUInt32BE(data.length, 0);
  const crc = crc32(Buffer.concat([typeBuffer, data]));
  const crcBuffer = Buffer.alloc(4);
  crcBuffer.writeUInt32BE(crc, 0);
  return Buffer.concat([length, typeBuffer, data, crcBuffer]);
}

function encodePngRgba(buffer, width, height) {
  const stride = width * 4;
  const raw = Buffer.alloc((stride + 1) * height);
  for (let row = 0; row < height; row += 1) {
    const rawOffset = row * (stride + 1);
    raw[rawOffset] = 0;
    buffer.copy(raw, rawOffset + 1, row * stride, row * stride + stride);
  }
  const compressed = deflateSync(raw);

  const signature = Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]);
  const ihdr = Buffer.alloc(13);
  ihdr.writeUInt32BE(width, 0);
  ihdr.writeUInt32BE(height, 4);
  ihdr[8] = 8;
  ihdr[9] = 6;
  ihdr[10] = 0;
  ihdr[11] = 0;
  ihdr[12] = 0;

  return Buffer.concat([
    signature,
    pngChunk('IHDR', ihdr),
    pngChunk('IDAT', compressed),
    pngChunk('IEND', Buffer.alloc(0)),
  ]);
}

async function renderQrPngBase64(input, options = {}) {
  const { scale = 6, marginModules = 4 } = options;
  const qr = createQrMatrix(input);
  const modules = qr.getModuleCount();
  const size = (modules + marginModules * 2) * scale;
  const buffer = Buffer.alloc(size * size * 4, 255);

  for (let row = 0; row < modules; row += 1) {
    for (let col = 0; col < modules; col += 1) {
      if (!qr.isDark(row, col)) {
        continue;
      }
      const startX = (col + marginModules) * scale;
      const startY = (row + marginModules) * scale;
      for (let y = 0; y < scale; y += 1) {
        const pixelY = startY + y;
        for (let x = 0; x < scale; x += 1) {
          const pixelX = startX + x;
          fillPixel(buffer, pixelX, pixelY, size, 0, 0, 0, 255);
        }
      }
    }
  }

  return encodePngRgba(buffer, size, size).toString('base64');
}

function getLoggerFactory() {
  try {
    return baileysRequire('pino');
  } catch {
    try {
      return require('pino');
    } catch {
      return () => ({
        trace() {},
        debug() {},
        info() {},
        warn() {},
        error() {},
        fatal() {},
        child() {
          return this;
        },
      });
    }
  }
}

async function stop() {
  active = false;
  if (reconnectTimer) {
    clearTimeout(reconnectTimer);
    reconnectTimer = null;
  }
  if (!socket) {
    return;
  }
  try {
    socket.ev.removeAllListeners('connection.update');
    socket.ev.removeAllListeners('creds.update');
    try {
      socket.ws?.close();
    } catch {
      // ignore
    }
    socket.end(undefined);
  } catch {
    // ignore
  }
  socket = null;
}

async function finishLogin(targetAccountId) {
  if (!active) {
    return;
  }
  await stop();
  await sleep(5000);
  emit('success', { accountId: targetAccountId });
  process.exit(0);
}

async function connectToWhatsApp(targetAccountId) {
  if (!active) {
    return;
  }

  const authDir = join(homedir(), '.openclaw', 'credentials', 'whatsapp', targetAccountId);
  if (!existsSync(authDir)) {
    mkdirSync(authDir, { recursive: true });
  }

  try {
    const pino = getLoggerFactory();
    const { state, saveCreds } = await initAuth(authDir);
    const { version } = await fetchLatestBaileysVersion();

    socket = makeWASocket({
      version,
      auth: state,
      printQRInTerminal: false,
      logger: pino({ level: 'silent' }),
      connectTimeoutMs: 60000,
    });

    let connectionOpened = false;
    let credsReceived = false;
    let credsTimeout = null;

    socket.ev.on('creds.update', async () => {
      await saveCreds();
      if (connectionOpened && !credsReceived) {
        credsReceived = true;
        if (credsTimeout) {
          clearTimeout(credsTimeout);
        }
        await sleep(3000);
        await finishLogin(targetAccountId);
      }
    });

    socket.ev.on('connection.update', async (update) => {
      try {
        const { connection, lastDisconnect, qr } = update;

        if (qr) {
          const png = await renderQrPngBase64(qr);
          emit('qr', { qr: png, raw: qr });
        }

        if (connection === 'close') {
          const error = lastDisconnect?.error;
          const statusCode = error?.output?.statusCode;
          const isLoggedOut = statusCode === DisconnectReason.loggedOut;
          const shouldReconnect = !isLoggedOut || retryCount < 2;

          if (shouldReconnect && active) {
            if (retryCount < 5) {
              retryCount += 1;
              reconnectTimer = setTimeout(() => {
                reconnectTimer = null;
                void connectToWhatsApp(targetAccountId);
              }, 1000);
              return;
            }

            active = false;
            emit('error', { message: 'Connection failed after multiple retries' });
            process.exit(1);
            return;
          }

          active = false;
          if (statusCode === DisconnectReason.loggedOut) {
            try {
              rmSync(authDir, { recursive: true, force: true });
            } catch (removeError) {
              log('[WhatsAppLogin] Failed to clear auth dir', removeError);
            }
          }
          if (socket) {
            socket.end(undefined);
            socket = null;
          }
          emit('error', { message: 'Logged out' });
          process.exit(1);
          return;
        }

        if (connection === 'open') {
          retryCount = 0;
          connectionOpened = true;
          credsTimeout = setTimeout(async () => {
            if (!credsReceived && active) {
              await finishLogin(targetAccountId);
            }
          }, 15000);
        }
      } catch (error) {
        emit('error', {
          message: error instanceof Error ? error.message : String(error),
        });
        process.exit(1);
      }
    });
  } catch (error) {
    if (active && retryCount < 5) {
      retryCount += 1;
      reconnectTimer = setTimeout(() => {
        reconnectTimer = null;
        void connectToWhatsApp(targetAccountId);
      }, 2000);
      return;
    }
    emit('error', {
      message: error instanceof Error ? error.message : String(error),
    });
    process.exit(1);
  }
}

process.on('SIGINT', () => {
  void stop().finally(() => process.exit(0));
});
process.on('SIGTERM', () => {
  void stop().finally(() => process.exit(0));
});

void connectToWhatsApp(accountId);
