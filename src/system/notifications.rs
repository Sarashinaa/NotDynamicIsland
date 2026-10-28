#![allow(dead_code)]

//! Real-Time Windows Notification Listener
//! Uses WinRT UserNotificationListener to capture Windows Action Center & Toast notifications.
//! Hybrid: Tries push event first, falls back seamlessly to ultra-low-power poll (800ms) for unpackaged desktop apps.

use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;

use windows::Foundation::TypedEventHandler;
use windows::UI::Notifications::Management::UserNotificationListener;
use windows::UI::Notifications::{
    KnownNotificationBindings, NotificationKinds, UserNotification,
    UserNotificationChangedEventArgs, UserNotificationChangedKind,
};
use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::PostMessageW;

pub const WM_APP_NOTIFICATION_ARRIVED: u32 = 0x8006;

#[derive(Clone, Debug)]
pub struct ToastNotificationPayload {
    pub app_name: String,
    pub title: String,
    pub message: String,
}

pub struct NotificationListener {
    running: Arc<AtomicBool>,
}

impl NotificationListener {
    pub fn new() -> Self {
        Self {
            running: Arc::new(AtomicBool::new(false)),
        }
    }

    pub fn start(&mut self, hwnd: HWND) {
        if self.running.load(Ordering::SeqCst) {
            return;
        }
        self.running.store(true, Ordering::SeqCst);
        let hwnd_raw = hwnd.0 as isize;
        let running = Arc::clone(&self.running);

        thread::spawn(move || {
            // Initialize COM for WinRT dispatch
            unsafe {
                let _ = windows::Win32::System::Com::CoInitializeEx(
                    None,
                    windows::Win32::System::Com::COINIT_MULTITHREADED,
                );
            }

            let listener = match UserNotificationListener::Current() {
                Ok(l) => l,
                Err(e) => {
                    eprintln!("[NotificationListener] Failed to get Current: {:?}", e);
                    return;
                }
            };

            // Request access
            if let Ok(op) = listener.RequestAccessAsync() {
                let _ = op.get();
            }

            let listener_clone = listener.clone();
            let mut event_subscribed = false;

            let token = listener.NotificationChanged(&TypedEventHandler::<
                UserNotificationListener,
                UserNotificationChangedEventArgs,
            >::new(move |_sender, args| {
                if let Some(args) = args {
                    if let Ok(kind) = args.ChangeKind() {
                        if kind == UserNotificationChangedKind::Added {
                            if let Ok(id) = args.UserNotificationId() {
                                if let Ok(notif) = listener_clone.GetNotification(id) {
                                    if let Some(payload) = parse_user_notification(&notif) {
                                        let ptr = Box::into_raw(Box::new(payload)) as isize;
                                        unsafe {
                                            let _ = PostMessageW(
                                                HWND(hwnd_raw as *mut _),
                                                WM_APP_NOTIFICATION_ARRIVED,
                                                WPARAM(0),
                                                LPARAM(ptr),
                                            );
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
                Ok(())
            }));

            if token.is_ok() {
                event_subscribed = true;
                println!("[NotificationListener] Subscribed to push NotificationChanged events.");
            } else {
                println!("[NotificationListener] Unpackaged desktop mode: running low-power notification listener.");
            }

            // Seed initial notifications to prevent spamming old historical notifications
            let mut seen_ids = HashSet::new();
            if let Ok(op) = listener.GetNotificationsAsync(NotificationKinds::Toast) {
                if let Ok(notifs) = op.get() {
                    if let Ok(count) = notifs.Size() {
                        for i in 0..count {
                            if let Ok(n) = notifs.GetAt(i) {
                                if let Ok(id) = n.Id() {
                                    seen_ids.insert(id);
                                }
                            }
                        }
                    }
                }
            }

            // Loop: if event_subscribed, sleep calmly; if fallback polling, check for new toasts
            while running.load(Ordering::Relaxed) {
                if !event_subscribed {
                    if let Ok(op) = listener.GetNotificationsAsync(NotificationKinds::Toast) {
                        if let Ok(notifs) = op.get() {
                            if let Ok(count) = notifs.Size() {
                                for i in 0..count {
                                    if let Ok(n) = notifs.GetAt(i) {
                                        if let Ok(id) = n.Id() {
                                            if !seen_ids.contains(&id) {
                                                seen_ids.insert(id);
                                                if let Some(payload) = parse_user_notification(&n) {
                                                    let ptr = Box::into_raw(Box::new(payload)) as isize;
                                                    unsafe {
                                                        let _ = PostMessageW(
                                                            HWND(hwnd_raw as *mut _),
                                                            WM_APP_NOTIFICATION_ARRIVED,
                                                            WPARAM(0),
                                                            LPARAM(ptr),
                                                        );
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                    thread::sleep(std::time::Duration::from_millis(800));
                } else {
                    thread::sleep(std::time::Duration::from_secs(1));
                }
            }
        });
    }
}

fn parse_user_notification(notif: &UserNotification) -> Option<ToastNotificationPayload> {
    let app_name = notif
        .AppInfo()
        .and_then(|info| info.DisplayInfo())
        .and_then(|disp| disp.DisplayName())
        .map(|h| h.to_string())
        .unwrap_or_else(|_| "Notification".to_string());

    let mut title = String::new();
    let mut message = String::new();

    if let Ok(visual) = notif.Notification().and_then(|n| n.Visual()) {
        if let Ok(binding_id) = KnownNotificationBindings::ToastGeneric() {
            if let Ok(binding) = visual.GetBinding(&binding_id) {
                if let Ok(texts) = binding.GetTextElements() {
                    if let Ok(count) = texts.Size() {
                        if count > 0 {
                            if let Ok(elem) = texts.GetAt(0) {
                                if let Ok(t) = elem.Text() {
                                    title = t.to_string();
                                }
                            }
                        }
                        if count > 1 {
                            if let Ok(elem) = texts.GetAt(1) {
                                if let Ok(t) = elem.Text() {
                                    message = t.to_string();
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    if !title.is_empty() || !message.is_empty() {
        Some(ToastNotificationPayload {
            app_name,
            title,
            message,
        })
    } else {
        None
    }
}
