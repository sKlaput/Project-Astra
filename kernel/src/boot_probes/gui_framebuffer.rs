use core::sync::atomic::{AtomicU64, Ordering};

use crate::{arch, fs, idle, loader, memory, scheduler, serial, syscall, user};

mod apps_v0;
mod demo_window;
mod fb_mapping;
mod feature_flags;

pub(crate) use apps_v0::{
    probe_app_file_manager_v0, probe_app_settings_v0, probe_app_terminal_v0,
    probe_app_text_editor_v0,
};
pub(crate) use demo_window::{probe_gui_demo, probe_gui_window_manager};
pub(crate) use fb_mapping::{probe_gui_fb_mapping, probe_gui_fb_mapping_user_task};
pub(crate) use feature_flags::{GUI_FB_DEEP_PROBE, GUI_FB_USER_DEEP_PROBE};
