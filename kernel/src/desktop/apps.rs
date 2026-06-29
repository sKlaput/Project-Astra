// ── Unified app registry ─────────────────────────────────────────────────────
//
// Single source of truth for all launch paths.
// `app_id`   – must match App::app_id() in the corresponding module.
// `label`    – shown on desktop icon, launcher title, and taskbar button.
// `icon_sub` – short subtitle drawn inside the icon cell (≤10 chars).
// `desc`     – one-line description shown in the launcher panel.

struct AppDesc {
    app_id: &'static str,
    label: &'static str,
    icon_sub: &'static str,
    desc: &'static str,
    /// Factory: creates a fresh instance of this app.
    /// The compositor calls this every time the icon is activated.
    make: fn() -> Box<dyn App>,
}

const APP_REGISTRY: [AppDesc; NUM_APPS] = [
    AppDesc {
        app_id: "terminal",
        label: "Terminal",
        icon_sub: "Shell",
        desc: "Open a shell",
        make: || Box::new(TerminalApp::new()),
    },
    AppDesc {
        app_id: "filemanager",
        label: "This PC",
        icon_sub: "ThisPc",
        desc: "Browse files",
        make: || Box::new(FileManagerApp::new()),
    },
    AppDesc {
        app_id: "settings",
        label: "Settings",
        icon_sub: "Config",
        desc: "Preferences",
        make: || Box::new(SettingsApp::new()),
    },
    AppDesc {
        app_id: "sysmonitor",
        label: "Sys Monitor",
        icon_sub: "Stats",
        desc: "Performance",
        make: || Box::new(SysMonitorApp::new()),
    },
    AppDesc {
        app_id: "calculator",
        label: "Calculator",
        icon_sub: "Calc",
        desc: "4-function calc",
        make: || Box::new(CalculatorApp::new()),
    },
    AppDesc {
        app_id: "imageviewer",
        label: "Viewer",
        icon_sub: "Images",
        desc: "View PPM images",
        make: || Box::new(ImageViewerApp::new()),
    },
    AppDesc {
        app_id: "notes",
        label: "Notes",
        icon_sub: "Notepad",
        desc: "Scratchpad",
        make: || Box::new(NotesApp::new()),
    },
    AppDesc {
        app_id: "logviewer",
        label: "Log Viewer",
        icon_sub: "Logs",
        desc: "Kernel log",
        make: || Box::new(LogViewerApp::new()),
    },
    AppDesc {
        app_id: "about",
        label: "About",
        icon_sub: "Info",
        desc: "About Astra OS",
        make: || Box::new(AboutApp::new()),
    },
    AppDesc {
        app_id: "snake",
        label: "Snake",
        icon_sub: "Game",
        desc: "Classic snake",
        make: || Box::new(SnakeApp::new()),
    },
    AppDesc {
        app_id: "tetris",
        label: "Tetris",
        icon_sub: "Game",
        desc: "Classic Tetris",
        make: || Box::new(TetrisApp::new()),
    },
];

// ── Desktop icons ─────────────────────────────────────────────────────────────

struct DesktopIcon {
    x: i32,
    y: i32,
    selected: bool,
    last_click_ms: u64,
}

fn icon_rect(row: usize, bar_h: usize) -> Rect {
    Rect {
        x: ICON_GRID_X,
        y: bar_h + ICON_GRID_Y + row * (ICON_CELL_H + 6),
        w: ICON_CELL_W,
        h: ICON_CELL_H,
    }
}

fn icon_rect_of(icon: &DesktopIcon) -> Rect {
    Rect {
        x: icon.x as usize,
        y: icon.y as usize,
        w: ICON_CELL_W,
        h: ICON_CELL_H,
    }
}

fn launcher_item_rect(i: usize) -> Rect {
    Rect {
        x: 0,
        y: BAR_H + LAUNCHER_HEAD_H + i * LAUNCHER_ITEM_H,
        w: LAUNCHER_W,
        h: LAUNCHER_ITEM_H,
    }
}

fn launcher_rect(sh: usize) -> Rect {
    Rect {
        x: 0,
        y: BAR_H,
        w: LAUNCHER_W,
        h: sh.saturating_sub(BAR_H),
    }
}

