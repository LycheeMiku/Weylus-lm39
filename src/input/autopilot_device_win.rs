use std::collections::HashMap;
use tracing::warn;
use winapi::shared::minwindef::DWORD;
use winapi::shared::windef::{HWND, POINT};
use winapi::um::winuser::*;

use crate::capturable::{Capturable, Geometry};
use crate::input::autopilot_device::AutoPilotDevice;
use crate::input::device::{InputDevice, InputDeviceType};
use crate::protocol::{
    Button, KeyboardEvent, PointerEvent, PointerEventType, PointerType, WheelEvent,
};

#[derive(Default, Debug, Clone, Copy)]
struct PenStateTracker {
    in_range: bool,
    in_contact: bool,
    barrel_down: bool,
    eraser_down: bool,
}

pub struct WindowsInput {
    capturable: Box<dyn Capturable>,
    autopilot_device: AutoPilotDevice,
    pointer_device_handle: *mut HSYNTHETICPOINTERDEVICE__,
    touch_device_handle: *mut HSYNTHETICPOINTERDEVICE__,
    multitouch_map: HashMap<i64, POINTER_TYPE_INFO>,
    pen_tracker: PenStateTracker,
    pub stylus_only: bool,
    pub orientation: u16,
    pub pressure_sensitivity: f64,
}

impl WindowsInput {
    pub fn new(capturable: Box<dyn Capturable>) -> Self {
        unsafe {
            InitializeTouchInjection(5, TOUCH_FEEDBACK_DEFAULT);
            Self {
                capturable: capturable.clone(),
                autopilot_device: AutoPilotDevice::new(capturable),
                pointer_device_handle: CreateSyntheticPointerDevice(PT_PEN, 1, 1),
                touch_device_handle: CreateSyntheticPointerDevice(PT_TOUCH, 5, 1),
                multitouch_map: HashMap::new(),
                pen_tracker: PenStateTracker::default(),
                stylus_only: false,
                orientation: 0,
                pressure_sensitivity: 1.0,
            }
        }
    }

    fn transform_coordinates(&self, orig_x: f64, orig_y: f64) -> (f64, f64) {
        match self.orientation {
            90 => (1.0 - orig_y, orig_x),
            180 => (1.0 - orig_x, 1.0 - orig_y),
            270 => (orig_y, 1.0 - orig_x),
            _ => (orig_x, orig_y),
        }
    }
}

impl Drop for WindowsInput {
    fn drop(&mut self) {
        unsafe {
            if !self.pointer_device_handle.is_null() {
                DestroySyntheticPointerDevice(self.pointer_device_handle);
                self.pointer_device_handle = std::ptr::null_mut();
            }
            if !self.touch_device_handle.is_null() {
                DestroySyntheticPointerDevice(self.touch_device_handle);
                self.touch_device_handle = std::ptr::null_mut();
            }
        }
    }
}

impl InputDevice for WindowsInput {
    fn send_wheel_event(&mut self, event: &WheelEvent) {
        unsafe { mouse_event(MOUSEEVENTF_WHEEL, 0, 0, event.dy as DWORD, 0) };
    }

    fn update_tablet_config(
        &mut self,
        stylus_only: bool,
        orientation: u16,
        pressure_sensitivity: f64,
    ) {
        self.stylus_only = stylus_only;
        self.orientation = orientation;
        self.pressure_sensitivity = pressure_sensitivity;
    }

