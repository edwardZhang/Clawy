#!/usr/bin/env zx

import 'zx/globals';
import os from 'node:os';

const VALID_SIGN_MODES = new Set(['none', 'adhoc', 'developer-id']);

function isTruthy(value) {
  if (typeof value !== 'string') {
    return false;
  }

  return ['1', 'true', 'yes', 'on'].includes(value.trim().toLowerCase());
}

function extractQuotedValue(line) {
  const match = line.match(/"([^"]+)"/);
  return match ? match[1] : null;
}

async function discoverDeveloperIdIdentities() {
  const result = await $({ stdio: 'pipe', nothrow: true })`security find-identity -v -p codesigning`;
  if (result.exitCode !== 0) {
    throw new Error(`Unable to inspect local code-signing identities.\n${result.stderr || result.stdout}`.trim());
  }

  const identities = result.stdout
    .split('\n')
    .map((line) => extractQuotedValue(line))
    .filter((identity) => identity && identity.startsWith('Developer ID Application:'));

  return [...new Set(identities)];
}

function hasAppStoreConnectCredentials(env) {
  return Boolean(env.APPLE_API_ISSUER && env.APPLE_API_KEY && env.APPLE_API_KEY_PATH);
}

function hasAppleIdCredentials(env) {
  return Boolean(env.APPLE_ID && env.APPLE_PASSWORD && env.APPLE_TEAM_ID);
}

export async function configureMacosSigning({ env, requestedSignMode = null, requestedNotarize = null }) {
  const platform = os.platform();
  const signMode = (requestedSignMode || env.CLAWY_MAC_SIGN_MODE || 'none').trim().toLowerCase();
  const notarize = requestedNotarize ?? isTruthy(env.CLAWY_MAC_NOTARIZE || '');

  if (!VALID_SIGN_MODES.has(signMode)) {
    throw new Error(`Unsupported macOS signing mode "${signMode}". Expected one of: none, adhoc, developer-id.`);
  }

  if (platform !== 'darwin') {
    if (signMode !== 'none' || notarize) {
      throw new Error('macOS signing can only be configured on macOS hosts.');
    }

    return {
      env,
      signMode,
      notarize,
      summary: 'macOS signing disabled (non-macOS host)',
    };
  }

  if (signMode === 'none') {
    if (notarize) {
      throw new Error('Notarization requires CLAWY_MAC_SIGN_MODE=developer-id.');
    }

    return {
      env,
      signMode,
      notarize,
      summary: 'macOS signing disabled',
    };
  }

  if (signMode === 'adhoc') {
    if (notarize) {
      throw new Error('Ad-hoc signatures cannot be notarized. Use CLAWY_MAC_SIGN_MODE=developer-id.');
    }

    env.APPLE_SIGNING_IDENTITY = '-';
    return {
      env,
      signMode,
      notarize,
      summary: 'using ad-hoc macOS signing identity (-)',
    };
  }

  const certificatePayloadConfigured = Boolean(env.APPLE_CERTIFICATE && env.APPLE_CERTIFICATE_PASSWORD);

  if (!certificatePayloadConfigured) {
    const configuredIdentity = env.APPLE_SIGNING_IDENTITY?.trim() || null;
    const identities = await discoverDeveloperIdIdentities();

    if (configuredIdentity) {
      if (!identities.includes(configuredIdentity)) {
        throw new Error(
          [
            `Configured APPLE_SIGNING_IDENTITY was not found in the local keychain: ${configuredIdentity}`,
            'Install the matching Developer ID Application certificate,',
            'or set APPLE_CERTIFICATE/APPLE_CERTIFICATE_PASSWORD for CI-style signing,',
            'or switch to CLAWY_MAC_SIGN_MODE=adhoc for unsigned distribution testing.',
          ].join(' '),
        );
      }
    } else if (identities.length === 1) {
      env.APPLE_SIGNING_IDENTITY = identities[0];
    } else if (identities.length === 0) {
      throw new Error(
        [
          'No Developer ID Application signing identity was found in the login keychain.',
          'Install a Developer ID Application certificate,',
          'or provide APPLE_CERTIFICATE and APPLE_CERTIFICATE_PASSWORD,',
          'or use CLAWY_MAC_SIGN_MODE=adhoc for local unsigned testing.',
        ].join(' '),
      );
    } else {
      throw new Error(
        [
          'Multiple Developer ID Application identities were found in the keychain.',
          'Set APPLE_SIGNING_IDENTITY explicitly to the one you want to use.',
          `Candidates: ${identities.join(' | ')}`,
        ].join(' '),
      );
    }
  }

  if (notarize && !hasAppStoreConnectCredentials(env) && !hasAppleIdCredentials(env)) {
    throw new Error(
      [
        'Notarization was requested, but no valid Apple notarization credentials were configured.',
        'Provide either APPLE_API_ISSUER + APPLE_API_KEY + APPLE_API_KEY_PATH,',
        'or APPLE_ID + APPLE_PASSWORD + APPLE_TEAM_ID.',
      ].join(' '),
    );
  }

  return {
    env,
    signMode,
    notarize,
    summary: certificatePayloadConfigured
      ? 'using APPLE_CERTIFICATE payload for Developer ID signing'
      : `using Developer ID signing identity "${env.APPLE_SIGNING_IDENTITY}"`,
  };
}
