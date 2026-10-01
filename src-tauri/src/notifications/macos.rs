use super::{Adapter, Notice, Permission, SendError};
use block2::{DynBlock, RcBlock};
use objc2::{
    define_class, msg_send,
    rc::Retained,
    runtime::{Bool, ProtocolObject},
    AnyThread, DefinedClass,
};
use objc2_foundation::{NSBundle, NSError, NSObject, NSObjectProtocol, NSString};
use objc2_user_notifications::*;
use std::{ptr::NonNull, sync::mpsc, time::Duration};

struct DelegateIvars {
    app: tauri::AppHandle,
}

define_class!(
    #[unsafe(super(NSObject))]
    #[name = "PRSniperNotificationDelegate"]
    #[ivars = DelegateIvars]
    struct Delegate;

    unsafe impl NSObjectProtocol for Delegate {}

    unsafe impl UNUserNotificationCenterDelegate for Delegate {
        #[unsafe(method(userNotificationCenter:willPresentNotification:withCompletionHandler:))]
        fn present(
            &self,
            _center: &UNUserNotificationCenter,
            _notification: &UNNotification,
            completion: &DynBlock<dyn Fn(UNNotificationPresentationOptions)>,
        ) {
            completion
                .call((UNNotificationPresentationOptions::Banner
                    | UNNotificationPresentationOptions::List,));
        }

        #[unsafe(method(userNotificationCenter:didReceiveNotificationResponse:withCompletionHandler:))]
        fn respond(
            &self,
            _center: &UNUserNotificationCenter,
            response: &UNNotificationResponse,
            completion: &DynBlock<dyn Fn()>,
        ) {
            if &*response.actionIdentifier() == unsafe { UNNotificationDefaultActionIdentifier } {
                let app = self.ivars().app.clone();
                let request_id = response.notification().request().identifier().to_string();
                tauri::async_runtime::spawn(async move {
                    crate::record(&app, crate::storage::DiagnosticEvent::NotificationActivated);
                    if let Err(error) = super::host::open(&app, &request_id).await {
                        crate::report(&app, error);
                    }
                });
            }
            completion.call(());
        }
    }
);

// UserNotifications invokes its delegate on background threads. Our only ivar is
// an immutable, thread-safe AppHandle; the panel adapter dispatches presentation to the UI thread.
unsafe impl Send for Delegate {}
unsafe impl Sync for Delegate {}

pub(super) struct Native {
    _delegate: Retained<Delegate>,
}

impl Native {
    pub(super) fn new(app: &tauri::AppHandle) -> Result<Self, String> {
        let bundle = NSBundle::mainBundle();
        if bundle
            .bundleIdentifier()
            .as_ref()
            .map(|s| s.to_string())
            .as_deref()
            != Some(app.config().identifier.as_str())
            || !bundle.bundlePath().to_string().ends_with(".app")
        {
            return Err("Native notifications require the packaged PR Sniper.app; the development web server is not a notification host.".into());
        }
        let allocated = Delegate::alloc().set_ivars(DelegateIvars { app: app.clone() });
        let delegate: Retained<Delegate> = unsafe { msg_send![super(allocated), init] };
        UNUserNotificationCenter::currentNotificationCenter()
            .setDelegate(Some(ProtocolObject::from_ref(&*delegate)));
        Ok(Self {
            _delegate: delegate,
        })
    }
}

fn permission(settings: &UNNotificationSettings) -> Permission {
    let authorization = match settings.authorizationStatus() {
        UNAuthorizationStatus::NotDetermined => "not_determined",
        UNAuthorizationStatus::Denied => "denied",
        UNAuthorizationStatus::Authorized => "authorized",
        UNAuthorizationStatus::Provisional => "provisional",
        _ => "unknown",
    };
    Permission {
        authorization: authorization.into(),
        alerts_enabled: Some(settings.alertSetting() == UNNotificationSetting::Enabled),
        center_enabled: Some(
            settings.notificationCenterSetting() == UNNotificationSetting::Enabled,
        ),
    }
}

impl Adapter for Native {
    fn permission(&self) -> Result<Permission, String> {
        let (tx, rx) = mpsc::sync_channel(1);
        let completion = RcBlock::new(move |settings: NonNull<UNNotificationSettings>| {
            // Framework pointers are borrowed only for the duration of this callback.
            let value = permission(unsafe { settings.as_ref() });
            let _ = tx.send(value);
        });
        UNUserNotificationCenter::currentNotificationCenter()
            .getNotificationSettingsWithCompletionHandler(&completion);
        rx.recv_timeout(Duration::from_secs(5)).map_err(|_| {
            "macOS notification permission could not be read; no permission is assumed.".into()
        })
    }

    fn request_permission(&self) -> Result<Permission, String> {
        let (tx, rx) = mpsc::sync_channel(1);
        let completion = RcBlock::new(move |_granted: Bool, error: *mut NSError| {
            let result = match unsafe { error.as_ref() } {
                None => Ok(()),
                Some(error) => Err(format!(
                    "macOS notification permission request failed ({}: {}). Check the app's notification permission and bundle signing identity.",
                    error.domain(), error.code(),
                )),
            };
            let _ = tx.send(result);
        });
        UNUserNotificationCenter::currentNotificationCenter()
            .requestAuthorizationWithOptions_completionHandler(
                UNAuthorizationOptions::Alert,
                &completion,
            );
        rx.recv_timeout(Duration::from_secs(120))
            .map_err(|_| "Notification permission request timed out. Notifications remain off; check System Settings and try again.".to_string())??;
        self.permission()
    }

    fn send(&self, notice: &Notice) -> Result<(), SendError> {
        let content = UNMutableNotificationContent::new();
        content.setTitle(&NSString::from_str(notice.event.category.title()));
        content.setBody(&NSString::from_str(notice.event.category.body()));
        content.setThreadIdentifier(&NSString::from_str(&notice.event.group));
        content.setInterruptionLevel(UNNotificationInterruptionLevel::Active);
        let request = UNNotificationRequest::requestWithIdentifier_content_trigger(
            &NSString::from_str(&notice.id),
            &content,
            None,
        );
        let (tx, rx) = mpsc::sync_channel(1);
        let completion = RcBlock::new(move |error: *mut NSError| {
            let result = match unsafe { error.as_ref() } {
                None => Ok(()),
                Some(error) => Err(format!(
                    "macOS rejected the notification request ({}: {}).",
                    error.domain(),
                    error.code(),
                )),
            };
            // A timeout is already recorded as unknown; a late callback must not resend.
            let _ = tx.send(result);
        });
        UNUserNotificationCenter::currentNotificationCenter()
            .addNotificationRequest_withCompletionHandler(&request, Some(&completion));
        rx.recv_timeout(Duration::from_secs(10))
            .map_err(|_| SendError { message: "macOS did not confirm the request. Delivery is unknown; it will not be repeated.".into(), uncertain: true })?
            .map_err(|message| SendError { message, uncertain: false })
    }
}
