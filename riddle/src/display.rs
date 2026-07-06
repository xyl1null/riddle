//! Display backends: qtfb (windowed, inside xochitl), quill (takeover,
//! vendor engine, xochitl stopped), and a macOS desktop window for local
//! development. Selected at runtime: QTFB_KEY picks AppLoad, macOS defaults to
//! desktop, otherwise takeover is used when compiled in.

use crate::surface::{PixFmt, Surface};
use std::io;

pub enum Display {
    Qtfb(crate::qtfb::QtfbClient),
    #[cfg(target_os = "macos")]
    Desktop(crate::desktop::DesktopDisplay),
    #[allow(dead_code)]
    Quill,
}

// C ABI from libquill.so (linked when built with --features takeover).
#[cfg(all(feature = "takeover", target_os = "linux"))]
mod quill_ffi {
    extern "C" {
        pub fn quill_init() -> i32;
        pub fn quill_width() -> i32;
        pub fn quill_height() -> i32;
        pub fn quill_stride() -> i32;
        pub fn quill_buffer() -> *mut u8;
        pub fn quill_swap(x: i32, y: i32, w: i32, h: i32, mode: i32, full: i32) -> u64;
        pub fn quill_process_events();
    }
}

impl Display {
    pub fn name(&self) -> &'static str {
        match self {
            Display::Qtfb(_) => "qtfb",
            #[cfg(target_os = "macos")]
            Display::Desktop(_) => "desktop",
            Display::Quill => "quill/takeover",
        }
    }

    pub fn open() -> io::Result<(Self, Surface)> {
        if let Ok(key) = std::env::var("QTFB_KEY") {
            let key: i32 = key.parse().map_err(io::Error::other)?;
            let mut client = crate::qtfb::QtfbClient::connect(
                key,
                crate::qtfb::FBFMT_RMPP_RGB565,
                1620,
                2160,
                2,
            )?;
            let _ = client.set_refresh_mode(crate::qtfb::REFRESH_MODE_UFAST);
            let buf = client.framebuffer();
            let (ptr, len) = (buf.as_mut_ptr(), buf.len());
            let surface = Surface::new(ptr, len, 1620, 2160, 1620 * 2, PixFmt::Rgb565);
            return Ok((Display::Qtfb(client), surface));
        }

        #[cfg(target_os = "macos")]
        {
            let (desktop, surface) = crate::desktop::DesktopDisplay::open()?;
            Ok((Display::Desktop(desktop), surface))
        }

        #[cfg(all(feature = "takeover", target_os = "linux"))]
        {
            unsafe {
                if quill_ffi::quill_init() != 0 {
                    return Err(io::Error::other("quill_init failed"));
                }
                let w = quill_ffi::quill_width() as usize;
                let h = quill_ffi::quill_height() as usize;
                let stride = quill_ffi::quill_stride() as usize;
                let ptr = quill_ffi::quill_buffer();
                if ptr.is_null() {
                    return Err(io::Error::other("quill buffer null"));
                }
                let surface = Surface::new(ptr, stride * h, w, h, stride, PixFmt::Rgb32);
                Ok((Display::Quill, surface))
            }
        }
        #[cfg(not(any(target_os = "macos", all(feature = "takeover", target_os = "linux"))))]
        Err(io::Error::other(
            "QTFB_KEY not set and this build has no takeover backend",
        ))
    }

    /// Push a region to the panel. `fast` selects the low-latency waveform.
    pub fn update(&mut self, x: i32, y: i32, w: i32, h: i32, _fast: bool) {
        match self {
            Display::Qtfb(c) => {
                let _ = c.update_partial(x, y, w, h);
            }
            #[cfg(target_os = "macos")]
            Display::Desktop(d) => {
                d.update_region(x, y, w, h);
            }
            #[allow(unused_variables)]
            Display::Quill => {
                #[cfg(all(feature = "takeover", target_os = "linux"))]
                unsafe {
                    // mode 0 = fastest (ink), 3 = balanced (text/anim)
                    quill_ffi::quill_swap(x, y, w, h, if _fast { 0 } else { 3 }, 0);
                    quill_ffi::quill_process_events();
                }
            }
        }
    }

    pub fn update_all(&mut self, w: usize, h: usize) {
        match self {
            Display::Qtfb(c) => {
                let _ = c.update_all();
            }
            #[cfg(target_os = "macos")]
            Display::Desktop(d) => {
                d.update_all();
            }
            #[allow(unused_variables)]
            Display::Quill => {
                #[cfg(all(feature = "takeover", target_os = "linux"))]
                unsafe {
                    quill_ffi::quill_swap(0, 0, w as i32, h as i32, 3, 0);
                    quill_ffi::quill_process_events();
                }
            }
        }
        let _ = (w, h);
    }

    /// Flashing clear of the whole panel (ghost removal).
    pub fn full_refresh(&mut self, w: usize, h: usize) {
        match self {
            Display::Qtfb(c) => {
                let _ = c.request_full_refresh();
            }
            #[cfg(target_os = "macos")]
            Display::Desktop(d) => {
                d.full_refresh();
            }
            #[allow(unused_variables)]
            Display::Quill => {
                #[cfg(all(feature = "takeover", target_os = "linux"))]
                unsafe {
                    quill_ffi::quill_swap(0, 0, w as i32, h as i32, 4, 1);
                    quill_ffi::quill_process_events();
                }
            }
        }
        let _ = (w, h);
    }

    /// Drain window-system events. For qtfb this also detects window close
    /// (returns Err); the takeover backend has no window to lose.
    pub fn pump(&mut self) -> io::Result<Vec<crate::qtfb::InputEvent>> {
        match self {
            Display::Qtfb(c) => c.drain_events(),
            #[cfg(target_os = "macos")]
            Display::Desktop(d) => d.pump(),
            Display::Quill => {
                #[cfg(all(feature = "takeover", target_os = "linux"))]
                unsafe {
                    quill_ffi::quill_process_events();
                }
                Ok(Vec::new())
            }
        }
    }

    pub fn terminate(&mut self) {
        match self {
            Display::Qtfb(c) => c.terminate(),
            #[cfg(target_os = "macos")]
            Display::Desktop(_) => {}
            Display::Quill => {}
        }
    }
}
