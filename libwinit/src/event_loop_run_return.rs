use std::ffi::c_void;
use std::sync::Arc;
use std::collections::VecDeque;
use std::sync::{Mutex, OnceLock};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::time::Duration;

use winit::platform::pump_events::EventLoopExtPumpEvents;
use winit::event::{Event, WindowEvent};

use crate::events::{EventProcessor, WinitControlFlow, WinitEvent};
use crate::event_loop::WinitEventLoop;
use crate::WinitUserEvent;
use value_box::{ReturnBoxerResult, ValueBox, ValueBoxPointer};

static EVENT_QUEUE: OnceLock<Mutex<VecDeque<WinitEvent>>> = OnceLock::new();

fn event_queue() -> &'static Mutex<VecDeque<WinitEvent>> {
    EVENT_QUEUE.get_or_init(|| Mutex::new(VecDeque::new()))
}

#[no_mangle]
pub extern "C" fn winit_event_loop_run_return(
    event_loop: *mut ValueBox<WinitEventLoop>,
) {
    let result = catch_unwind(AssertUnwindSafe(|| event_loop.with_mut_ok(|event_loop| {
        let mut processor = EventProcessor::new();
        let windows = Arc::clone(&event_loop.windows);
        let mut exit_requested = false;
        event_loop.event_loop.pump_events(Some(Duration::from_millis(50)), |event, active| {
            active.set_control_flow(winit::event_loop::ControlFlow::Wait);
            if matches!(&event, Event::UserEvent(WinitUserEvent::Exit)) {
                exit_requested = true;
                if windows.lock().map(|windows| windows.is_empty()).unwrap_or(false) {
                    active.exit();
                }
                return;
            }
            let destroyed = matches!(
                &event,
                Event::WindowEvent { event: WindowEvent::Destroyed, .. }
            );
            if let Event::WindowEvent { window_id, .. } = &event {
                if destroyed {
                    let _ = windows.lock().map(|mut windows| windows.remove(window_id));
                }
            }
            let all_windows_destroyed = windows
                .lock()
                .map(|windows| windows.is_empty())
                .unwrap_or(false);
            let mut c_event = WinitEvent::default();
            if processor.process(event, &mut c_event) {
                event_queue().lock().unwrap().push_back(c_event);
                if destroyed && all_windows_destroyed {
                    exit_requested = true;
                    active.exit();
                }
            }
            if exit_requested && destroyed && all_windows_destroyed {
                active.exit();
            }
        });
    }).log()));
    if result.is_err() {
        eprintln!("libwinit: panic while running the event loop");
    }
}

#[no_mangle]
pub extern "C" fn winit_event_queue_pop() -> *mut WinitEvent {
    event_queue()
        .lock()
        .ok()
        .and_then(|mut events| events.pop_front())
        .map(|event| Box::into_raw(Box::new(event)))
        .unwrap_or(std::ptr::null_mut())
}

#[no_mangle]
pub extern "C" fn winit_event_loop_run_return_data(
    event_loop: *mut ValueBox<WinitEventLoop>,
    data: *mut c_void,
    callback: extern "C" fn(*mut c_void, *mut WinitEvent) -> WinitControlFlow,
) {
    event_loop.with_mut_ok(|event_loop| {
        let mut processor = EventProcessor::new();
        let windows = Arc::clone(&event_loop.windows);
        let mut exit_requested = false;
        event_loop.event_loop.pump_events(Some(Duration::from_millis(50)), |event, active| {
            active.set_control_flow(winit::event_loop::ControlFlow::Wait);
            if matches!(&event, Event::UserEvent(WinitUserEvent::Exit)) {
                exit_requested = true;
                if windows.lock().map(|windows| windows.is_empty()).unwrap_or(false) {
                    active.exit();
                }
                return;
            }
            let destroyed = matches!(
                &event,
                Event::WindowEvent { event: WindowEvent::Destroyed, .. }
            );
            if let Event::WindowEvent { window_id, .. } = &event {
                if destroyed {
                    let _ = windows.lock().map(|mut windows| windows.remove(window_id));
                }
            }
            let all_windows_destroyed = windows
                .lock()
                .map(|windows| windows.is_empty())
                .unwrap_or(false);
            let mut c_event = WinitEvent::default();
            if processor.process(event, &mut c_event) {
                let ptr = Box::into_raw(Box::new(c_event));
                let flow = callback(data, ptr);
                unsafe { drop(Box::from_raw(ptr)); }
                apply_control_flow(
                    active,
                    flow,
                    &mut exit_requested,
                    destroyed && all_windows_destroyed,
                );
            }
            if exit_requested && destroyed && all_windows_destroyed {
                active.exit();
            }
        });
    }).log();
}

fn apply_control_flow(
    active: &winit::event_loop::ActiveEventLoop,
    flow: WinitControlFlow,
    exit_requested: &mut bool,
    destroyed: bool,
) {
    match flow {
        WinitControlFlow::Poll => active.set_control_flow(winit::event_loop::ControlFlow::Poll),
        WinitControlFlow::Wait => active.set_control_flow(winit::event_loop::ControlFlow::Wait),
        WinitControlFlow::Exit => {
            *exit_requested = true;
            if destroyed {
                active.exit();
            }
        }
    }
}
