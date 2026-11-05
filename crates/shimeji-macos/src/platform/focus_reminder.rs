//! Immediate native local notifications. No pending notification survives a
//! pause, sleep or app exit. A missing bundle or declined permission degrades
//! to the persistent menu/settings completed state.
use block2::{Block, RcBlock};
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, Bool, NSObject, NSObjectProtocol};
use objc2::{AnyThread, class, define_class, msg_send};
use objc2_foundation::{NSBundle, NSError, NSString};
use std::sync::{
    Arc,
    atomic::{AtomicU8, Ordering},
};

#[link(name = "UserNotifications", kind = "framework")]
unsafe extern "C" {}

define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind = AnyThread]
    struct NotificationDelegate;
    unsafe impl NSObjectProtocol for NotificationDelegate {}
    impl NotificationDelegate {
        #[unsafe(method(userNotificationCenter:willPresentNotification:withCompletionHandler:))]
        fn present(&self, _center: &AnyObject, _notification: &AnyObject, completion: &Block<dyn Fn(usize)>) {
            // Banner + list, even when the user has Settings open. Audio is
            // independently opt-in and played once by the app's PCM player.
            completion.call((24,));
        }
    }
);

#[derive(Default)]
pub struct Reminder {
    permission: Arc<AtomicU8>,
    delegate: Option<Retained<NotificationDelegate>>,
    sequence: u64,
}
impl Reminder {
    fn center(&mut self) -> Option<Retained<AnyObject>> {
        if NSBundle::mainBundle().bundleIdentifier().is_none() {
            eprintln!("pomodoro_notification=unavailable_without_app_bundle");
            return None;
        }
        // The framework is explicitly linked above. All pointers remain in the
        // macOS shell; blocks own their captures and no callback touches UI.
        let center: Retained<AnyObject> =
            unsafe { msg_send![class!(UNUserNotificationCenter), currentNotificationCenter] };
        if self.delegate.is_none() {
            let delegate: Retained<NotificationDelegate> =
                unsafe { msg_send![NotificationDelegate::alloc(), init] };
            unsafe {
                let _: () = msg_send![&*center, setDelegate: &*delegate];
            }
            self.delegate = Some(delegate);
        }
        Some(center)
    }
    pub fn authorize(&mut self) {
        if self.permission.load(Ordering::Acquire) != 0 {
            return;
        }
        let Some(center) = self.center() else {
            return;
        };
        self.permission.store(1, Ordering::Release);
        let permission = Arc::clone(&self.permission);
        let done = RcBlock::new(move |granted: Bool, error: *mut NSError| {
            permission.store(if granted.as_bool() { 2 } else { 3 }, Ordering::Release);
            if !error.is_null() {
                eprintln!("pomodoro_notification=authorization_error");
            }
            println!("pomodoro_notification_authorized={}", granted.as_bool());
        });
        unsafe {
            let _: () = msg_send![&*center, requestAuthorizationWithOptions: 4usize, completionHandler: &*done];
        }
    }
    pub fn deliver(&mut self, message: &str) {
        let Some(center) = self.center() else {
            return;
        };
        self.sequence = self.sequence.saturating_add(1);
        let content: Retained<AnyObject> =
            unsafe { msg_send![class!(UNMutableNotificationContent), new] };
        let title = NSString::from_str("桌宠番茄钟");
        let body = NSString::from_str(message);
        let identifier =
            NSString::from_str(&format!("focus-{}-{}", std::process::id(), self.sequence));
        let request: Retained<AnyObject> = unsafe {
            let _: () = msg_send![&*content,setTitle:&*title];
            let _: () = msg_send![&*content,setBody:&*body];
            msg_send![class!(UNNotificationRequest),requestWithIdentifier:&*identifier,content:&*content,trigger:std::ptr::null::<AnyObject>()]
        };
        let done = RcBlock::new(|error: *mut NSError| {
            if !error.is_null() {
                eprintln!("pomodoro_notification=delivery_error");
            }
        });
        unsafe {
            let _: () =
                msg_send![&*center,addNotificationRequest:&*request,withCompletionHandler:&*done];
        }
    }
}
