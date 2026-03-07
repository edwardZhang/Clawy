export interface DesktopIpcRenderer {
  invoke(channel: string, ...args: unknown[]): Promise<unknown>;
  on(channel: string, callback: (...args: unknown[]) => void): (() => void) | void;
  once(channel: string, callback: (...args: unknown[]) => void): void;
  off(channel: string, callback?: (...args: unknown[]) => void): void;
}

export interface DesktopBridgeApi {
  ipcRenderer: DesktopIpcRenderer;
  openExternal: (url: string) => Promise<void>;
  platform: NodeJS.Platform;
  isDev: boolean;
}

declare global {
  interface Window {
    desktop: DesktopBridgeApi;
  }
}

export {};
