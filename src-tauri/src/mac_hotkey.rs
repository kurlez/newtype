//! Listen-only keyboard tap for the hotkey thread.
//!
//! Key identity comes from the hardware keycode and modifier flags. This
//! thread must not call TIS, TSM, or `UCKeyTranslate`. macOS 26 asserts that
//! those run on the main queue, and `rdev`'s listen callback does that on
//! the first key to fill `event.name`.

use core_foundation::runloop::{kCFRunLoopCommonModes, CFRunLoop};
use core_graphics::event::{
    CGEvent, CGEventTap, CGEventTapLocation, CGEventTapOptions, CGEventTapPlacement, CGEventType,
    CallbackResult, EventField,
};

use crate::pipeline::hotkey::{self, KeyEdge};

pub fn listen() -> Result<(), &'static str> {
    let tap = CGEventTap::new(
        CGEventTapLocation::HID,
        CGEventTapPlacement::HeadInsertEventTap,
        CGEventTapOptions::ListenOnly,
        vec![
            CGEventType::KeyDown,
            CGEventType::KeyUp,
            CGEventType::FlagsChanged,
        ],
        |_proxy, kind, event| {
            if let Some(edge) = edge_from_event(kind, event) {
                let _ = crate::key_feed::push(edge, false);
            }
            CallbackResult::Keep
        },
    )
    .map_err(|_| "could not create the keyboard event tap")?;
    let source = tap
        .mach_port()
        .create_runloop_source(0)
        .map_err(|_| "could not create the keyboard run loop source")?;
    CFRunLoop::get_current().add_source(&source, unsafe { kCFRunLoopCommonModes });
    tap.enable();
    CFRunLoop::run_current();
    Ok(())
}

fn edge_from_event(kind: CGEventType, event: &CGEvent) -> Option<KeyEdge> {
    let raw = event.get_integer_value_field(EventField::KEYBOARD_EVENT_KEYCODE);
    let code = u16::try_from(raw).ok()?;
    match kind {
        CGEventType::KeyDown => Some(KeyEdge::Down(hotkey::key_from_mac_keycode(code))),
        CGEventType::KeyUp => Some(KeyEdge::Up(hotkey::key_from_mac_keycode(code))),
        CGEventType::FlagsChanged => hotkey::mac_flags_changed(code, event.get_flags().bits()),
        _ => None,
    }
}
