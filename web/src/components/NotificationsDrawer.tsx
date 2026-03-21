import React from 'react';
import { useNotificationStore, type NotificationType } from '../stores/notificationStore';
import { RightDrawer } from './RightDrawer';
import { Button } from './Button';
import { 
  BellIcon, 
  CheckCircleIcon, 
  InfoIcon, 
  AlertTriangleIcon, 
  XIcon, 
  TrashIcon 
} from './icons';
import { cn } from './styles';

export const NotificationsDrawer: React.FC = () => {
  const { 
    notifications, 
    isOpen, 
    setIsOpen, 
    markAsRead, 
    markAllAsRead, 
    dismissNotification, 
    dismissAll 
  } = useNotificationStore();

  const getIcon = (type: NotificationType) => {
    switch (type) {
      case 'success':
        return <CheckCircleIcon className="h-5 w-5 text-[var(--success)]" />;
      case 'warning':
        return <AlertTriangleIcon className="h-5 w-5 text-[var(--warning)]" />;
      case 'error':
        return <AlertTriangleIcon className="h-5 w-5 text-[var(--error)]" />;
      default:
        return <InfoIcon className="h-5 w-5 text-[var(--accent)]" />;
    }
  };

  const formatTime = (isoString: string) => {
    try {
      return new Date(isoString).toLocaleTimeString([], { 
        hour: '2-digit', 
        minute: '2-digit' 
      });
    } catch {
      return isoString;
    }
  };

  return (
    <RightDrawer
      open={isOpen}
      onOpenChange={setIsOpen}
      title="Notifications"
      description="Stay updated with coordinator and task events"
      footer={
        notifications.length > 0 && (
          <div className="flex gap-3">
            <Button
              className="flex-1 gap-2"
              variant="secondary"
              onClick={markAllAsRead}
            >
              <CheckCircleIcon className="h-4 w-4" />
              Mark all as read
            </Button>
            <Button
              className="flex-1 gap-2 border-rose-500/30 text-rose-500 hover:bg-rose-500/10"
              variant="secondary"
              onClick={dismissAll}
            >
              <TrashIcon className="h-4 w-4" />
              Clear all
            </Button>
          </div>
        )
      }
    >
      <div className="flex flex-col gap-3">
        {notifications.length === 0 ? (
          <div className="flex flex-col items-center justify-center py-20 text-center opacity-40">
            <BellIcon className="mb-4 h-12 w-12" />
            <p className="text-lg font-medium">No notifications yet</p>
            <p className="text-sm">Important events will appear here</p>
          </div>
        ) : (
          notifications.map((notif) => (
            <div
              key={notif.id}
              className={cn(
                'group relative flex gap-4 rounded-xl border p-4 transition-all',
                notif.read 
                  ? 'border-white/5 bg-white/[0.02] opacity-60' 
                  : 'border-[var(--accent)]/20 bg-[var(--accent)]/[0.03]'
              )}
              onMouseEnter={() => !notif.read && markAsRead(notif.id)}
            >
              <div className="mt-0.5 flex-shrink-0">{getIcon(notif.type)}</div>
              <div className="flex-1 pr-6">
                <div className="flex items-center justify-between gap-2">
                  <h4 className="font-semibold text-sm leading-tight">{notif.title}</h4>
                  <span className="text-[10px] uppercase tracking-wider text-[var(--text-muted)]">
                    {formatTime(notif.timestamp)}
                  </span>
                </div>
                <p className="mt-1 text-xs leading-relaxed text-[var(--text-secondary)]">
                  {notif.message}
                </p>
              </div>
              <button
                className="absolute right-2 top-2 rounded-md p-1 opacity-0 transition-opacity hover:bg-white/10 group-hover:opacity-100"
                onClick={(e) => {
                  e.stopPropagation();
                  dismissNotification(notif.id);
                }}
                title="Dismiss"
              >
                <XIcon className="h-3.5 w-3.5" />
              </button>
            </div>
          ))
        )}
      </div>
    </RightDrawer>
  );
};
