use std::collections::HashSet;
use std::ffi::c_void;
use std::sync::{Arc, Mutex};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy};
use winit::monitor::MonitorHandle;

use crate::events::{
    winit_event_loop_process_ime_commit, EventProcessor, WinitControlFlow, WinitEvent,
};
use crate::WinitUserEvent;
use value_box::{ReturnBoxerResult, ValueBox, ValueBoxIntoRaw, ValueBoxPointer};

pub struct WinitEventLoop {
    pub(crate) event_loop: EventLoop<WinitUserEvent>,
    pub(crate) windows: Arc<Mutex<HashSet<winit::window::WindowId>>>,
    pub(crate) primary_monitor: Arc<Mutex<Option<MonitorHandle>>>,
}
#[derive(Debug)]
pub struct WinitEventLoopProxy(pub(crate) EventLoopProxy<WinitUserEvent>);

#[no_mangle]
pub extern "C" fn winit_event_loop_new() -> *mut ValueBox<WinitEventLoop> {
    // Prefer X11 when a DISPLAY is available. It supports creating the
    // initial window before the first active-event callback; callers can
    // override this with WINIT_UNIX_BACKEND=wayland.
    #[cfg(target_os = "linux")]
    if std::env::var_os("WINIT_UNIX_BACKEND").is_none()
        && std::env::var_os("DISPLAY").is_some()
    {
        std::env::set_var("WINIT_UNIX_BACKEND", "x11");
    }
    let event_loop = match EventLoop::with_user_event().build() {
        Ok(event_loop) => event_loop,
        Err(error) => {
            eprintln!("failed to create winit event loop: {error}");
            return std::ptr::null_mut();
        }
    };
    ValueBox::new(WinitEventLoop {
        event_loop,
        windows: Arc::new(Mutex::new(HashSet::new())),
        primary_monitor: Arc::new(Mutex::new(None)),
    })
    .into_raw()
}

