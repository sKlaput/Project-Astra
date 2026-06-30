// ---------------------------------------------------------------------------
// Astra OS — Settings app
//
// Multi-tab layout:
//   [System]  — hardware/build info  (read-only)
//   [Display] — desktop background colour picker
//   [Input]   — keyboard/mouse info (placeholder)
//   [About]   — project info + runtime stats
//
// Tab bar on the left; content on the right.
// Arrow keys navigate within a tab; Tab switches tabs.
// ---------------------------------------------------------------------------

use crate::app::{App, AppAction};
use crate::framebuffer;
use crate::input::Key;
use core::sync::atomic::Ordering as AO;

// ── Colours ───────────────────────────────────────────────────────────────────

const BG: u32 = 0x0A0E14;
const SIDEBAR_BG: u32 = 0x080C10;
const SEP: u32 = 0x1E3A5F;
const HEADING: u32 = 0x4FC3F7;
const LABEL: u32 = 0x546E7A;
const VALUE: u32 = 0xE8F4FD;
const ACCENT: u32 = 0xB0D4B8;
const TAB_SEL_BG: u32 = 0x1A2E44;
const TAB_SEL_TXT: u32 = 0xFFFFFF;
const TAB_TXT: u32 = 0x7090B0;
const HINT: u32 = 0x2A4060;
const SWATCH_SEL: u32 = 0xFFFFFF;

// ── Font metrics ──────────────────────────────────────────────────────────────

const SC: usize = 2;
const CW: usize = 6 * SC; // 12
const CH: usize = 8 * SC; // 16

// ── Layout ────────────────────────────────────────────────────────────────────

const SIDEBAR_W: usize = 110;
const PAD: usize = 12;
const TAB_H: usize = 28;

// ── Tabs ──────────────────────────────────────────────────────────────────────

const NUM_TABS: usize = 4;
const TAB_LABELS: [&str; NUM_TABS] = ["System", "Display", "Input", "About"];

// ── System info ───────────────────────────────────────────────────────────────

const NUM_SYSINFO: usize = 5;
struct KV {
    k: &'static str,
    v: &'static str,
}
const SYSINFO: [KV; NUM_SYSINFO] = [
    KV {
        k: "Version",
        v: "Astra OS v0.1",
    },
    KV {
        k: "Architecture",
        v: "x86_64",
    },
    KV {
        k: "Bootloader",
        v: "Limine (UEFI)",
    },
    KV {
        k: "Resolution",
        v: "1280x800 32bpp",
    },
    KV {
        k: "Timer",
        v: "PIT @ 100 Hz",
    },
];

// ── Display: background colour presets ────────────────────────────────────────

const NUM_THEMES: usize = 8;
const THEMES: [(u32, &str); NUM_THEMES] = [
    (0x0D1117, "Deep Space"),
    (0x071207, "Forest Night"),
    (0x0D0714, "Nebula"),
    (0x14070B, "Ember"),
    (0x060E18, "Ocean"),
    (0x141210, "Warm Slate"),
    (0x050508, "Midnight"),
    (0x0F1218, "Steel"),
];

// ── App struct ────────────────────────────────────────────────────────────────

pub struct SettingsApp {
    tab: usize,
    row: usize,
}

include!("settings/core.rs");
include!("settings/app_impl.rs");
include!("settings/sections.rs");
include!("settings/formatting.rs");
