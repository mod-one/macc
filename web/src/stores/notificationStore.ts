import { create } from 'zustand';

export type NotificationType = 'info' | 'success' | 'warning' | 'error';

export interface Notification {
  id: string;
  type: NotificationType;
  title: string;
  message: string;
  timestamp: string;
  read: boolean;
}

interface NotificationState {
  notifications: Notification[];
  unreadCount: number;
  isOpen: boolean;
  setIsOpen: (open: boolean) => void;
  addNotification: (notification: Omit<Notification, 'id' | 'read' | 'timestamp'>) => void;
  markAsRead: (id: string) => void;
  markAllAsRead: () => void;
  dismissNotification: (id: string) => void;
  dismissAll: () => void;
}

export const useNotificationStore = create<NotificationState>((set) => ({
  notifications: [],
  unreadCount: 0,
  isOpen: false,
  setIsOpen: (open) => set({ isOpen: open }),
  addNotification: (n) =>
    set((state) => {
      const newNotification: Notification = {
        ...n,
        id: Math.random().toString(36).substring(7),
        timestamp: new Date().toISOString(),
        read: false,
      };
      const notifications = [newNotification, ...state.notifications].slice(0, 100);
      return {
        notifications,
        unreadCount: notifications.filter((notif) => !notif.read).length,
      };
    }),
  markAsRead: (id) =>
    set((state) => {
      const notifications = state.notifications.map((n) =>
        n.id === id ? { ...n, read: true } : n,
      );
      return {
        notifications,
        unreadCount: notifications.filter((notif) => !notif.read).length,
      };
    }),
  markAllAsRead: () =>
    set((state) => {
      const notifications = state.notifications.map((n) => ({ ...n, read: true }));
      return {
        notifications,
        unreadCount: 0,
      };
    }),
  dismissNotification: (id) =>
    set((state) => {
      const notifications = state.notifications.filter((n) => n.id !== id);
      return {
        notifications,
        unreadCount: notifications.filter((notif) => !notif.read).length,
      };
    }),
  dismissAll: () => set({ notifications: [], unreadCount: 0 }),
}));
