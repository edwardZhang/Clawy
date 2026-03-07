import { describe, expect, it } from 'vitest';
import {
  createRuntimeCheckMachineState,
  runtimeCheckMachineReducer,
  runtimeChecksReady,
  runtimePrerequisitesReady,
} from '@/pages/Setup/runtime-check-machine';

describe('runtime check machine', () => {
  it('starts with all cards idle and not ready', () => {
    const state = createRuntimeCheckMachineState();

    expect(state.checks.nodejs.status).toBe('idle');
    expect(state.checks.openclaw.status).toBe('idle');
    expect(state.checks.gateway.status).toBe('idle');
    expect(runtimePrerequisitesReady(state)).toBe(false);
    expect(runtimeChecksReady(state)).toBe(false);
  });

  it('becomes ready only when all three checks succeed', () => {
    const state = runtimeCheckMachineReducer(createRuntimeCheckMachineState(), {
      type: 'merge',
      updates: {
        nodejs: { status: 'success', message: 'Node ready' },
        openclaw: { status: 'success', message: 'OpenClaw ready' },
        gateway: { status: 'success', message: 'Gateway ready' },
      },
    });

    expect(runtimePrerequisitesReady(state)).toBe(true);
    expect(runtimeChecksReady(state)).toBe(true);
  });

  it('clears metadata when a check is retried', () => {
    const state = runtimeCheckMachineReducer(createRuntimeCheckMachineState(), {
      type: 'set',
      key: 'openclaw',
      patch: {
        status: 'success',
        message: 'OpenClaw ready',
        detail: 'Managed runtime active',
        path: '/tmp/openclaw',
      },
    });

    const retried = runtimeCheckMachineReducer(state, {
      type: 'set',
      key: 'openclaw',
      patch: {
        status: 'checking',
        message: 'Checking',
        detail: undefined,
        path: undefined,
      },
    });

    expect(retried.checks.openclaw.status).toBe('checking');
    expect(retried.checks.openclaw.detail).toBeUndefined();
    expect(retried.checks.openclaw.path).toBeUndefined();
  });
});
