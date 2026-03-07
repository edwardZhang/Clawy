export type RuntimeCheckKey = 'nodejs' | 'openclaw' | 'gateway';

export type RuntimeCheckStatus = 'idle' | 'checking' | 'success' | 'error';

export interface RuntimeCheckState {
  status: RuntimeCheckStatus;
  message: string;
  detail?: string;
  path?: string;
}

export interface RuntimeCheckMachineState {
  checks: Record<RuntimeCheckKey, RuntimeCheckState>;
}

export interface RuntimeCheckPatch {
  status: RuntimeCheckStatus;
  message?: string;
  detail?: string;
  path?: string;
}

type RuntimeCheckMachineAction =
  | { type: 'reset' }
  | { type: 'set'; key: RuntimeCheckKey; patch: RuntimeCheckPatch }
  | {
      type: 'merge';
      updates: Partial<Record<RuntimeCheckKey, RuntimeCheckPatch>>;
    };

function createIdleCheck(): RuntimeCheckState {
  return {
    status: 'idle',
    message: '',
  };
}

export function createRuntimeCheckMachineState(): RuntimeCheckMachineState {
  return {
    checks: {
      nodejs: createIdleCheck(),
      openclaw: createIdleCheck(),
      gateway: createIdleCheck(),
    },
  };
}

function mergeCheckState(
  current: RuntimeCheckState,
  patch: RuntimeCheckPatch
): RuntimeCheckState {
  return {
    ...current,
    ...patch,
    message: patch.message ?? current.message,
    detail: patch.detail,
    path: patch.path,
  };
}

export function runtimeCheckMachineReducer(
  state: RuntimeCheckMachineState,
  action: RuntimeCheckMachineAction
): RuntimeCheckMachineState {
  switch (action.type) {
    case 'reset':
      return createRuntimeCheckMachineState();
    case 'set':
      return {
        checks: {
          ...state.checks,
          [action.key]: mergeCheckState(state.checks[action.key], action.patch),
        },
      };
    case 'merge': {
      const nextChecks = { ...state.checks };
      for (const [key, patch] of Object.entries(action.updates) as Array<
        [RuntimeCheckKey, RuntimeCheckPatch | undefined]
      >) {
        if (!patch) {
          continue;
        }
        nextChecks[key] = mergeCheckState(state.checks[key], patch);
      }
      return {
        checks: nextChecks,
      };
    }
    default:
      return state;
  }
}

export function runtimePrerequisitesReady(state: RuntimeCheckMachineState) {
  return state.checks.nodejs.status === 'success' && state.checks.openclaw.status === 'success';
}

export function runtimeChecksReady(state: RuntimeCheckMachineState) {
  return runtimePrerequisitesReady(state) && state.checks.gateway.status === 'success';
}