#[no_mangle]
pub extern "C" fn winit_event_loop_drop(_ptr: *mut ValueBox<WinitEventLoop>) {
    _ptr.release();
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum WinitEventLoopType {
    Windows,
    MacOS,
    X11,
    Wayland,
    Unknown,
}

#[cfg(target_os = "linux")]
pub fn get_event_loop_type(
    _event_loop: &WinitEventLoop,
) -> WinitEventLoopType {
    use winit::platform::wayland::EventLoopExtWayland;
    use winit::platform::x11::EventLoopExtX11;

    if _event_loop.event_loop.is_wayland() {
        return WinitEventLoopType::Wayland;
    }
    if _event_loop.event_loop.is_x11() {
        return WinitEventLoopType::X11;
    }
    return WinitEventLoopType::Unknown;
}

#[cfg(target_os = "windows")]
pub fn get_event_loop_type(
    _event_loop: &WinitEventLoop,
) -> WinitEventLoopType {
    WinitEventLoopType::Windows
}

#[cfg(target_os = "macos")]
pub fn get_event_loop_type(
    _event_loop: &WinitEventLoop,
) -> WinitEventLoopType {
    WinitEventLoopType::MacOS
}

#[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
pub fn get_event_loop_type(
    _event_loop: &WinitEventLoop,
) -> WinitEventLoopType {
    WinitEventLoopType::Unknown
}

#[no_mangle]
pub extern "C" fn winit_event_loop_get_type(
    event_loop: *mut ValueBox<WinitEventLoop>,
) -> WinitEventLoopType {
    event_loop
        .with_ref_ok(|event_loop| get_event_loop_type(event_loop))
        .or_log(WinitEventLoopType::Unknown)
}

/// Hijacks the calling thread and initializes the winit event loop with the provided closure.
/// Since the closure is 'static, it must be a move closure if it needs to access any data from the calling context.
/// The callback's return value controls the event loop after each dispatched event.
/// Any values not passed to this function will not be dropped.
#[no_mangle]
pub extern "C" fn winit_event_loop_run_data(
    event_loop: *mut ValueBox<WinitEventLoop>,
    data: *mut c_void,
    callback: extern "C" fn(*mut c_void, *mut WinitEvent) -> WinitControlFlow,
) {
    event_loop
        .take_value()
        .map(|event_loop| {
            let mut event_processor = EventProcessor::new();
            let mut exit_requested = false;
            let event_loop_windows = Arc::clone(&event_loop.windows);
            event_loop.event_loop.run(
                move |event, event_loop| {
                    event_loop.set_control_flow(ControlFlow::Wait);
                    if matches!(&event, winit::event::Event::UserEvent(WinitUserEvent::Exit)) {
                        exit_requested = true;
                        let no_windows = event_loop_windows
                            .lock()
                            .map(|windows| windows.is_empty())
                            .unwrap_or(false);
                        if no_windows {
                            event_loop.exit();
                        }
                        return;
                    }
                    let destroyed = matches!(
                        &event,
                        winit::event::Event::WindowEvent {
                            event: winit::event::WindowEvent::Destroyed,
                            ..
                        }
                    );
                    if let winit::event::Event::WindowEvent { window_id, .. } = &event {
                        if destroyed {
                            let _ = event_loop_windows.lock().map(|mut windows| windows.remove(window_id));
                        }
                    }
                    let all_windows_destroyed = event_loop_windows
                        .lock()
                        .map(|windows| windows.is_empty())
                        .unwrap_or(false);
                    if let winit::event::Event::WindowEvent {
                        window_id,
                        event: winit::event::WindowEvent::Ime(winit::event::Ime::Commit(text)),
                    } = &event
                    {
                        for c_event in
                            winit_event_loop_process_ime_commit(window_id.clone(), text)
                        {
                            let c_event_ptr = Box::into_raw(Box::new(c_event));
                            let c_control_flow = callback(data, c_event_ptr);
                            unsafe {
                                let _ = Box::from_raw(c_event_ptr);
                            }
                            apply_control_flow(
                                event_loop,
                                c_control_flow,
                                &mut exit_requested,
                                false,
                            );
                        }
                        return;
                    }
                    let mut c_event: WinitEvent = Default::default();
                    let processed = event_processor.process(event, &mut c_event);
                    if processed {
                        let c_event_ptr = Box::into_raw(Box::new(c_event));
                        let c_control_flow = callback(data, c_event_ptr);
                        unsafe {
                            let _ = Box::from_raw(c_event_ptr);
                        };

                        apply_control_flow(
                            event_loop,
                            c_control_flow,
                            &mut exit_requested,
                            destroyed && all_windows_destroyed,
                        );
                    }

                    if exit_requested && destroyed && all_windows_destroyed {
                        event_loop.exit();
                    }
                },
            )
        })
        .log();
}

fn apply_control_flow(
    active: &ActiveEventLoop,
    flow: WinitControlFlow,
    exit_requested: &mut bool,
    destroyed: bool,
) {
    match flow {
        WinitControlFlow::Poll => active.set_control_flow(ControlFlow::Poll),
        WinitControlFlow::Wait => active.set_control_flow(ControlFlow::Wait),
        WinitControlFlow::Exit => {
            *exit_requested = true;
            if destroyed {
                active.exit();
            }
        }
    }
}

#[no_mangle]
pub extern "C" fn winit_event_loop_create_proxy(
    event_loop: *mut ValueBox<WinitEventLoop>,
) -> *mut ValueBox<WinitEventLoopProxy> {
    event_loop
        .with_ref_ok(|event_loop| value_box!(WinitEventLoopProxy(event_loop.event_loop.create_proxy())))
        .into_raw()
}

#[no_mangle]
pub extern "C" fn winit_event_loop_drop_proxy(
    event_loop_proxy: *mut ValueBox<WinitEventLoopProxy>,
) {
    event_loop_proxy.release();
}

/// Requests the event loop to exit through an event-loop proxy.
///
/// The request is deferred until all registered windows have delivered their
/// `Destroyed` event. This function is safe to call from another thread.
#[no_mangle]
pub extern "C" fn winit_event_loop_exit(
    event_loop_proxy: *mut ValueBox<WinitEventLoopProxy>,
) -> bool {
    event_loop_proxy
        .with_ref_ok(|proxy| proxy.0.send_event(WinitUserEvent::Exit).is_ok())
        .or_log(false)
}

///////////////////////////////////////////////////////////////////////////////////////
//////////////////////////////// M O N I T O R    I D /////////////////////////////////
///////////////////////////////////////////////////////////////////////////////////////

#[no_mangle]
pub extern "C" fn winit_event_loop_get_primary_monitor(
    event_loop: *mut ValueBox<WinitEventLoop>,
) -> *mut ValueBox<MonitorHandle> {
    event_loop
        .with_ref_ok(|event_loop| {
            event_loop
                .primary_monitor
                .lock()
                .ok()
                .and_then(|monitor| monitor.clone())
        })
        .map(|monitor| {
            monitor
                .map(|monitor| ValueBox::new(monitor).into_raw())
                .unwrap_or(std::ptr::null_mut())
        })
        .or_log(std::ptr::null_mut())
}
