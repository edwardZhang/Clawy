import { desktopApi } from '@/lib/desktop/api';
/**
 * TitleBar Component
 * macOS: use the native transparent title bar.
 * Windows/Linux: icon + "Clawy" on left, minimize/maximize/close on right.
 */
import { useState, useEffect } from 'react';
import { Minus, Square, X, Copy } from 'lucide-react';
import logoSvg from '@/assets/logo.svg';

export function TitleBar() {
  const isMac = desktopApi.platform === 'darwin';

  if (isMac) {
    return null;
  }

  return <WindowsTitleBar />;
}

function WindowsTitleBar() {
  const [maximized, setMaximized] = useState(false);

  useEffect(() => {
    // Check initial state
    desktopApi.ipcRenderer.invoke('window:isMaximized').then((val) => {
      setMaximized(val as boolean);
    });
  }, []);

  const handleMinimize = () => {
    desktopApi.ipcRenderer.invoke('window:minimize');
  };

  const handleMaximize = () => {
    desktopApi.ipcRenderer.invoke('window:maximize').then(() => {
      desktopApi.ipcRenderer.invoke('window:isMaximized').then((val) => {
        setMaximized(val as boolean);
      });
    });
  };

  const handleClose = () => {
    desktopApi.ipcRenderer.invoke('window:close');
  };

  return (
    <div className="flex h-11 shrink-0 items-center justify-between border-b bg-background">
      {/* Left: Icon + App Name */}
      <div
        data-tauri-drag-region
        className="drag-region flex min-w-0 flex-1 items-center gap-2 pl-3 select-none"
      >
        <img src={logoSvg} alt="Clawy" className="h-5 w-auto" />
        <span className="text-xs font-medium text-muted-foreground select-none">
          Clawy
        </span>
      </div>

      {/* Right: Window Controls */}
      <div className="no-drag flex h-full">
        <button
          onClick={handleMinimize}
          className="flex h-full w-11 items-center justify-center text-muted-foreground hover:bg-accent transition-colors"
          title="Minimize"
        >
          <Minus className="h-4 w-4" />
        </button>
        <button
          onClick={handleMaximize}
          className="flex h-full w-11 items-center justify-center text-muted-foreground hover:bg-accent transition-colors"
          title={maximized ? 'Restore' : 'Maximize'}
        >
          {maximized ? <Copy className="h-3.5 w-3.5" /> : <Square className="h-3.5 w-3.5" />}
        </button>
        <button
          onClick={handleClose}
          className="flex h-full w-11 items-center justify-center text-muted-foreground hover:bg-red-500 hover:text-white transition-colors"
          title="Close"
        >
          <X className="h-4 w-4" />
        </button>
      </div>
    </div>
  );
}
