import { describe, expect, it, beforeEach } from 'vitest';
import { useNotificationStore } from './notificationStore';

describe('useNotificationStore', () => {
  beforeEach(() => {
    // Reset store state before each test
    useNotificationStore.setState({
      notifications: [],
      unreadCount: 0,
      isOpen: false,
    });
  });

  it('adds a notification and increments unreadCount', () => {
    const store = useNotificationStore.getState();
    store.addNotification({
      type: 'info',
      title: 'Test Notification',
      message: 'This is a test message',
    });

    const state = useNotificationStore.getState();
    expect(state.notifications).toHaveLength(1);
    expect(state.notifications[0]).toMatchObject({
      type: 'info',
      title: 'Test Notification',
      message: 'This is a test message',
      read: false,
    });
    expect(state.unreadCount).toBe(1);
  });

  it('limits notifications to 100', () => {
    const store = useNotificationStore.getState();
    
    // Add 105 notifications
    for (let i = 0; i < 105; i++) {
      store.addNotification({
        type: 'info',
        title: `Notification ${i}`,
        message: 'Test message',
      });
    }

    const state = useNotificationStore.getState();
    expect(state.notifications).toHaveLength(100);
    // The first added (oldest) should be removed, the last added should be at the top
    expect(state.notifications[0].title).toBe('Notification 104');
    expect(state.notifications[99].title).toBe('Notification 5');
  });

  it('marks a notification as read and decrements unreadCount', () => {
    const store = useNotificationStore.getState();
    store.addNotification({
      type: 'success',
      title: 'Success',
      message: 'Done',
    });

    const notifId = useNotificationStore.getState().notifications[0].id;
    store.markAsRead(notifId);

    const state = useNotificationStore.getState();
    expect(state.notifications[0].read).toBe(true);
    expect(state.unreadCount).toBe(0);
  });

  it('marks all notifications as read', () => {
    const store = useNotificationStore.getState();
    store.addNotification({ type: 'info', title: '1', message: 'm1' });
    store.addNotification({ type: 'info', title: '2', message: 'm2' });

    store.markAllAsRead();

    const state = useNotificationStore.getState();
    expect(state.notifications.every(n => n.read)).toBe(true);
    expect(state.unreadCount).toBe(0);
  });

  it('dismisses a notification', () => {
    const store = useNotificationStore.getState();
    store.addNotification({ type: 'info', title: 'To Dismiss', message: 'm' });
    
    const notifId = useNotificationStore.getState().notifications[0].id;
    store.dismissNotification(notifId);

    const state = useNotificationStore.getState();
    expect(state.notifications).toHaveLength(0);
    expect(state.unreadCount).toBe(0);
  });

  it('dismisses all notifications', () => {
    const store = useNotificationStore.getState();
    store.addNotification({ type: 'info', title: '1', message: 'm1' });
    store.addNotification({ type: 'info', title: '2', message: 'm2' });

    store.dismissAll();

    const state = useNotificationStore.getState();
    expect(state.notifications).toHaveLength(0);
    expect(state.unreadCount).toBe(0);
  });

  it('toggles open state', () => {
    const store = useNotificationStore.getState();
    expect(useNotificationStore.getState().isOpen).toBe(false);

    store.setIsOpen(true);
    expect(useNotificationStore.getState().isOpen).toBe(true);

    store.setIsOpen(false);
    expect(useNotificationStore.getState().isOpen).toBe(false);
  });
});