// ── App pixel-art icons ───────────────────────────────────────────────────────
//
// Each icon is drawn with fill_rect only, inside a 44×28 area centred in the
// icon cell above the label text. Origin passed as (ix, iy).
//
//  0 – Terminal     : monitor frame with ">_" prompt
//  1 – This PC      : computer monitor with stand
//  2 – Settings     : gear with cardinal + diagonal teeth
//  3 – Sys Monitor  : bar chart with Y/X axes
//  4 – Calculator   : frame with display + button grid
//  5 – Image Viewer : picture frame with sky/mountain scene
//  6 – Notes        : notepad with binding strip + ruled lines
//  7 – Log Viewer   : monitor with scrolling log lines
//  8 – About        : info “i” badge
//  9 – Snake        : game grid with S-shaped snake + apple
// 10 – Tetris        : coloured block stack in a well
// 100 – Desktop File   : document page with folded corner + text lines
// 101 – Desktop Folder : classic folder shape with tab and depth shadow

fn draw_app_icon(idx: usize, r: Rect) {
    // Centre the 44×28 drawing area horizontally; start 7px below accent strip.
    let iw: usize = 44;
    let ih: usize = 28;
    let ix = r.x + (r.w.saturating_sub(iw)) / 2;
    let iy = r.y + 7;

    match idx {
        // ── 0: Terminal ───────────────────────────────────────────────────
        // Outer monitor frame (dark blue-grey)
        // Inner screen (deep green-black)
        // ">_" prompt in green
        0 => {
            const FRAME: u32 = 0x2A4060;
            const SCREEN: u32 = 0x061008;
            const GREEN: u32 = 0x00CC66;
            const CURSOR: u32 = 0x00FF88;
            // Monitor frame
            framebuffer::fill_rect(ix, iy, iw, ih - 4, FRAME);
            // Screen inset (2px border)
            framebuffer::fill_rect(ix + 2, iy + 2, iw - 4, ih - 8, SCREEN);
            // Stand neck
            framebuffer::fill_rect(ix + iw / 2 - 3, iy + ih - 4, 6, 2, FRAME);
            // Stand base
            framebuffer::fill_rect(ix + iw / 2 - 7, iy + ih - 2, 14, 2, FRAME);
            // ">" chevron — two diagonal 2×2 pixel blocks each side
            let px = ix + 4;
            let py = iy + 6;
            framebuffer::fill_rect(px, py, 2, 2, GREEN); // top-left arm
            framebuffer::fill_rect(px + 2, py + 2, 2, 2, GREEN); // point top
            framebuffer::fill_rect(px, py + 4, 2, 2, GREEN); // bottom-left arm
                                                             // underscore cursor (blinks visually via colour difference)
            framebuffer::fill_rect(px + 4, py + 4, 6, 2, CURSOR);
        }

        // ── 1: This PC (Computer monitor) ────────────────────────────────
        1 => {
            const FRAME: u32 = 0x2A5080;
            const SCREEN: u32 = 0x0A1828;
            const GLOW: u32 = 0x1A6090;
            const STAND: u32 = 0x1E3A58;
            const BASE: u32 = 0x182E48;
            // Monitor outer frame
            framebuffer::fill_rect(ix, iy, iw, ih - 6, FRAME);
            // Screen inset
            framebuffer::fill_rect(ix + 2, iy + 2, iw - 4, ih - 10, SCREEN);
            // Screen glow line at top of screen
            framebuffer::fill_rect(ix + 2, iy + 2, iw - 4, 2, GLOW);
            // Stand neck
            framebuffer::fill_rect(ix + iw / 2 - 2, iy + ih - 6, 4, 3, STAND);
            // Stand base
            framebuffer::fill_rect(ix + iw / 2 - 8, iy + ih - 3, 16, 3, BASE);
            // Small HDD cylinder at bottom-right of screen
            framebuffer::fill_rect(ix + iw - 10, iy + ih - 12, 6, 4, STAND);
            framebuffer::fill_rect(ix + iw - 9, iy + ih - 13, 4, 2, FRAME);
        }

        // ── 2: Settings (Gear) ────────────────────────────────────────────
        // Hub square + 4 cardinal teeth + 4 diagonal teeth + dark hole
        2 => {
            const GEAR: u32 = 0x5090C8;
            const GEAR2: u32 = 0x70B0E0;
            const HOLE: u32 = 0x0A1020;
            let cx = ix + iw / 2;
            let cy = iy + ih / 2 - 1;
            // Cardinal teeth (wider)
            framebuffer::fill_rect(cx - 3, cy - 11, 6, 5, GEAR); // top
            framebuffer::fill_rect(cx - 3, cy + 6, 6, 5, GEAR); // bottom
            framebuffer::fill_rect(cx - 11, cy - 3, 5, 6, GEAR); // left
            framebuffer::fill_rect(cx + 6, cy - 3, 5, 6, GEAR); // right
                                                                // Hub body
            framebuffer::fill_rect(cx - 7, cy - 7, 14, 14, GEAR);
            // Diagonal teeth (narrower)
            framebuffer::fill_rect(cx - 9, cy - 9, 4, 4, GEAR2);
            framebuffer::fill_rect(cx + 5, cy - 9, 4, 4, GEAR2);
            framebuffer::fill_rect(cx - 9, cy + 5, 4, 4, GEAR2);
            framebuffer::fill_rect(cx + 5, cy + 5, 4, 4, GEAR2);
            // Centre hole
            framebuffer::fill_rect(cx - 3, cy - 3, 6, 6, HOLE);
        }

        // ── 3: Sys Monitor (Bar chart) ────────────────────────────────────
        // Y axis + X axis + 4 bars at different heights
        3 => {
            const AXIS: u32 = 0x304860;
            const BAR1: u32 = 0x00A8C0;
            const BAR2: u32 = 0x00C8A0;
            const BAR3: u32 = 0x0080E0;
            const BAR4: u32 = 0x40D0FF;
            let base_y = iy + ih - 4; // X-axis y
                                      // Y axis
            framebuffer::fill_rect(ix + 2, iy + 1, 2, ih - 4, AXIS);
            // X axis
            framebuffer::fill_rect(ix + 2, base_y - 2, iw - 4, 2, AXIS);
            // Bar 1 — medium
            framebuffer::fill_rect(ix + 6, base_y - 10, 6, 8, BAR1);
            // Bar 2 — tall
            framebuffer::fill_rect(ix + 14, base_y - 16, 6, 14, BAR2);
            // Bar 3 — short
            framebuffer::fill_rect(ix + 22, base_y - 8, 6, 6, BAR3);
            // Bar 4 — tallest
            framebuffer::fill_rect(ix + 30, base_y - 20, 6, 18, BAR4);
        }

        // ── 4: Calculator — grid of buttons with "=" accent ──────────────
        4 => {
            const FRAME: u32 = 0x1A2F48;
            const BTN_C: u32 = 0x243448;
            const BTN_OP: u32 = 0x1A3A5F;
            const BTN_EQ: u32 = 0x1A5F3F;
            const DIGIT: u32 = 0x90B8D8;
            // Calculator body
            framebuffer::fill_rect(ix, iy, iw, ih, FRAME);
            // Display strip
            framebuffer::fill_rect(ix + 2, iy + 2, iw - 4, 7, 0x060B10);
            // "=" character hint in display
            framebuffer::fill_rect(ix + iw - 8, iy + 3, 4, 1, DIGIT);
            framebuffer::fill_rect(ix + iw - 8, iy + 5, 4, 1, DIGIT);
            // Button grid: 3 cols × 3 rows of small squares
            for row in 0..3usize {
                for col in 0..3usize {
                    let bx = ix + 2 + col * 8;
                    let by = iy + 11 + row * 6;
                    let bg = if col == 2 && row == 2 {
                        BTN_EQ
                    } else if col == 2 {
                        BTN_OP
                    } else {
                        BTN_C
                    };
                    framebuffer::fill_rect(bx, by, 6, 4, bg);
                }
            }
        }

        // ── 5: Image Viewer — picture frame with a sun/mountain scene ────
        5 => {
            const FRAME_COL: u32 = 0x2A3A50;
            const SKY: u32 = 0x0A2040;
            const GROUND: u32 = 0x1A3020;
            const SUN: u32 = 0xF0B030;
            const MTN: u32 = 0x304858;
            // Outer frame border
            framebuffer::fill_rect(ix, iy, iw, ih, FRAME_COL);
            // Image area inset
            framebuffer::fill_rect(ix + 2, iy + 2, iw - 4, ih - 4, SKY);
            // Ground strip
            framebuffer::fill_rect(ix + 2, iy + ih - 8, iw - 4, 6, GROUND);
            // Sun (small square)
            framebuffer::fill_rect(ix + iw - 10, iy + 4, 5, 5, SUN);
            // Mountain silhouette (two triangles via diagonal lines approximation)
            for row in 0..8usize {
                let w2 = (row * 2).min(iw - 4);
                framebuffer::fill_rect(
                    ix + 2 + (iw - 4).saturating_sub(w2) / 2,
                    iy + (ih - 8) - row - 2,
                    w2,
                    1,
                    MTN,
                );
            }
        }

        // ── 6: Notes — notepad with lines ────────────────────────────────
        6 => {
            const PAGE: u32 = 0x0E1E14;
            const RULE: u32 = 0x1A4028;
            const CURL: u32 = 0x2A6040;
            const BIND: u32 = 0x0A1810;
            // Page background
            framebuffer::fill_rect(ix, iy, iw, ih, PAGE);
            // Binding strip on left
            framebuffer::fill_rect(ix, iy, 6, ih, BIND);
            // Ruled lines
            for row in 0..5usize {
                framebuffer::fill_rect(ix + 8, iy + 6 + row * 4, iw - 10, 1, RULE);
            }
            // Corner curl (top-right triangle approximation)
            framebuffer::fill_rect(ix + iw - 6, iy, 6, 2, CURL);
            framebuffer::fill_rect(ix + iw - 4, iy + 2, 4, 2, CURL);
            framebuffer::fill_rect(ix + iw - 2, iy + 4, 2, 2, CURL);
        }

        // ── 7: Log Viewer — terminal scroll output ────────────────────────
        7 => {
            const FRAME: u32 = 0x0C1810;
            const SCREEN: u32 = 0x050C06;
            const LINE1: u32 = 0x206030;
            const LINE2: u32 = 0x184028;
            const PROMPT: u32 = 0x30A050;
            // Monitor frame
            framebuffer::fill_rect(ix, iy, iw, ih - 4, FRAME);
            // Screen inset
            framebuffer::fill_rect(ix + 2, iy + 2, iw - 4, ih - 8, SCREEN);
            // Log lines  (alternating green shades)
            for row in 0..5usize {
                let col = if row % 2 == 0 { LINE1 } else { LINE2 };
                framebuffer::fill_rect(ix + 4, iy + 4 + row * 3, iw - 8, 2, col);
            }
            // Prompt at bottom
            framebuffer::fill_rect(ix + 4, iy + ih - 8, 8, 2, PROMPT);
            // Stand
            framebuffer::fill_rect(ix + iw / 2 - 3, iy + ih - 4, 6, 2, FRAME);
            framebuffer::fill_rect(ix + iw / 2 - 7, iy + ih - 2, 14, 2, FRAME);
        }

        // ── 8: About — stylised info "i" badge ───────────────────────────
        8 => {
            const BADGE: u32 = 0x0C2040;
            const RING: u32 = 0x1E5090;
            const LETTER: u32 = 0x80C0FF;
            const DOT: u32 = 0xA0D8FF;
            let cx = ix + iw / 2;
            let cy = iy + ih / 2 - 1;
            // Outer ring (circle approx via concentric rects)
            framebuffer::fill_rect(cx - 10, cy - 12, 20, 24, RING);
            framebuffer::fill_rect(cx - 8, cy - 10, 16, 20, BADGE);
            // "i" dot
            framebuffer::fill_rect(cx - 2, cy - 7, 4, 4, DOT);
            // "i" stem
            framebuffer::fill_rect(cx - 2, cy - 1, 4, 9, LETTER);
            // Serif base
            framebuffer::fill_rect(cx - 4, cy + 8, 8, 2, LETTER);
        }

        // ── 9: Snake — winding snake on a dark grid ───────────────────────
        9 => {
            const GRID_BG: u32 = 0x060B06;
            const GRID_L: u32 = 0x0C150C;
            const S_HEAD: u32 = 0x50F870;
            const S_BODY: u32 = 0x28A840;
            const APPLE: u32 = 0xFF4444;
            // Grid background
            framebuffer::fill_rect(ix, iy, iw, ih, GRID_BG);
            // Grid lines (4x4 subcells)
            for g in 0..5usize {
                framebuffer::fill_rect(ix, iy + g * 5, iw, 1, GRID_L);
                framebuffer::fill_rect(ix + g * 8, iy, 1, ih, GRID_L);
            }
            // Snake body — S-shaped winding path
            // Row 1 (left to right)
            framebuffer::fill_rect(ix + 2, iy + 2, 20, 3, S_BODY);
            // Turn down right side
            framebuffer::fill_rect(ix + 19, iy + 2, 3, 9, S_BODY);
            // Row 2 (right to left)
            framebuffer::fill_rect(ix + 4, iy + 8, 18, 3, S_BODY);
            // Turn down left side
            framebuffer::fill_rect(ix + 2, iy + 8, 3, 9, S_BODY);
            // Row 3 (left to right) — tail
            framebuffer::fill_rect(ix + 2, iy + 14, 14, 3, S_BODY);
            // Head (brighter, at end of row 1)
            framebuffer::fill_rect(ix + 2, iy + 2, 5, 3, S_HEAD);
            // Apple
            framebuffer::fill_rect(ix + iw - 8, iy + ih - 8, 5, 5, APPLE);
        }

        // ── 10: Tetris — stacked coloured blocks in a well ────────────────
        10 => {
            const WELL: u32 = 0x060810;
            const WALL: u32 = 0x1A2A3A;
            const C1: u32 = 0x00B8D8; // cyan  (I-piece)
            const C2: u32 = 0xE8B000; // yellow (O-piece)
            const C3: u32 = 0xB000D8; // purple (T-piece)
            const C4: u32 = 0x00C840; // green  (S-piece)
            const C5: u32 = 0xE04000; // red    (Z-piece)
                                      // Well background
            framebuffer::fill_rect(ix + 4, iy, iw - 8, ih, WELL);
            // Well walls
            framebuffer::fill_rect(ix, iy, 4, ih, WALL);
            framebuffer::fill_rect(ix + iw - 4, iy, 4, ih, WALL);
            // Block size = 5x4 with 1px gap
            // Row 3 (bottom) — full row: cyan + yellow
            framebuffer::fill_rect(ix + 5, iy + ih - 5, 8, 4, C1);
            framebuffer::fill_rect(ix + 14, iy + ih - 5, 8, 4, C2);
            framebuffer::fill_rect(ix + 23, iy + ih - 5, 8, 4, C1);
            // Row 2 — partial: purple + green
            framebuffer::fill_rect(ix + 5, iy + ih - 10, 8, 4, C3);
            framebuffer::fill_rect(ix + 14, iy + ih - 10, 8, 4, C4);
            // Row 1 — sparse: red block + falling piece
            framebuffer::fill_rect(ix + 5, iy + ih - 15, 8, 4, C5);
            // Falling I-piece (top, centred)
            framebuffer::fill_rect(ix + 14, iy + 2, 8, 4, C1);
            framebuffer::fill_rect(ix + 14, iy + 7, 8, 4, C1);
        }

        // ── 100: Desktop File — document page with folded corner ────────
        100 => {
            const PAGE: u32 = 0x0E1E30;
            const FOLD: u32 = 0x060C18;
            const LINE: u32 = 0x2A5888;
            const LINE2: u32 = 0x1E3A60;
            // Page body (slightly narrower than full iw to look like paper)
            framebuffer::fill_rect(ix, iy, iw - 8, ih, PAGE);
            // Right edge (below fold)
            framebuffer::fill_rect(ix + iw - 8, iy + 8, 8, ih - 8, PAGE);
            // Folded corner — dark triangle approximation
            framebuffer::fill_rect(ix + iw - 8, iy, 8, 8, FOLD);
            framebuffer::fill_rect(ix + iw - 8, iy, 2, 8, PAGE); // edge of fold
            framebuffer::fill_rect(ix + iw - 8, iy + 6, 8, 2, PAGE); // bottom of fold
                                                                     // Text lines
            framebuffer::fill_rect(ix + 4, iy + 6, iw - 18, 2, LINE);
            framebuffer::fill_rect(ix + 4, iy + 11, iw - 18, 2, LINE);
            framebuffer::fill_rect(ix + 4, iy + 16, iw - 18, 2, LINE2);
            framebuffer::fill_rect(ix + 4, iy + 21, iw - 22, 2, LINE2);
        }

        // ── 101: Desktop Folder — classic folder with tab and shadow ──────
        101 => {
            const BODY: u32 = 0x7A4C10;
            const BODY2: u32 = 0x9A6418;
            const TAB: u32 = 0xF0B830;
            const SHAD: u32 = 0x3A2008;
            const EDGE: u32 = 0xC08020;
            // Shadow offset
            framebuffer::fill_rect(ix + 3, iy + 9, iw - 2, ih - 9, SHAD);
            // Folder tab (top-left flap)
            framebuffer::fill_rect(ix, iy + 4, 18, 5, TAB);
            // Main body
            framebuffer::fill_rect(ix, iy + 9, iw - 3, ih - 9, BODY);
            // Top highlight edge
            framebuffer::fill_rect(ix, iy + 9, iw - 3, 2, EDGE);
            // Inner lighter area (open folder depth suggestion)
            framebuffer::fill_rect(ix + 4, iy + 13, iw - 11, ih - 18, BODY2);
        }

        _ => {}
    }
}