    fn send_pointer_event(&mut self, event: &PointerEvent) {
        // Palm rejection / Stylus only mode filter
        if self.stylus_only && event.pointer_type != PointerType::Pen {
            return;
        }

        if let Err(err) = self.capturable.before_input() {
            warn!("Failed to activate window, sending no input ({})", err);
            return;
        }

        let Geometry::VirtualScreen(offset_x, offset_y, width, height, left, top) =
            self.capturable.geometry().unwrap()
        else {
            unreachable!()
        };

        // Transform orientation
        let (tx, ty) = self.transform_coordinates(event.x, event.y);
        let tx = tx.clamp(0.0, 1.0);
        let ty = ty.clamp(0.0, 1.0);

        // Virtual-screen coordinates for synthetic pointer injection (ptPixelLocation)
        // In Windows SyntheticPointerDevice, coordinates are relative to the virtual desktop bounding box origin (always >= 0)
        let max_x = (offset_x + width as i32 - 1).max(offset_x);
        let max_y = (offset_y + height as i32 - 1).max(offset_y);
        let x = ((offset_x as f64 + tx * (width as f64 - 1.0)).round() as i32).clamp(offset_x, max_x);
        let y = ((offset_y as f64 + ty * (height as f64 - 1.0)).round() as i32).clamp(offset_y, max_y);

        // Desktop coordinates for SetCursorPos (used when pointer is emulated as Mouse)
        let max_screen_x = (left + width as i32 - 1).max(left);
        let max_screen_y = (top + height as i32 - 1).max(top);
        let screen_x = ((left as f64 + tx * (width as f64 - 1.0)).round() as i32).clamp(left, max_screen_x);
        let screen_y = ((top as f64 + ty * (height as f64 - 1.0)).round() as i32).clamp(top, max_screen_y);

        match event.pointer_type {
            PointerType::Pen => {
                let has_primary = event.buttons.contains(Button::PRIMARY);
                let has_secondary = event.buttons.contains(Button::SECONDARY);
                let has_eraser = event.buttons.contains(Button::ERASER);

                let is_contact_event = match event.event_type {
                    PointerEventType::DOWN => true,
                    PointerEventType::UP => false,
                    PointerEventType::CANCEL
                    | PointerEventType::LEAVE
                    | PointerEventType::OUT => false,
                    PointerEventType::MOVE => has_primary || event.pressure > 0.0,
                    PointerEventType::ENTER | PointerEventType::OVER => false,
                };

                let is_leave_event = matches!(
                    event.event_type,
                    PointerEventType::CANCEL | PointerEventType::LEAVE | PointerEventType::OUT
                );

                let mut pointer_flags: u32 = POINTER_FLAG_PRIMARY;
                let mut button_change_type: u32 = POINTER_CHANGE_NONE;
                let mut pen_flags: u32 = PEN_FLAG_NONE;

                // Handle Barrel button and Eraser flags
                if has_secondary {
                    pen_flags |= PEN_FLAG_BARREL;
                }
                if has_eraser {
                    pen_flags |= PEN_FLAG_ERASER | PEN_FLAG_INVERTED;
                }

                if is_leave_event {
                    // Pen completely leaves the tablet detection area
                    if self.pen_tracker.in_contact {
                        // Lift contact first
                        let mut p_info = POINTER_TYPE_INFO {
                            type_: PT_PEN,
                            u: unsafe { std::mem::zeroed() },
                        };
                        unsafe {
                            *p_info.u.penInfo_mut() = POINTER_PEN_INFO {
                                pointerInfo: POINTER_INFO {
                                    pointerType: PT_PEN,
                                    pointerId: 0,
                                    frameId: 0,
                                    pointerFlags: POINTER_FLAG_PRIMARY | POINTER_FLAG_INRANGE | POINTER_FLAG_UP,
                                    sourceDevice: std::ptr::null_mut(),
                                    hwndTarget: 0 as HWND,
                                    ptPixelLocation: POINT { x, y },
                                    ptHimetricLocation: POINT { x: 0, y: 0 },
                                    ptPixelLocationRaw: POINT { x, y },
                                    ptHimetricLocationRaw: POINT { x: 0, y: 0 },
                                    dwTime: 0,
                                    historyCount: 1,
                                    InputData: 0,
                                    dwKeyStates: 0,
                                    PerformanceCount: 0,
                                    ButtonChangeType: POINTER_CHANGE_FIRSTBUTTON_UP,
                                },
                                penFlags: pen_flags,
                                penMask: PEN_MASK_PRESSURE,
                                pressure: 0,
                                rotation: (event.twist as u32) % 360,
                                tiltX: event.tilt_x.clamp(-90, 90),
                                tiltY: event.tilt_y.clamp(-90, 90),
                            };
                            InjectSyntheticPointerInput(self.pointer_device_handle, &p_info, 1);
                        }
                    }

                    // Send out of range update
                    pointer_flags |= POINTER_FLAG_UPDATE; // no INRANGE
                    self.pen_tracker.in_range = false;
                    self.pen_tracker.in_contact = false;
                    self.pen_tracker.barrel_down = false;
                    self.pen_tracker.eraser_down = false;
                } else if is_contact_event {
                    // Contact down / drag
                    pointer_flags |= POINTER_FLAG_INRANGE | POINTER_FLAG_INCONTACT;
                    if !self.pen_tracker.in_contact {
                        pointer_flags |= POINTER_FLAG_DOWN;
                        button_change_type = if has_secondary {
                            POINTER_CHANGE_SECONDBUTTON_DOWN
                        } else {
                            POINTER_CHANGE_FIRSTBUTTON_DOWN
                        };
                        self.pen_tracker.in_contact = true;
                    } else {
                        pointer_flags |= POINTER_FLAG_UPDATE;
                        if has_secondary != self.pen_tracker.barrel_down {
                            button_change_type = if has_secondary {
                                POINTER_CHANGE_SECONDBUTTON_DOWN
                            } else {
                                POINTER_CHANGE_SECONDBUTTON_UP
                            };
                        }
                    }
                    self.pen_tracker.in_range = true;
                } else {
                    // In-Range Hovering (no contact)
                    pointer_flags |= POINTER_FLAG_INRANGE;
                    if self.pen_tracker.in_contact {
                        pointer_flags |= POINTER_FLAG_UP;
                        button_change_type = if self.pen_tracker.barrel_down {
                            POINTER_CHANGE_SECONDBUTTON_UP
                        } else {
                            POINTER_CHANGE_FIRSTBUTTON_UP
                        };
                        self.pen_tracker.in_contact = false;
                    } else {
                        pointer_flags |= POINTER_FLAG_UPDATE;
                    }
                    self.pen_tracker.in_range = true;
                }

                self.pen_tracker.barrel_down = has_secondary;
                self.pen_tracker.eraser_down = has_eraser;

                // Pressure calculation with sensitivity curve
                let pressure_val = if self.pen_tracker.in_contact {
                    let mut p = event.pressure.clamp(0.0, 1.0);
                    if self.pressure_sensitivity > 0.0 && (self.pressure_sensitivity - 1.0).abs() > 0.01 {
                        p = p.powf(1.0 / self.pressure_sensitivity);
                    }
                    ((p * 1024.0).round() as u32).clamp(1, 1024)
                } else {
                    0
                };

                unsafe {
                    let mut pointer_type_info = POINTER_TYPE_INFO {
                        type_: PT_PEN,
                        u: std::mem::zeroed(),
                    };
                    *pointer_type_info.u.penInfo_mut() = POINTER_PEN_INFO {
                        pointerInfo: POINTER_INFO {
                            pointerType: PT_PEN,
                            pointerId: 0, // Single synthetic pen device slot
                            frameId: 0,
                            pointerFlags: pointer_flags,
                            sourceDevice: std::ptr::null_mut(),
                            hwndTarget: 0 as HWND,
                            ptPixelLocation: POINT { x, y },
                            ptHimetricLocation: POINT { x: 0, y: 0 },
                            ptPixelLocationRaw: POINT { x, y },
                            ptHimetricLocationRaw: POINT { x: 0, y: 0 },
                            dwTime: 0,
                            historyCount: 1,
                            InputData: 0,
                            dwKeyStates: 0,
                            PerformanceCount: 0,
                            ButtonChangeType: button_change_type,
                        },
                        penFlags: pen_flags,
                        penMask: PEN_MASK_PRESSURE
                            | PEN_MASK_ROTATION
                            | PEN_MASK_TILT_X
                            | PEN_MASK_TILT_Y,
                        pressure: pressure_val,
                        rotation: (event.twist as u32) % 360,
                        tiltX: event.tilt_x.clamp(-90, 90),
                        tiltY: event.tilt_y.clamp(-90, 90),
                    };
                    InjectSyntheticPointerInput(self.pointer_device_handle, &pointer_type_info, 1);
                }
            }
            PointerType::Touch => {
                let mut pointer_flags = match event.event_type {
                    PointerEventType::DOWN => {
                        POINTER_FLAG_INRANGE | POINTER_FLAG_INCONTACT | POINTER_FLAG_DOWN
                    }
                    PointerEventType::MOVE | PointerEventType::OVER | PointerEventType::ENTER => {
                        POINTER_FLAG_INRANGE | POINTER_FLAG_UPDATE
                    }
                    PointerEventType::UP => POINTER_FLAG_UP,
                    PointerEventType::CANCEL | PointerEventType::LEAVE | PointerEventType::OUT => {
                        POINTER_FLAG_INRANGE | POINTER_FLAG_UPDATE | POINTER_FLAG_CANCELED
                    }
                };
                let button_change_type = match event.buttons {
                    Button::PRIMARY => {
                        pointer_flags |= POINTER_FLAG_INCONTACT;
                        POINTER_CHANGE_FIRSTBUTTON_DOWN
                    }
                    Button::SECONDARY => POINTER_CHANGE_SECONDBUTTON_DOWN,
                    Button::AUXILARY => POINTER_CHANGE_THIRDBUTTON_DOWN,
                    Button::NONE => POINTER_CHANGE_NONE,
                    _ => POINTER_CHANGE_NONE,
                };
                if event.is_primary {
                    pointer_flags |= POINTER_FLAG_PRIMARY;
                }

                unsafe {
                    let mut pointer_type_info = POINTER_TYPE_INFO {
                        type_: PT_TOUCH,
                        u: std::mem::zeroed(),
                    };

                    let mut pointer_touch_info: POINTER_TOUCH_INFO = std::mem::zeroed();
                    pointer_touch_info.pointerInfo = std::mem::zeroed();
                    pointer_touch_info.pointerInfo.pointerType = PT_TOUCH;
                    pointer_touch_info.pointerInfo.pointerFlags = pointer_flags;
                    pointer_touch_info.pointerInfo.pointerId = (event.pointer_id.abs() % 5) as u32;
                    pointer_touch_info.pointerInfo.ptPixelLocation = POINT { x, y };
                    pointer_touch_info.touchFlags = TOUCH_FLAG_NONE;
                    pointer_touch_info.touchMask = TOUCH_MASK_PRESSURE;
                    pointer_touch_info.pressure = (event.pressure * 1024f64) as u32;
                    pointer_touch_info.pointerInfo.ButtonChangeType = button_change_type;

                    *pointer_type_info.u.touchInfo_mut() = pointer_touch_info;
                    self.multitouch_map
                        .insert(event.pointer_id, pointer_type_info);
                    let len = self.multitouch_map.len();

                    let mut pointer_type_info_vec: Vec<POINTER_TYPE_INFO> = Vec::new();
                    for (_i, info) in self.multitouch_map.iter() {
                        pointer_type_info_vec.push(*info);
                    }
                    let b: Box<[POINTER_TYPE_INFO]> = pointer_type_info_vec.into_boxed_slice();
                    let m: *mut POINTER_TYPE_INFO = Box::into_raw(b) as _;

                    InjectSyntheticPointerInput(self.touch_device_handle, m, len as u32);

                    match event.event_type {
                        PointerEventType::DOWN
                        | PointerEventType::MOVE
                        | PointerEventType::OVER
                        | PointerEventType::ENTER => {}

                        PointerEventType::UP
                        | PointerEventType::CANCEL
                        | PointerEventType::LEAVE
                        | PointerEventType::OUT => {
                            self.multitouch_map.remove(&event.pointer_id);
                        }
                    }
                }
            }
            PointerType::Mouse => {
                let mut dw_flags = 0;

                match event.event_type {
                    PointerEventType::DOWN => match event.buttons {
                        Button::PRIMARY => {
                            dw_flags |= MOUSEEVENTF_LEFTDOWN;
                        }
                        Button::SECONDARY => {
                            dw_flags |= MOUSEEVENTF_RIGHTDOWN;
                        }
                        Button::AUXILARY => {
                            dw_flags |= MOUSEEVENTF_MIDDLEDOWN;
                        }
                        _ => {}
                    },
                    PointerEventType::MOVE | PointerEventType::OVER | PointerEventType::ENTER => {
                        unsafe { SetCursorPos(screen_x, screen_y) };
                    }
                    PointerEventType::UP => match event.button {
                        Button::PRIMARY => {
                            dw_flags |= MOUSEEVENTF_LEFTUP;
                        }
                        Button::SECONDARY => {
                            dw_flags |= MOUSEEVENTF_RIGHTUP;
                        }
                        Button::AUXILARY => {
                            dw_flags |= MOUSEEVENTF_MIDDLEUP;
                        }
                        _ => {}
                    },
                    PointerEventType::CANCEL | PointerEventType::LEAVE | PointerEventType::OUT => {
                        dw_flags |= MOUSEEVENTF_LEFTUP;
                    }
                }
                unsafe { mouse_event(dw_flags, 0, 0, 0, 0) };
            }
            PointerType::Unknown => {}
        }
    }

    fn send_keyboard_event(&mut self, event: &KeyboardEvent) {
        self.autopilot_device.send_keyboard_event(event);
    }

    fn set_capturable(&mut self, capturable: Box<dyn Capturable>) {
        self.capturable = capturable.clone();
        self.autopilot_device.set_capturable(capturable);
        self.pen_tracker = PenStateTracker::default();
        self.multitouch_map.clear();
    }

    fn device_type(&self) -> InputDeviceType {
        InputDeviceType::WindowsInput
    }
}
