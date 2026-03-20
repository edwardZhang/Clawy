#!/usr/bin/env node

import crypto from 'node:crypto';
import fs from 'node:fs/promises';
import path from 'node:path';

function parseArgs(argv) {
  const result = {
    file: '',
    key: '',
    contentType: '',
    cacheControl: '',
    publicBaseUrl: process.env.OSS_PUBLIC_BASE_URL || process.env.ALIYUN_OSS_PUBLIC_BASE_URL || '',
  };

  for (let index = 0; index < argv.length; index += 1) {
    const arg = argv[index];
    const next = argv[index + 1];
    switch (arg) {
      case '--':
        break;
      case '--file':
        result.file = next ?? '';
        index += 1;
        break;
      case '--key':
        result.key = next ?? '';
        index += 1;
        break;
      case '--content-type':
        result.contentType = next ?? '';
        index += 1;
        break;
      case '--cache-control':
        result.cacheControl = next ?? '';
        index += 1;
        break;
      case '--public-base-url':
        result.publicBaseUrl = next ?? '';
        index += 1;
        break;
      case '--help':
      case '-h':
        printHelp();
        process.exit(0);
      default:
        throw new Error(`Unknown argument: ${arg}`);
    }
  }

  if (!result.file.trim()) {
    throw new Error('Missing --file');
  }
  if (!result.key.trim()) {
    throw new Error('Missing --key');
  }

  return result;
}

function printHelp() {
  console.log(`Usage:
  node scripts/upload-oss-file.mjs --file <path> --key <object-key> [options]

Options:
  --content-type <mime>       Override content type
  --cache-control <value>     Optional Cache-Control header
  --public-base-url <url>     Public base URL printed after upload

Env:
  ALIYUN_OSS_ACCESS_KEY_ID
  ALIYUN_OSS_ACCESS_KEY_SECRET
  ALIYUN_OSS_BUCKET
  ALIYUN_OSS_ENDPOINT
  OSS_PUBLIC_BASE_URL
  ALIYUN_OSS_PUBLIC_BASE_URL
`);
}

function requiredEnv(name) {
  const value = process.env[name]?.trim();
  if (!value) {
    throw new Error(`Missing required environment variable: ${name}`);
  }
  return value;
}

function guessContentType(filePath) {
  const extension = path.extname(filePath).toLowerCase();
  switch (extension) {
    case '.json':
      return 'application/json; charset=utf-8';
    case '.dmg':
      return 'application/x-apple-diskimage';
    case '.exe':
      return 'application/vnd.microsoft.portable-executable';
    case '.msi':
      return 'application/x-msi';
    case '.deb':
      return 'application/vnd.debian.binary-package';
    case '.rpm':
      return 'application/x-rpm';
    case '.appimage':
      return 'application/octet-stream';
    default:
      return 'application/octet-stream';
  }
}

function canonicalizeOssHeaders(headers) {
  return Object.entries(headers)
    .map(([key, value]) => [key.toLowerCase(), String(value).trim()])
    .filter(([key, value]) => key.startsWith('x-oss-') && value)
    .sort(([a], [b]) => a.localeCompare(b))
    .map(([key, value]) => `${key}:${value}\n`)
    .join('');
}

function buildSignature({ method, contentType, date, bucket, key, ossHeaders, secret }) {
  const canonicalizedHeaders = canonicalizeOssHeaders(ossHeaders);
  const canonicalizedResource = `/${bucket}/${key}`;
  const stringToSign = [
    method,
    '',
    contentType,
    date,
    `${canonicalizedHeaders}${canonicalizedResource}`,
  ].join('\n');

  return crypto.createHmac('sha1', secret).update(stringToSign).digest('base64');
}

function objectUrl(host, key) {
  const encodedKey = key
    .split('/')
    .map((segment) => encodeURIComponent(segment))
    .join('/');
  return `https://${host}/${encodedKey}`;
}

const options = parseArgs(process.argv.slice(2));
const filePath = path.resolve(options.file);
const accessKeyId = requiredEnv('ALIYUN_OSS_ACCESS_KEY_ID');
const accessKeySecret = requiredEnv('ALIYUN_OSS_ACCESS_KEY_SECRET');
const bucket = requiredEnv('ALIYUN_OSS_BUCKET');
const endpoint = requiredEnv('ALIYUN_OSS_ENDPOINT');
const host = `${bucket}.${endpoint}`.replace(/^https?:\/\//, '');
const body = await fs.readFile(filePath);
const date = new Date().toUTCString();
const contentType = options.contentType || guessContentType(filePath);
const ossHeaders = {};

if (options.cacheControl) {
  ossHeaders['Cache-Control'] = options.cacheControl;
}

const signature = buildSignature({
  method: 'PUT',
  contentType,
  date,
  bucket,
  key: options.key,
  ossHeaders,
  secret: accessKeySecret,
});

const headers = {
  Date: date,
  Authorization: `OSS ${accessKeyId}:${signature}`,
  'Content-Type': contentType,
  'Content-Length': String(body.byteLength),
  ...ossHeaders,
};

const uploadUrl = objectUrl(host, options.key);
const response = await fetch(uploadUrl, {
  method: 'PUT',
  headers,
  body,
});

if (!response.ok) {
  const detail = await response.text().catch(() => '');
  throw new Error(`OSS upload failed (${response.status}): ${detail || response.statusText}`);
}

const publicBaseUrl = (options.publicBaseUrl || `https://${host}`).replace(/\/+$/, '');
const publicUrl = `${publicBaseUrl}/${options.key}`;

console.log(`Uploaded ${path.basename(filePath)} -> ${options.key}`);
console.log(`Public URL: ${publicUrl}`);
