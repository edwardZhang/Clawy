type DesktopApi = NonNullable<Window['desktop']>;

function requireDesktopApi(): DesktopApi {
  const api = window.desktop;
  if (!api) {
    throw new Error('Desktop bridge is not installed');
  }
  return api;
}

export const desktopApi: DesktopApi = {
  ipcRenderer: {
    invoke(channel, ...args) {
      return requireDesktopApi().ipcRenderer.invoke(channel, ...args);
    },
    on(channel, callback) {
      const unlisten = requireDesktopApi().ipcRenderer.on(channel, callback);
      if (typeof unlisten === 'function') {
        return unlisten;
      }
      return () => {
        requireDesktopApi().ipcRenderer.off(channel, callback);
      };
    },
    once(channel, callback) {
      requireDesktopApi().ipcRenderer.once(channel, callback);
    },
    off(channel, callback) {
      requireDesktopApi().ipcRenderer.off(channel, callback);
    },
  },
  openExternal(url) {
    return requireDesktopApi().openExternal(url);
  },
  get platform() {
    return requireDesktopApi().platform;
  },
  get isDev() {
    return requireDesktopApi().isDev;
  },
};
