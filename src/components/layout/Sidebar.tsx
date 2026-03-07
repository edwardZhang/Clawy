import { desktopApi } from '@/lib/desktop/api';
/**
 * Sidebar Component
 * Navigation sidebar with menu items.
 * No longer fixed - sits inside the flex layout below the title bar.
 */
import { useState } from 'react';
import { NavLink, useLocation, useNavigate } from 'react-router-dom';
import * as DropdownMenu from '@radix-ui/react-dropdown-menu';
import {
  Home,
  MessageSquare,
  Radio,
  Puzzle,
  Clock,
  Settings,
  ChevronLeft,
  ChevronRight,
  Terminal,
  ExternalLink,
  Trash2,
  Plus,
} from 'lucide-react';
import { cn } from '@/lib/utils';
import { useSettingsStore } from '@/stores/settings';
import { useChatStore } from '@/stores/chat';
import { Button } from '@/components/ui/button';
import { Badge } from '@/components/ui/badge';
import { ConfirmDialog } from '@/components/ui/confirm-dialog';
import { useTranslation } from 'react-i18next';

interface NavItemProps {
  to: string;
  icon: React.ReactNode;
  label: string;
  badge?: string;
  collapsed?: boolean;
  onClick?: () => void;
}

function NavItem({ to, icon, label, badge, collapsed, onClick }: NavItemProps) {
  return (
    <NavLink
      to={to}
      onClick={onClick}
      className={({ isActive }) =>
        cn(
          'flex items-center gap-3 rounded-lg px-3 py-2 text-sm font-medium transition-colors',
          'hover:bg-accent hover:text-accent-foreground',
          isActive
            ? 'bg-accent text-accent-foreground'
            : 'text-muted-foreground',
          collapsed && 'justify-center px-2'
        )
      }
    >
      {icon}
      {!collapsed && (
        <>
          <span className="min-w-0 flex-1 truncate whitespace-nowrap text-left">{label}</span>
          {badge && (
            <Badge variant="secondary" className="ml-auto">
              {badge}
            </Badge>
          )}
        </>
      )}
    </NavLink>
  );
}

