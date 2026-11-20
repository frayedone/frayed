//! Thin JNI glue for Android notifications and APK installation.
//! No domain logic here; every failure is logged and swallowed.

use jni::objects::JObject;
use jni::{jni_sig, jni_str, Env, JavaVM, JValue};
use slint::android::AndroidApp;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::OnceLock;

type JResult<T> = Result<T, jni::errors::Error>;

static APP: OnceLock<AndroidApp> = OnceLock::new();
static CHANNEL: AtomicBool = AtomicBool::new(false);

fn log_err(what: &str, err: impl std::fmt::Display) {
    eprintln!("[frayed] {what}: {err}");
}

pub fn init(app: &AndroidApp) {
    let _ = APP.set(app.clone());
}

fn with_jni<F>(label: &str, f: F)
where
    F: for<'local> FnOnce(&mut Env<'local>, &JObject<'local>) -> JResult<()>,
{
    let Some(app) = APP.get() else {
        log_err(label, "no android app");
        return;
    };
    let vm_ptr = app.vm_as_ptr() as *mut jni::sys::JavaVM;
    if vm_ptr.is_null() {
        log_err(label, "null JavaVM");
        return;
    }
    // Safety: android-activity hands us the live JVM. from_raw asserts non-null.
    let vm = unsafe { JavaVM::from_raw(vm_ptr) };
    if let Err(e) = vm.attach_current_thread(|env| {
        // Safety: the Activity ref is global and owned by AndroidApp, kept alive in APP.
        let activity = unsafe { JObject::from_raw(env, app.activity_as_ptr() as jni::sys::jobject) };
        let result = f(env, &activity);
        if env.exception_check() {
            env.exception_clear();
        }
        result
    }) {
        log_err(label, e);
    }
}

pub fn notify(title: &str, body: &str) {
    with_jni("notify", |env, activity| {
        let sdk = env
            .get_static_field(
                jni_str!("android/os/Build$VERSION"),
                jni_str!("SDK_INT"),
                jni_sig!("I"),
            )?
            .i()?;
        let service = env.new_string("notification")?;
        let manager = env
            .call_method(
                activity,
                jni_str!("getSystemService"),
                jni_sig!("(Ljava/lang/String;)Ljava/lang/Object;"),
                &[JValue::Object(service.as_ref())],
            )?
            .l()?;

        if sdk >= 26 && !CHANNEL.load(Ordering::Relaxed) {
            let id = env.new_string("frayed")?;
            let name = env.new_string("frayed")?;
            let importance = env
                .get_static_field(
                    jni_str!("android/app/NotificationManager"),
                    jni_str!("IMPORTANCE_DEFAULT"),
                    jni_sig!("I"),
                )?
                .i()?;
            let channel = env.new_object(
                jni_str!("android/app/NotificationChannel"),
                jni_sig!("(Ljava/lang/String;Ljava/lang/CharSequence;I)V"),
                &[
                    JValue::Object(id.as_ref()),
                    JValue::Object(name.as_ref()),
                    JValue::Int(importance),
                ],
            )?;
            env.call_method(
                &manager,
                jni_str!("createNotificationChannel"),
                jni_sig!("(Landroid/app/NotificationChannel;)V"),
                &[JValue::Object(&channel)],
            )?
            .v()?;
            CHANNEL.store(true, Ordering::Relaxed);
        }

        let id = env.new_string("frayed")?;
        let title = env.new_string(title)?;
        let body = env.new_string(body)?;
        let builder = if sdk >= 26 {
            env.new_object(
                jni_str!("android/app/Notification$Builder"),
                jni_sig!("(Landroid/content/Context;Ljava/lang/String;)V"),
                &[JValue::Object(activity), JValue::Object(id.as_ref())],
            )?
        } else {
            env.new_object(
                jni_str!("android/app/Notification$Builder"),
                jni_sig!("(Landroid/content/Context;)V"),
                &[JValue::Object(activity)],
            )?
        };
        let builder = env
            .call_method(
                &builder,
                jni_str!("setContentTitle"),
                jni_sig!("(Ljava/lang/CharSequence;)Landroid/app/Notification$Builder;"),
                &[JValue::Object(title.as_ref())],
            )?
            .l()?;
        let builder = env
            .call_method(
                &builder,
                jni_str!("setContentText"),
                jni_sig!("(Ljava/lang/CharSequence;)Landroid/app/Notification$Builder;"),
                &[JValue::Object(body.as_ref())],
            )?
            .l()?;
        let icon = env
            .get_static_field(
                jni_str!("android/R$drawable"),
                jni_str!("ic_dialog_info"),
                jni_sig!("I"),
            )?
            .i()?;
        let builder = env
            .call_method(
                &builder,
                jni_str!("setSmallIcon"),
                jni_sig!("(I)Landroid/app/Notification$Builder;"),
                &[JValue::Int(icon)],
            )?
            .l()?;
        let notification = env
            .call_method(
                &builder,
                jni_str!("build"),
                jni_sig!("()Landroid/app/Notification;"),
                &[],
            )?
            .l()?;
        env.call_method(
            &manager,
            jni_str!("notify"),
            jni_sig!("(ILandroid/app/Notification;)V"),
            &[JValue::Int(1), JValue::Object(&notification)],
        )?
        .v()
    });
}

