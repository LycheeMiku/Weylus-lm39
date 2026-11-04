use std::boxed::Box;
use std::error::Error;
#[allow(unused_imports)]
use tracing::warn;

#[cfg(target_os = "macos")]
pub mod core_graphics;
#[cfg(target_os = "linux")]
pub mod pipewire;
#[cfg(target_os = "linux")]
#[allow(dead_code)]
pub mod remote_desktop_dbus;
pub mod testsrc;

#[cfg(target_os = "windows")]
pub mod captrs_capture;
#[cfg(target_os = "windows")]
pub mod win_ctx;
#[cfg(target_os = "linux")]
pub mod x11;
pub trait Recorder {
    fn capture(&mut self) -> Result<crate::video::PixelProvider<'_>, Box<dyn Error>>;
}

pub trait BoxCloneCapturable {
    fn box_clone(&self) -> Box<dyn Capturable>;
}

impl<T> BoxCloneCapturable for T
where
    T: Clone + Capturable + 'static,
{
    fn box_clone(&self) -> Box<dyn Capturable> {
        Box::new(self.clone())
    }
}
/// Relative: x, y, width, height of the Capturable as floats relative to the absolute size of the
/// screen. For example x=0.5, y=0.0, width=0.5, height=1.0 means the right half of the screen.
/// VirtualScreen: offset_x, offset_y, width, height for a capturable using a virtual screen. (Windows)
pub enum Geometry {
    Relative(f64, f64, f64, f64),
    #[cfg(target_os = "windows")]
    VirtualScreen(i32, i32, u32, u32, i32, i32),
}

pub trait Capturable: Send + BoxCloneCapturable {
    /// Name of the Capturable, for example the window title, if it is a window.
    fn name(&self) -> String;

    /// Return Geometry of the Capturable.
    fn geometry(&self) -> Result<Geometry, Box<dyn Error>>;

    /// Callback that is called right before input is simulated.
    /// Useful to focus the window on input.
    fn before_input(&mut self) -> Result<(), Box<dyn Error>>;

    /// Return a Recorder that can record the current capturable.
    fn recorder(&self, capture_cursor: bool) -> Result<Box<dyn Recorder>, Box<dyn Error>>;
}

impl Clone for Box<dyn Capturable> {
    fn clone(&self) -> Self {
        self.box_clone()
    }
}

pub fn get_capturables(
    #[cfg(target_os = "linux")] wayland_support: bool,
    #[cfg(target_os = "linux")] capture_cursor: bool,
) -> Vec<Box<dyn Capturable>> {
    let mut capturables: Vec<Box<dyn Capturable>> = vec![];
    #[cfg(target_os = "linux")]
    {
        if wayland_support {
            use crate::capturable::pipewire::get_capturables as get_capturables_pw;
            match get_capturables_pw(capture_cursor) {
                Ok(captrs) => {
                    for c in captrs {
                        capturables.push(Box::new(c));
                    }
                }
                Err(err) => warn!(
                    "Failed to get list of capturables via dbus/pipewire: {}",
                    err
                ),
            }
        }

        use crate::capturable::x11::X11Context;
        let x11ctx = X11Context::new();
        if let Some(mut x11ctx) = x11ctx {
            match x11ctx.capturables() {
                Ok(captrs) => {
                    for c in captrs {
                        capturables.push(Box::new(c));
                    }
                }
                Err(err) => warn!("Failed to get list of capturables via X11: {}", err),
            }
        };
    }

    #[cfg(target_os = "macos")]
    {
        use crate::capturable::core_graphics::get_displays as get_displays_cg;
        use crate::capturable::core_graphics::get_windows as get_windows_cg;
        match get_displays_cg() {
            Ok(captrs) => {
                for c in captrs {
                    capturables.push(Box::new(c));
                }
            }
            Err(err) => warn!("Failed to get list of displays via CoreGraphics: {}", err),
        }

        match get_windows_cg() {
            Ok(mut captrs) => {
                captrs.sort_by(|a, b| a.name().to_lowercase().cmp(&b.name().to_lowercase()));
                for c in captrs {
                    capturables.push(Box::new(c));
                }
            }
            Err(err) => warn!("Failed to get list of windows via CoreGraphics: {}", err),
        }
    }

    #[cfg(target_os = "windows")]
    {
        use crate::capturable::captrs_capture::CaptrsCapturable;
        use crate::capturable::win_ctx::WinCtx;
        let winctx = WinCtx::new();
        let union_rect = winctx.get_union_rect().clone();

        let mut outputs = winctx.get_outputs().clone();
        // Sort outputs: primary display (DesktopCoordinates.left == 0 && top == 0) first,
        // then left-to-right, top-to-bottom
        outputs.sort_by_key(|o| {
            let is_primary = o.DesktopCoordinates.left == 0 && o.DesktopCoordinates.top == 0;
            (
                if is_primary { 0 } else { 1 },
                o.DesktopCoordinates.left,
                o.DesktopCoordinates.top,
            )
        });

        for (i, o) in outputs.iter().enumerate() {
            let is_primary = o.DesktopCoordinates.left == 0 && o.DesktopCoordinates.top == 0;
            let w = (o.DesktopCoordinates.right - o.DesktopCoordinates.left).abs();
            let h = (o.DesktopCoordinates.bottom - o.DesktopCoordinates.top).abs();
            let primary_tag = if is_primary { " [Primary]" } else { "" };
            let captr = CaptrsCapturable::new(
                (i + 1) as u8,
                format!("Display {} ({}x{}){}", i + 1, w, h, primary_tag),
                o.DesktopCoordinates,
                union_rect,
            );
            capturables.push(Box::new(captr));
        }

        if outputs.len() > 1 {
            let total_w = (union_rect.right - union_rect.left).abs();
            let total_h = (union_rect.bottom - union_rect.top).abs();
            capturables.push(Box::new(CaptrsCapturable::new(
                0,
                format!("All Displays ({}x{} Span)", total_w, total_h),
                union_rect,
                union_rect,
            )));
        }
    }

    capturables
}