export function Sidebar() {
  const sidebarCollapsed = useSettingsStore((state) => state.sidebarCollapsed);
  const setSidebarCollapsed = useSettingsStore((state) => state.setSidebarCollapsed);
  const devModeUnlocked = useSettingsStore((state) => state.devModeUnlocked);

  const sessions = useChatStore((s) => s.sessions);
  const currentSessionKey = useChatStore((s) => s.currentSessionKey);
  const sessionLabels = useChatStore((s) => s.sessionLabels);
  const sessionLastActivity = useChatStore((s) => s.sessionLastActivity);
  const switchSession = useChatStore((s) => s.switchSession);
  const newSession = useChatStore((s) => s.newSession);
  const deleteSession = useChatStore((s) => s.deleteSession);

  const navigate = useNavigate();
  const { pathname } = useLocation();
  const isOnChat = pathname === '/' || pathname === '/chat';

  const mainSessions = sessions.filter((s) => s.key.endsWith(':main'));
  const otherSessions = sessions.filter((s) => !s.key.endsWith(':main'));
  const orderedSessions = [...mainSessions, ...[...otherSessions].sort((a, b) =>
    (sessionLastActivity[b.key] ?? 0) - (sessionLastActivity[a.key] ?? 0)
  )];

  const getSessionLabel = (key: string, displayName?: string, label?: string) =>
    sessionLabels[key] ?? label ?? displayName ?? key;

  const openDevConsole = async () => {
    try {
      const result = await desktopApi.ipcRenderer.invoke('gateway:getControlUiUrl') as {
        success: boolean;
        url?: string;
        error?: string;
      };
      if (result.success && result.url) {
        desktopApi.openExternal(result.url);
      } else {
        console.error('Failed to get Dev Console URL:', result.error);
      }
    } catch (err) {
      console.error('Error opening Dev Console:', err);
    }
  };

  const { t } = useTranslation();
  const [sessionToDelete, setSessionToDelete] = useState<{ key: string; label: string } | null>(null);
  const [sessionsExpanded, setSessionsExpanded] = useState(true);
  const [collapsedChatMenuOpen, setCollapsedChatMenuOpen] = useState(false);

  const navItems = [
    { to: '/cron', icon: <Clock className="h-5 w-5" />, label: t('sidebar.cronTasks') },
    { to: '/skills', icon: <Puzzle className="h-5 w-5" />, label: t('sidebar.skills') },
    { to: '/channels', icon: <Radio className="h-5 w-5" />, label: t('sidebar.channels') },
    { to: '/dashboard', icon: <Home className="h-5 w-5" />, label: t('sidebar.dashboard') },
    { to: '/settings', icon: <Settings className="h-5 w-5" />, label: t('sidebar.settings') },
  ];

  const handleCreateSession = () => {
    setCollapsedChatMenuOpen(false);
    newSession();
    navigate('/');
  };

  const renderSessionRows = (compact = false) => (
    <>
      <button
        onClick={handleCreateSession}
        className={cn(
          'flex w-full items-center gap-2 rounded-md px-3 py-1.5 text-left text-sm transition-colors',
          'text-muted-foreground hover:bg-accent hover:text-accent-foreground',
          compact && 'pr-3',
        )}
      >
        <Plus className="h-3.5 w-3.5 shrink-0" />
        <span className="min-w-0 flex-1 truncate whitespace-nowrap text-left">
          {t('sidebar.newChat')}
        </span>
      </button>

      {orderedSessions.map((s) => (
        <div key={s.key} className="group relative flex items-center">
          <button
            onClick={() => {
              setCollapsedChatMenuOpen(false);
              switchSession(s.key);
              navigate('/');
            }}
            className={cn(
              'w-full truncate rounded-md px-3 py-1.5 text-left text-sm transition-colors',
              !compact && !s.key.endsWith(':main') && 'pr-7',
              'hover:bg-accent hover:text-accent-foreground',
              isOnChat && currentSessionKey === s.key
                ? 'bg-accent/60 font-medium text-accent-foreground'
                : 'text-muted-foreground',
            )}
          >
            {getSessionLabel(s.key, s.displayName, s.label)}
          </button>
          {!compact && !s.key.endsWith(':main') && (
            <button
              aria-label="Delete session"
              onClick={(e) => {
                e.stopPropagation();
                setSessionToDelete({
                  key: s.key,
                  label: getSessionLabel(s.key, s.displayName, s.label),
                });
              }}
              className={cn(
                'absolute right-1 flex items-center justify-center rounded p-0.5 transition-opacity',
                'opacity-0 group-hover:opacity-100',
                'text-muted-foreground hover:bg-destructive/10 hover:text-destructive',
              )}
            >
              <Trash2 className="h-3.5 w-3.5" />
            </button>
          )}
        </div>
      ))}
    </>
  );

  return (
    <aside
      className={cn(
        'flex shrink-0 flex-col overflow-hidden border-r bg-background transition-all duration-300',
        sidebarCollapsed ? 'w-16' : 'w-64'
      )}
    >
      {/* Navigation */}
      <nav className="flex-1 overflow-hidden flex flex-col p-2 gap-1">
        {sidebarCollapsed ? (
          <DropdownMenu.Root open={collapsedChatMenuOpen} onOpenChange={setCollapsedChatMenuOpen}>
            <DropdownMenu.Trigger asChild>
              <button
                title={t('sidebar.chat')}
                aria-label={t('sidebar.chat')}
                className={cn(
                  'flex items-center justify-center gap-3 rounded-lg px-2 py-2 text-sm font-medium transition-colors',
                  'hover:bg-accent hover:text-accent-foreground',
                  isOnChat ? 'bg-accent text-accent-foreground' : 'text-muted-foreground',
                )}
              >
                <MessageSquare className="h-5 w-5 shrink-0" />
              </button>
            </DropdownMenu.Trigger>

            <DropdownMenu.Portal>
              <DropdownMenu.Content
                side="right"
                align="start"
                sideOffset={8}
                className={cn(
                  'z-50 min-w-56 overflow-hidden rounded-lg border bg-popover p-1 text-popover-foreground shadow-md',
                  'max-h-80 overflow-y-auto',
                )}
              >
                <div className="px-2 py-1.5 text-xs font-medium text-muted-foreground">
                  {t('sidebar.chat')}
                </div>
                <div className="space-y-0.5">
                  {renderSessionRows(true)}
                </div>
              </DropdownMenu.Content>
            </DropdownMenu.Portal>
          </DropdownMenu.Root>
        ) : (
          <div className="space-y-1">
            <button
              type="button"
              onClick={() => setSessionsExpanded((expanded) => !expanded)}
              className={cn(
                'flex w-full items-center gap-3 rounded-lg px-3 py-2 text-sm font-medium transition-colors',
                'hover:bg-accent hover:text-accent-foreground',
                isOnChat ? 'bg-accent text-accent-foreground' : 'text-muted-foreground',
              )}
            >
              <MessageSquare className="h-5 w-5 shrink-0" />
              <span className="min-w-0 flex-1 truncate whitespace-nowrap text-left">
                {t('sidebar.chat')}
              </span>
              <ChevronRight
                className={cn(
                  'h-4 w-4 shrink-0 transition-transform',
                  sessionsExpanded && 'rotate-90',
                )}
              />
            </button>

            {sessionsExpanded && (
              <div
                className={cn(
                  'ml-4 max-h-72 w-[calc(100%-1rem)] space-y-0.5 overflow-y-auto border-l pl-2',
                )}
              >
                {renderSessionRows(false)}
              </div>
            )}
          </div>
        )}

        {navItems.map((item) => (
          <NavItem
            key={item.to}
            {...item}
            collapsed={sidebarCollapsed}
          />
        ))}
      </nav>

      {/* Footer */}
      <div className="p-2 space-y-2">
        {devModeUnlocked && !sidebarCollapsed && (
          <Button
            variant="ghost"
            size="sm"
            className="w-full justify-start"
            onClick={openDevConsole}
          >
            <Terminal className="h-4 w-4 mr-2" />
            {t('sidebar.devConsole')}
            <ExternalLink className="h-3 w-3 ml-auto" />
          </Button>
        )}

        <Button
          variant="ghost"
          size="icon"
          className="w-full"
          onClick={() => setSidebarCollapsed(!sidebarCollapsed)}
        >
          {sidebarCollapsed ? (
            <ChevronRight className="h-4 w-4" />
          ) : (
            <ChevronLeft className="h-4 w-4" />
          )}
        </Button>
      </div>

      <ConfirmDialog
        open={!!sessionToDelete}
        title={t('common.confirm', 'Confirm')}
        message={sessionToDelete ? t('sidebar.deleteSessionConfirm', `Delete "${sessionToDelete.label}"?`) : ''}
        confirmLabel={t('common.delete', 'Delete')}
        cancelLabel={t('common.cancel', 'Cancel')}
        variant="destructive"
        onConfirm={async () => {
          if (!sessionToDelete) return;
          await deleteSession(sessionToDelete.key);
          if (currentSessionKey === sessionToDelete.key) navigate('/');
          setSessionToDelete(null);
        }}
        onCancel={() => setSessionToDelete(null)}
      />
    </aside>
  );
}