pub fn install_apk(path: &str) {
    with_jni("install_apk", |env, activity| {
        // ponytail: a file:// Uri throws FileUriExposedException on Android 7+;
        // a real FileProvider needs XML resources cargo-apk2 cannot wire here.
        let path = env.new_string(path)?;
        let file = env.new_object(
            jni_str!("java/io/File"),
            jni_sig!("(Ljava/lang/String;)V"),
            &[JValue::Object(path.as_ref())],
        )?;
        let uri = env
            .call_static_method(
                jni_str!("android/net/Uri"),
                jni_str!("fromFile"),
                jni_sig!("(Ljava/io/File;)Landroid/net/Uri;"),
                &[JValue::Object(&file)],
            )?
            .l()?;
        let mime = env.new_string("application/vnd.android.package-archive")?;
        let view = env.new_string("android.intent.action.VIEW")?;
        if start_install(env, activity, &view, &uri, &mime).is_err() {
            if env.exception_check() {
                env.exception_clear();
            }
            eprintln!("[frayed] ACTION_VIEW install failed, trying INSTALL_PACKAGE");
            let install = env.new_string("android.intent.action.INSTALL_PACKAGE")?;
            start_install(env, activity, &install, &uri, &mime)?;
        }
        Ok(())
    });
}

fn start_install(
    env: &mut Env<'_>,
    activity: &JObject<'_>,
    action: &jni::objects::JString<'_>,
    uri: &JObject<'_>,
    mime: &jni::objects::JString<'_>,
) -> JResult<()> {
    const NEW_TASK: i32 = 0x1000_0000;
    const GRANT_READ_URI_PERMISSION: i32 = 0x0000_0001;

    let intent = env.new_object(
        jni_str!("android/content/Intent"),
        jni_sig!("(Ljava/lang/String;)V"),
        &[JValue::Object(action.as_ref())],
    )?;
    let intent = env
        .call_method(
            &intent,
            jni_str!("setDataAndType"),
            jni_sig!("(Landroid/net/Uri;Ljava/lang/String;)Landroid/content/Intent;"),
            &[JValue::Object(uri), JValue::Object(mime.as_ref())],
        )?
        .l()?;
    let intent = env
        .call_method(
            &intent,
            jni_str!("setFlags"),
            jni_sig!("(I)Landroid/content/Intent;"),
            &[JValue::Int(NEW_TASK | GRANT_READ_URI_PERMISSION)],
        )?
        .l()?;
    env.call_method(
        activity,
        jni_str!("startActivity"),
        jni_sig!("(Landroid/content/Intent;)V"),
        &[JValue::Object(&intent)],
    )?
    .v()
}
