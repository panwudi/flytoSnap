use block2::{Block, RcBlock};
use objc2::{
    DefinedClass, MainThreadOnly, define_class, msg_send,
    rc::{Allocated, Retained},
    runtime::{AnyClass, AnyObject},
};
use objc2_foundation::{
    MainThreadMarker, NSBundle, NSError, NSObject, NSObjectProtocol, NSString, ns_string,
};
use std::cell::RefCell;

#[derive(Default)]
struct DelegateState {
    pending: RefCell<Option<RcBlock<dyn Fn()>>>,
}

define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind=MainThreadOnly]
    #[ivars=DelegateState]
    struct UpdateDelegate;
    unsafe impl NSObjectProtocol for UpdateDelegate {}
    impl UpdateDelegate {
        #[unsafe(method(updater:mayPerformUpdateCheck:error:))]
        fn may_check(&self,_updater:&AnyObject,_kind:usize,error:*mut *mut NSError)->bool {
            let busy=super::app::update_busy();
            if busy && !error.is_null() {
                let message=NSString::from_str(&crate::i18n::tr("请先结束当前截图或录屏, 再检查更新"));
                let info=objc2_foundation::NSDictionary::from_slices(&[unsafe {objc2_foundation::NSLocalizedDescriptionKey}], &[&*message as &AnyObject]);
                let reason=unsafe {NSError::errorWithDomain_code_userInfo(ns_string!("com.flyto.snap.updater"),1,Some(&info))};
                unsafe {error.write(Retained::autorelease_ptr(reason));}
            }
            !busy
        }
        #[unsafe(method(updater:shouldPostponeRelaunchForUpdate:untilInvokingBlock:))]
        fn postpone(&self,_updater:&AnyObject,_item:&AnyObject,install:&Block<dyn Fn()>)->bool {
            if super::app::update_busy() {
                *self.ivars().pending.borrow_mut()=Some(install.copy());
                true
            } else {false}
        }
    }
);

pub struct Updater {
    _framework: Retained<NSBundle>,
    controller: Retained<AnyObject>,
    updater: Retained<AnyObject>,
    delegate: Retained<UpdateDelegate>,
}

impl Updater {
    pub fn load() -> Result<Self, String> {
        let root = NSBundle::mainBundle()
            .privateFrameworksPath()
            .ok_or("Please use the complete flytoSnap app bundle")?;
        let path = NSString::from_str(&format!("{root}/Sparkle.framework"));
        let framework = NSBundle::bundleWithPath(&path).ok_or("Sparkle.framework is missing")?;
        unsafe { framework.loadAndReturnError() }
            .map_err(|e| e.localizedDescription().to_string())?;
        let class = AnyClass::get(c"SPUStandardUpdaterController")
            .ok_or("Sparkle updater class is unavailable")?;
        let mtm = MainThreadMarker::new().expect("updater must run on main thread");
        let this = UpdateDelegate::alloc(mtm).set_ivars(DelegateState::default());
        let delegate: Retained<UpdateDelegate> = unsafe { msg_send![super(this), init] };
        let allocated: Allocated<AnyObject> = unsafe { msg_send![class, alloc] };
        let controller: Retained<AnyObject> = unsafe {
            msg_send![allocated,initWithStartingUpdater:false,updaterDelegate:&*delegate,userDriverDelegate:Option::<&AnyObject>::None]
        };
        let updater: Retained<AnyObject> = unsafe { msg_send![&*controller, updater] };
        Ok(Self {
            _framework: framework,
            controller,
            updater,
            delegate,
        })
    }

    pub fn start(&self) -> Result<(), String> {
        let mut error: Option<Retained<NSError>> = None;
        let success: bool = unsafe { msg_send![&*self.updater,startUpdater:&mut error] };
        if success {
            Ok(())
        } else {
            Err(error
                .map(|e| e.localizedDescription().to_string())
                .unwrap_or_else(|| "Could not start the updater".into()))
        }
    }

    pub fn can_check(&self) -> bool {
        unsafe { msg_send![&*self.updater, canCheckForUpdates] }
    }
    pub fn check(&self) {
        unsafe {
            let _: () = msg_send![&*self.controller,checkForUpdates:Option::<&AnyObject>::None];
        }
    }
    pub fn automatic(&self) -> bool {
        unsafe { msg_send![&*self.updater, automaticallyDownloadsUpdates] }
    }
    pub fn set_automatic(&self, enabled: bool) {
        unsafe {
            let _: () = msg_send![&*self.updater,setAutomaticallyChecksForUpdates:enabled];
            let _: () = msg_send![&*self.updater,setAutomaticallyDownloadsUpdates:enabled];
        }
    }
    pub fn deferred_install(&self) -> Option<RcBlock<dyn Fn()>> {
        self.delegate.ivars().pending.borrow_mut().take()
    }
}

pub fn verify() -> Result<(), String> {
    let updater = Updater::load()?;
    updater.start()?;
    let bundle = NSBundle::mainBundle();
    for key in ["SUFeedURL", "SUPublicEDKey"] {
        if bundle
            .objectForInfoDictionaryKey(&NSString::from_str(key))
            .is_none()
        {
            return Err(format!("Missing updater configuration: {key}"));
        }
    }
    let url: Option<Retained<objc2_foundation::NSURL>> =
        unsafe { msg_send![&*updater.updater, feedURL] };
    if url.and_then(|u| u.absoluteString()).as_deref()
        != Some(ns_string!(
            "https://raw.githubusercontent.com/panwudi/flytoSnap/main/appcast.xml"
        ))
    {
        return Err("Unexpected update feed URL".into());
    }
    println!(
        "Updater bundle check passed: Sparkle loaded, controller created, signed feed and key configured."
    );
    Ok(())
}
