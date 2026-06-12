// ---------------------------------------------------------------------------
// Astra OS — Terminal window client
//
// Pure content renderer + input handler.  The desktop compositor owns the
// window chrome, cursor, and presentation.  This module only:
//   - Manages terminal state (history + input buffer)
//   - Renders its content into a given client rect (called by compositor)
//   - Handles keyboard events routed from the compositor
// ---------------------------------------------------------------------------

use crate::framebuffer;
use crate::input::Key;
use spin::Mutex;

// ── Colours ───────────────────────────────────────────────────────────────────

const BG:         u32 = 0x0A0E14;
const TEXT_NORM:  u32 = 0xB0D4B8;
const PROMPT_COL: u32 = 0x4FC3F7;
const INPUT_COL:  u32 = 0xE8F4FD;
const CURSOR_COL: u32 = 0x4FC3F7;
const ERR_COL:    u32 = 0xFF6B6B;
const SEPARATOR:  u32 = 0x1E3A5F;

// ── Font metrics (scale 2) ────────────────────────────────────────────────────

const SCALE:  usize = 2;
const CHAR_W: usize = 6 * SCALE;   // 12
const CHAR_H: usize = 8 * SCALE;   // 16 (7px glyph * 2 + 2px line gap)
const PAD_X:  usize = 10;

const PROMPT: &str = "astra$ ";

// ── Storage ───────────────────────────────────────────────────────────────────

const HIST_ROWS:  usize = 40;
const LINE_BUF:   usize = 82;
const CMD_HIST:   usize = 16;   // command history ring size
const PATH_BUF:   usize = 128;  // max cwd path length

#[derive(Clone, Copy)]
struct Line {
    data: [u8; LINE_BUF],
    len:  usize,
    col:  u32,
}

impl Line {
    const fn empty() -> Self {
        Line { data: [0u8; LINE_BUF], len: 0, col: TEXT_NORM }
    }
}

struct TermState {
    hist:       [Line; HIST_ROWS],
    hist_cnt:   usize,
    input:      [u8; LINE_BUF],
    input_len:  usize,
    cursor_pos: usize,   // byte offset within input where next char is inserted
    inited:     bool,
    // command history
    cmd_hist:   [[u8; LINE_BUF]; CMD_HIST],
    cmd_hlen:   [usize; CMD_HIST],
    cmd_hcount: usize,           // total commands entered
    cmd_hpos:   usize,           // browse position (0 = latest)
    // scroll: 0 = pinned to bottom (most recent), N = scrolled back N rows
    scroll_off: usize,
    // current working directory (FAT32 cluster + display path)
    cwd_cluster: u32,            // 0 = use FAT32 root when mounted
    cwd_path:    [u8; PATH_BUF],
    cwd_plen:    usize,
}

impl TermState {
    const fn new() -> Self {
        TermState {
            hist:       [Line::empty(); HIST_ROWS],
            hist_cnt:   0,
            input:      [0u8; LINE_BUF],
            input_len:  0,
            cursor_pos: 0,
            inited:     false,
            cmd_hist:   [[0u8; LINE_BUF]; CMD_HIST],
            cmd_hlen:   [0usize; CMD_HIST],
            cmd_hcount: 0,
            cmd_hpos:   0,
            scroll_off: 0,
            cwd_cluster: 0,
            cwd_path:   [0u8; PATH_BUF],
            cwd_plen:   0,
        }
    }

    fn push_str(&mut self, s: &str, col: u32) {
        self.push_bytes(s.as_bytes(), col);
    }

    fn push_bytes(&mut self, b: &[u8], col: u32) {
        if self.hist_cnt == HIST_ROWS {
            for i in 0..HIST_ROWS - 1 {
                self.hist[i] = self.hist[i + 1];
            }
            self.hist_cnt -= 1;
        }
        let ln = &mut self.hist[self.hist_cnt];
        let n = b.len().min(LINE_BUF);
        ln.data[..n].copy_from_slice(&b[..n]);
        ln.len = n;
        ln.col = col;
        self.hist_cnt += 1;
        // Auto-scroll to bottom when new content arrives
        self.scroll_off = 0;
    }

    /// Push a command into the ring history and reset browse position.
    fn push_cmd_hist(&mut self, cmd: &[u8], len: usize) {
        if len == 0 { return; }
        let slot = self.cmd_hcount % CMD_HIST;
        let n = len.min(LINE_BUF);
        self.cmd_hist[slot][..n].copy_from_slice(&cmd[..n]);
        self.cmd_hlen[slot] = n;
        self.cmd_hcount += 1;
        self.cmd_hpos = 0;
    }

    /// Navigate history: delta = 1 means older, -1 means newer.
    /// Returns true if input was changed.
    fn history_navigate(&mut self, delta: isize) -> bool {
        let total = self.cmd_hcount.min(CMD_HIST);
        if total == 0 { return false; }
        let new_pos = (self.cmd_hpos as isize + delta)
            .clamp(0, total as isize) as usize;
        if new_pos == self.cmd_hpos { return false; }
        self.cmd_hpos = new_pos;
        if new_pos == 0 {
            self.input_len = 0;
        } else {
            // pos=1 → most recent, pos=total → oldest
            let idx = (self.cmd_hcount.wrapping_sub(new_pos)) % CMD_HIST;
            let n = self.cmd_hlen[idx];
            self.input[..n].copy_from_slice(&self.cmd_hist[idx][..n]);
            self.input_len = n;
        }
        // always put cursor at end after history navigation
        self.cursor_pos = self.input_len;
        true
    }

    /// Return current directory display string (e.g. "/docs").
    fn cwd_str(&self) -> &str {
        if self.cwd_plen == 0 {
            "/"
        } else {
            core::str::from_utf8(&self.cwd_path[..self.cwd_plen]).unwrap_or("/")
        }
    }
}

static TERM: Mutex<TermState> = Mutex::new(TermState::new());

// ── Public interface (called by desktop compositor) ───────────────────────────

#[derive(Copy, Clone, PartialEq, Eq)]
pub enum TermAction {
    Nothing,
    RedrawAll,
    RedrawInput,
    Close,
}

/// Height of the input-line region at the bottom of client area.
/// Covers: separator (1px) + gap (3px) + prompt/input row (CHAR_H) + padding (10px).
pub const INPUT_REGION_H: usize = CHAR_H + 14;

/// One-time welcome banner.
pub fn init_if_needed() {
    let mut t = TERM.lock();
    if !t.inited {
        t.inited = true;
        t.push_str("Astra OS  Terminal  v0.1", TEXT_NORM);
        t.push_str("Type 'help' for commands.  ESC to close.", TEXT_NORM);
        t.push_str("", TEXT_NORM);
    }
}

/// Render terminal content into the given client area (backbuffer only).
pub fn render(cx: usize, cy: usize, cw: usize, ch: usize) {
    let t = TERM.lock();

    let inner_x = cx + PAD_X;
    let inner_w = cw.saturating_sub(PAD_X * 2);
    let max_cols = inner_w / CHAR_W;
    if max_cols == 0 { return; }

    // Input row at bottom of client area
    let input_y = cy + ch.saturating_sub(CHAR_H + 10);

    // History area
    let history_y = cy + 6;
    let avail_h = input_y.saturating_sub(history_y + 6);
    let vis_rows = avail_h / CHAR_H;

    // Clear history area background so stale lines don't bleed through on scroll
    framebuffer::fill_rect(cx, history_y, cw, avail_h, BG);

    // Clamp scroll so we can't scroll past the top of history
    let max_scroll = t.hist_cnt.saturating_sub(vis_rows);
    let scroll = t.scroll_off.min(max_scroll);
    let start_idx = t.hist_cnt.saturating_sub(vis_rows + scroll);
    let end_idx = t.hist_cnt.saturating_sub(scroll);

    // Show scroll indicator when not at the bottom
    if scroll > 0 {
        framebuffer::draw_text_scaled(inner_x, history_y, "^ PgUp/PgDn to scroll ^", 0x444466, SCALE);
    }

    for (row, idx) in (start_idx..end_idx).enumerate() {
        let ln = &t.hist[idx];
        if ln.len > 0 {
            let n = ln.len.min(max_cols);
            let s = unsafe { core::str::from_utf8_unchecked(&ln.data[..n]) };
            framebuffer::draw_text_scaled(inner_x, history_y + row * CHAR_H, s, ln.col, SCALE);
        }
    }

    // Separator line
    framebuffer::fill_rect(cx, input_y - 4, cw, 1, SEPARATOR);

    // Prompt
    let prompt_cols = PROMPT.len().min(max_cols);
    framebuffer::draw_text_scaled(inner_x, input_y, &PROMPT[..prompt_cols], PROMPT_COL, SCALE);

    // User input — scroll window keeps cursor visible
    let text_x = inner_x + prompt_cols * CHAR_W;
    let input_cols = max_cols.saturating_sub(prompt_cols);
    if t.input_len > 0 && input_cols > 0 {
        let scroll_start = if t.cursor_pos >= input_cols { t.cursor_pos + 1 - input_cols } else { 0 };
        let shown_end = (scroll_start + input_cols).min(t.input_len);
        if shown_end > scroll_start {
            let s = unsafe { core::str::from_utf8_unchecked(&t.input[scroll_start..shown_end]) };
            framebuffer::draw_text_scaled(text_x, input_y, s, INPUT_COL, SCALE);
        }
    }

    // Block cursor at cursor_pos
    let input_cols_c = max_cols.saturating_sub(prompt_cols);
    let scroll_c = if t.cursor_pos >= input_cols_c { t.cursor_pos + 1 - input_cols_c } else { 0 };
    let cur_x = text_x + (t.cursor_pos - scroll_c) * CHAR_W;
    framebuffer::fill_rect(cur_x, input_y, CHAR_W - 2, CHAR_H, CURSOR_COL);
}

/// Render only the input-line region (separator + prompt + input + cursor).
/// The compositor should clear the input sub-rect before calling this.
pub fn render_input_line(cx: usize, cy: usize, cw: usize, ch: usize) {
    let t = TERM.lock();
    let inner_x = cx + PAD_X;
    let inner_w = cw.saturating_sub(PAD_X * 2);
    let max_cols = inner_w / CHAR_W;
    if max_cols == 0 { return; }

    let input_y = cy + ch.saturating_sub(CHAR_H + 10);

    // Separator line
    framebuffer::fill_rect(cx, input_y - 4, cw, 1, SEPARATOR);

    // Prompt
    let prompt_cols = PROMPT.len().min(max_cols);
    framebuffer::draw_text_scaled(inner_x, input_y, &PROMPT[..prompt_cols], PROMPT_COL, SCALE);

    // User input — scroll window keeps cursor visible
    let text_x = inner_x + prompt_cols * CHAR_W;
    let input_cols = max_cols.saturating_sub(prompt_cols);
    if t.input_len > 0 && input_cols > 0 {
        let scroll_start = if t.cursor_pos >= input_cols { t.cursor_pos + 1 - input_cols } else { 0 };
        let shown_end = (scroll_start + input_cols).min(t.input_len);
        if shown_end > scroll_start {
            let s = unsafe { core::str::from_utf8_unchecked(&t.input[scroll_start..shown_end]) };
            framebuffer::draw_text_scaled(text_x, input_y, s, INPUT_COL, SCALE);
        }
    }

    // Block cursor at cursor_pos
    let input_cols_c = max_cols.saturating_sub(prompt_cols);
    let scroll_c = if t.cursor_pos >= input_cols_c { t.cursor_pos + 1 - input_cols_c } else { 0 };
    let cur_x = text_x + (t.cursor_pos - scroll_c) * CHAR_W;
    framebuffer::fill_rect(cur_x, input_y, CHAR_W - 2, CHAR_H, CURSOR_COL);
}

/// Handle a keyboard event. Returns what the compositor should do.
pub fn handle_key(key: Key) -> TermAction {
    match key {
        Key::Escape => TermAction::Nothing,

        Key::Backspace => {
            let mut t = TERM.lock();
            if t.cursor_pos > 0 {
                let pos = t.cursor_pos - 1;
                let len = t.input_len;
                t.input.copy_within(pos + 1..len, pos);
                let new_len = len - 1;
                t.input[new_len] = 0;
                t.input_len = new_len;
                t.cursor_pos = pos;
                TermAction::RedrawInput
            } else {
                TermAction::Nothing
            }
        }

        Key::Delete => {
            let mut t = TERM.lock();
            let pos = t.cursor_pos;
            let len = t.input_len;
            if pos < len {
                t.input.copy_within(pos + 1..len, pos);
                let new_len = len - 1;
                t.input[new_len] = 0;
                t.input_len = new_len;
                TermAction::RedrawInput
            } else {
                TermAction::Nothing
            }
        }

        Key::ArrowLeft => {
            let mut t = TERM.lock();
            if t.cursor_pos > 0 {
                t.cursor_pos -= 1;
                TermAction::RedrawInput
            } else {
                TermAction::Nothing
            }
        }

        Key::ArrowRight => {
            let mut t = TERM.lock();
            if t.cursor_pos < t.input_len {
                t.cursor_pos += 1;
                TermAction::RedrawInput
            } else {
                TermAction::Nothing
            }
        }

        Key::Home => {
            let mut t = TERM.lock();
            if t.cursor_pos != 0 {
                t.cursor_pos = 0;
                TermAction::RedrawInput
            } else {
                TermAction::Nothing
            }
        }

        Key::End => {
            let mut t = TERM.lock();
            if t.cursor_pos != t.input_len {
                t.cursor_pos = t.input_len;
                TermAction::RedrawInput
            } else {
                TermAction::Nothing
            }
        }

        Key::ArrowUp => {
            let changed = TERM.lock().history_navigate(1);
            if changed { TermAction::RedrawInput } else { TermAction::Nothing }
        }

        Key::ArrowDown => {
            let changed = TERM.lock().history_navigate(-1);
            if changed { TermAction::RedrawInput } else { TermAction::Nothing }
        }

        Key::PageUp => {
            let mut t = TERM.lock();
            t.scroll_off = t.scroll_off.saturating_add(8);
            TermAction::RedrawAll
        }

        Key::PageDown => {
            let mut t = TERM.lock();
            t.scroll_off = t.scroll_off.saturating_sub(8);
            TermAction::RedrawAll
        }

        Key::Enter => {
            execute_input();
            TermAction::RedrawAll
        }

        Key::Char(c) => {
            let mut t = TERM.lock();
            if t.input_len < LINE_BUF - 1 {
                let pos = t.cursor_pos;
                let len = t.input_len;
                t.input.copy_within(pos..len, pos + 1);
                t.input[pos] = c;
                t.input_len += 1;
                t.cursor_pos += 1;
                t.cmd_hpos = 0;
                TermAction::RedrawInput
            } else {
                TermAction::Nothing
            }
        }

        _ => TermAction::Nothing,
    }
}

// ── Internal: command execution ───────────────────────────────────────────────

fn execute_input() {
    let (cmd_data, cmd_len) = {
        let t = TERM.lock();
        let mut d = [0u8; LINE_BUF];
        let l = t.input_len;
        d[..l].copy_from_slice(&t.input[..l]);
        (d, l)
    };

    // Echo prompt + command to history, push to cmd history, clear input
    {
        let mut t = TERM.lock();
        let mut echo = [0u8; LINE_BUF];
        let pb = PROMPT.as_bytes();
        let pn = pb.len().min(LINE_BUF);
        echo[..pn].copy_from_slice(&pb[..pn]);
        let cn = cmd_len.min(LINE_BUF - pn);
        echo[pn..pn + cn].copy_from_slice(&cmd_data[..cn]);
        t.push_bytes(&echo[..pn + cn], PROMPT_COL);
        t.push_cmd_hist(&cmd_data, cmd_len);
        t.input_len = 0;
        t.cursor_pos = 0;
        t.cmd_hpos = 0;
    }

    if cmd_len == 0 { return; }

    let raw = unsafe { core::str::from_utf8_unchecked(&cmd_data[..cmd_len]) };
    let raw = raw.trim_end();
    if raw.is_empty() { return; }

    let (cmd, args) = match raw.find(' ') {
        Some(pos) => (&raw[..pos], raw[pos + 1..].trim_start()),
        None => (raw, ""),
    };

    run_cmd(cmd, args);
}

fn run_cmd(cmd: &str, args: &str) {
    match cmd {
        "help" => {
            let mut t = TERM.lock();
            t.push_str("Commands:", TEXT_NORM);
            t.push_str("  help              - this list", TEXT_NORM);
            t.push_str("  clear             - clear screen", TEXT_NORM);
            t.push_str("  version           - OS version", TEXT_NORM);
            t.push_str("  uptime            - time since boot", TEXT_NORM);
            t.push_str("  mem               - heap memory usage", TEXT_NORM);
            t.push_str("  ls [path]         - list directory", TEXT_NORM);
            t.push_str("  cd <dir>          - change directory", TEXT_NORM);
            t.push_str("  cat <file>        - print file contents", TEXT_NORM);
            t.push_str("  touch <name>      - create empty file", TEXT_NORM);
            t.push_str("  mkdir <name>      - create directory", TEXT_NORM);
            t.push_str("  rm <name>         - delete file or folder", TEXT_NORM);
            t.push_str("  rename <old> <new>- rename entry", TEXT_NORM);
            t.push_str("  cp <src> <dst>    - copy file", TEXT_NORM);
            t.push_str("  mv <src> <dst>    - move/rename file", TEXT_NORM);
            t.push_str("  net               - network status", TEXT_NORM);
            t.push_str("  ping <ip>         - send ICMP echo to <ip>", TEXT_NORM);
            t.push_str("  dns <host>        - resolve hostname via DNS", TEXT_NORM);
            t.push_str("  http <url>        - HTTP GET (e.g. http http://example.com/)", TEXT_NORM);
            t.push_str("  netcheck [n]      - run ping/dns/http checks n times (default 3)", TEXT_NORM);
            t.push_str("  exec <prog>        - run user program (hello/gui)", TEXT_NORM);
            t.push_str("  ps                 - list processes", TEXT_NORM);
            t.push_str("  kill <pid>         - terminate process", TEXT_NORM);
            t.push_str("  memprobe          - kernel/user isolation diagnostic", TEXT_NORM);
            t.push_str("  memtest           - pointer-validation regression battery", TEXT_NORM);
            t.push_str("  cpuinfo           - CPU vendor/brand, APIC, topology", TEXT_NORM);
            t.push_str("  apictest          - switch tick source PIT->LAPIC->PIT", TEXT_NORM);
            t.push_str("  echo <text>        - print text", TEXT_NORM);
            t.push_str("  Up/Down arrows    - command history", TEXT_NORM);
        }

        "clear" => {
            TERM.lock().hist_cnt = 0;
        }

        "version" => {
            let mut t = TERM.lock();
            t.push_str("Astra OS  v0.1", TEXT_NORM);
            t.push_str("Kernel: Rust no_std / UEFI / x86_64", TEXT_NORM);
            t.push_str("Build:  April 2026", TEXT_NORM);
        }

        "uptime" => {
            let ms = crate::arch::x86_64::interrupts::uptime_ms();
            let secs = ms / 1000;
            let millis = ms % 1000;
            let mut buf = [0u8; 48];
            let mut pos = 0;
            let pfx = b"Uptime: ";
            buf[..pfx.len()].copy_from_slice(pfx);
            pos += pfx.len();
            pos += write_dec(&mut buf[pos..], secs);
            buf[pos] = b's'; pos += 1;
            buf[pos] = b'.'; pos += 1;
            pos += write_dec(&mut buf[pos..], millis);
            buf[pos] = b'm'; pos += 1;
            buf[pos] = b's'; pos += 1;
            let s = unsafe { core::str::from_utf8_unchecked(&buf[..pos]) };
            TERM.lock().push_str(s, TEXT_NORM);
        }

        "mem" => {
            let heap = crate::memory::heap::get_telemetry();
            let used_kb  = heap.used_bytes / 1024;
            let total_kb = (heap.mapped_pages * 4096) / 1024;
            let free_kb  = total_kb.saturating_sub(used_kb);
            let mut t = TERM.lock();
            // "Used:  1234 KB / 8192 KB  (15%)"
            let mut buf = [0u8; LINE_BUF];
            let mut pos = 0;
            let pfx = b"Heap used:  ";
            buf[..pfx.len()].copy_from_slice(pfx);
            pos += pfx.len();
            pos += write_dec(&mut buf[pos..], used_kb as u64);
            let mid = b" KB / ";
            buf[pos..pos + mid.len()].copy_from_slice(mid);
            pos += mid.len();
            pos += write_dec(&mut buf[pos..], total_kb as u64);
            let sfx = b" KB";
            buf[pos..pos + sfx.len()].copy_from_slice(sfx);
            pos += sfx.len();
            let s = unsafe { core::str::from_utf8_unchecked(&buf[..pos]) };
            t.push_str(s, TEXT_NORM);

            let mut buf2 = [0u8; LINE_BUF];
            let mut p2 = 0;
            let pfx2 = b"Heap free:  ";
            buf2[..pfx2.len()].copy_from_slice(pfx2);
            p2 += pfx2.len();
            p2 += write_dec(&mut buf2[p2..], free_kb as u64);
            let sfx2 = b" KB";
            buf2[p2..p2 + sfx2.len()].copy_from_slice(sfx2);
            p2 += sfx2.len();
            let s2 = unsafe { core::str::from_utf8_unchecked(&buf2[..p2]) };
            t.push_str(s2, TEXT_NORM);
        }

        "ls" => {
            cmd_ls(args);
        }

        "cd" => {
            cmd_cd(args);
        }

        "cat" => {
            cmd_cat(args);
        }

        "touch" => {
            cmd_touch(args);
        }

        "mkdir" => {
            cmd_mkdir(args);
        }

        "rm" => {
            cmd_rm(args);
        }

        "rename" => {
            cmd_rename(args);
        }

        "cp" => {
            cmd_cp(args);
        }

        "mv" => {
            cmd_mv(args);
        }

        "net" => {
            cmd_net();
        }

        "ping" => {
            cmd_ping(args);
        }

        "dns" => {
            cmd_dns(args);
        }

        "http" => {
            cmd_http(args);
        }

        "netcheck" => {
            cmd_netcheck(args);
        }

        "exec" => {
            cmd_exec(args);
        }

        "ps" => {
            cmd_ps();
        }

        "kill" => {
            cmd_kill(args);
        }

        "memprobe" => {
            cmd_memprobe();
        }

        "memtest" => {
            cmd_memtest();
        }

        "cpuinfo" => {
            cmd_cpuinfo();
        }

        "apictest" => {
            cmd_apictest();
        }

        "echo" => {
            let text = if args.is_empty() { "" } else { args };
            TERM.lock().push_str(text, TEXT_NORM);
        }

        other => {
            let mut t = TERM.lock();
            let mut buf = [0u8; LINE_BUF];
            let pfx = b"Unknown command: ";
            let pn = pfx.len().min(LINE_BUF);
            buf[..pn].copy_from_slice(&pfx[..pn]);
            let ob = other.as_bytes();
            let on = ob.len().min(LINE_BUF - pn);
            buf[pn..pn + on].copy_from_slice(&ob[..on]);
            t.push_bytes(&buf[..pn + on], ERR_COL);
        }
    }
}

// ── Command implementations ───────────────────────────────────────────────────

fn cmd_ls(args: &str) {
    // Resolve which cluster to list
    let cluster = if args.is_empty() {
        cwd_cluster()
    } else {
        resolve_cluster_for_path(args)
    };

    match cluster {
        None => {
            TERM.lock().push_str("ls: path not found", ERR_COL);
        }
        Some(clus) => {
            let mut count = 0usize;
            crate::fat32::list_dir(clus, |de| {
                // skip . and .. in listing
                if de.name_len == 1 && de.name[0] == b'.' { return true; }
                if de.name_len == 2 && de.name[0] == b'.' && de.name[1] == b'.' { return true; }
                // Build display line: "  NAME  <DIR>" or "  NAME  1234 B"
                let mut buf = [0u8; LINE_BUF];
                let mut pos = 0;
                buf[pos] = b' '; pos += 1;
                buf[pos] = b' '; pos += 1;
                let nn = de.name_len.min(12);
                buf[pos..pos + nn].copy_from_slice(&de.name[..nn]);
                pos += nn;
                // pad to column 14
                while pos < 16 { buf[pos] = b' '; pos += 1; }
                if de.is_dir {
                    let d = b"<DIR>";
                    buf[pos..pos + d.len()].copy_from_slice(d);
                    pos += d.len();
                } else {
                    pos += write_dec(&mut buf[pos..], de.size as u64);
                    let b_ = b" B";
                    buf[pos..pos + b_.len()].copy_from_slice(b_);
                    pos += b_.len();
                }
                let s = unsafe { core::str::from_utf8_unchecked(&buf[..pos]) };
                TERM.lock().push_str(s, if de.is_dir { 0x4FC3F7 } else { TEXT_NORM });
                count += 1;
                true
            });
            if count == 0 {
                TERM.lock().push_str("  (empty)", TEXT_NORM);
            }
        }
    }
}

fn cmd_cd(args: &str) {
    if args.is_empty() {
        // cd with no args → go to root
        let mut t = TERM.lock();
        t.cwd_cluster = 0;
        t.cwd_plen = 0;
        return;
    }
    if args == ".." {
        // go up — we only have the current path, so strip last segment
        let mut t = TERM.lock();
        if t.cwd_plen == 0 {
            return; // already at root
        }
        // find last '/' in path
        let path = &t.cwd_path[..t.cwd_plen];
        let slash = path.iter().rposition(|&b| b == b'/');
        match slash {
            None | Some(0) => {
                t.cwd_cluster = 0;
                t.cwd_plen = 0;
            }
            Some(pos) => {
                t.cwd_plen = pos;
                // re-resolve cluster by walking path from root
                let new_plen = pos;
                let new_path_bytes = {
                    let mut tmp = [0u8; PATH_BUF];
                    tmp[..new_plen].copy_from_slice(&t.cwd_path[..new_plen]);
                    tmp
                };
                drop(t);
                let path_str = unsafe { core::str::from_utf8_unchecked(&new_path_bytes[..new_plen]) };
                let new_clus = walk_path_to_cluster(path_str).unwrap_or(0);
                let mut t2 = TERM.lock();
                t2.cwd_cluster = new_clus;
                t2.cwd_plen = new_plen;
            }
        }
        return;
    }

    // Navigate into a named subdirectory
    let parent_clus = cwd_cluster().unwrap_or_else(|| {
        if crate::fat32::is_mounted() { crate::fat32::root_cluster() } else { 0 }
    });
    let nb = args.as_bytes();
    match crate::fat32::find_in_dir(parent_clus, nb) {
        None => {
            TERM.lock().push_str("cd: not found", ERR_COL);
        }
        Some(de) if !de.is_dir => {
            TERM.lock().push_str("cd: not a directory", ERR_COL);
        }
        Some(de) => {
            let mut t = TERM.lock();
            // append /name to cwd_path
            let nn = de.name_len.min(12);
            let p = t.cwd_plen;
            if p + 1 + nn <= PATH_BUF {
                t.cwd_path[p] = b'/';
                let p1 = p + 1;
                t.cwd_path[p1..p1 + nn].copy_from_slice(&de.name[..nn]);
                t.cwd_plen = p1 + nn;
            }
            t.cwd_cluster = de.cluster;
        }
    }
}

fn cmd_cat(args: &str) {
    if args.is_empty() {
        TERM.lock().push_str("usage: cat <filename>", ERR_COL);
        return;
    }
    let parent_clus = cwd_cluster().unwrap_or_else(|| {
        if crate::fat32::is_mounted() { crate::fat32::root_cluster() } else { 0 }
    });
    let nb = args.as_bytes();
    match crate::fat32::find_in_dir(parent_clus, nb) {
        None => {
            TERM.lock().push_str("cat: file not found", ERR_COL);
        }
        Some(de) if de.is_dir => {
            TERM.lock().push_str("cat: is a directory", ERR_COL);
        }
        Some(de) => {
            // Read up to 4 KB and display line by line
            const READ_MAX: usize = 4096;
            let mut buf = [0u8; READ_MAX];
            let n = crate::fat32::read_file(de.cluster, de.size, &mut buf);
            if n == 0 {
                TERM.lock().push_str("(empty file)", TEXT_NORM);
                return;
            }
            let mut start = 0usize;
            let mut t = TERM.lock();
            for i in 0..n {
                if buf[i] == b'\n' || i == n - 1 {
                    let end = if buf[i] == b'\n' { i } else { i + 1 };
                    let line = &buf[start..end];
                    if !line.is_empty() {
                        // split into LINE_BUF-sized chunks if needed
                        let mut off = 0;
                        while off < line.len() {
                            let chunk = &line[off..(off + (LINE_BUF - 1)).min(line.len())];
                            t.push_bytes(chunk, TEXT_NORM);
                            off += LINE_BUF - 1;
                        }
                    } else {
                        t.push_str("", TEXT_NORM);
                    }
                    start = i + 1;
                }
            }
            if n >= READ_MAX {
                t.push_str("... (truncated at 4 KB)", TEXT_NORM);
            }
        }
    }
}

fn cmd_touch(args: &str) {
    if args.is_empty() {
        TERM.lock().push_str("usage: touch <filename>", ERR_COL);
        return;
    }
    if !crate::fat32::is_mounted() {
        TERM.lock().push_str("touch: no FAT32 disk mounted", ERR_COL);
        return;
    }
    let parent_clus = cwd_cluster().unwrap_or_else(|| crate::fat32::root_cluster());
    let ok = crate::fat32::write_file(parent_clus, args.as_bytes(), &[]);
    let mut t = TERM.lock();
    if ok {
        t.push_str("created", TEXT_NORM);
    } else {
        t.push_str("touch: failed", ERR_COL);
    }
}

fn cmd_mkdir(args: &str) {
    if args.is_empty() {
        TERM.lock().push_str("usage: mkdir <dirname>", ERR_COL);
        return;
    }
    if !crate::fat32::is_mounted() {
        TERM.lock().push_str("mkdir: no FAT32 disk mounted", ERR_COL);
        return;
    }
    let parent_clus = cwd_cluster().unwrap_or_else(|| crate::fat32::root_cluster());
    let ok = crate::fat32::create_dir(parent_clus, args.as_bytes());
    let mut t = TERM.lock();
    if ok {
        t.push_str("created", TEXT_NORM);
    } else {
        t.push_str("mkdir: failed", ERR_COL);
    }
}

fn cmd_rm(args: &str) {
    if args.is_empty() {
        TERM.lock().push_str("usage: rm <name>", ERR_COL);
        return;
    }
    if !crate::fat32::is_mounted() {
        TERM.lock().push_str("rm: no FAT32 disk mounted", ERR_COL);
        return;
    }
    let parent_clus = cwd_cluster().unwrap_or_else(|| crate::fat32::root_cluster());
    let ok = crate::fat32::delete_entry(parent_clus, args.as_bytes());
    let mut t = TERM.lock();
    if ok {
        t.push_str("deleted", TEXT_NORM);
    } else {
        t.push_str("rm: not found or failed", ERR_COL);
    }
}

fn cmd_rename(args: &str) {
    // args: "oldname newname"
    let (old, new) = match args.find(' ') {
        Some(pos) => (&args[..pos], args[pos + 1..].trim_start()),
        None => {
            TERM.lock().push_str("usage: rename <old> <new>", ERR_COL);
            return;
        }
    };
    if new.is_empty() {
        TERM.lock().push_str("usage: rename <old> <new>", ERR_COL);
        return;
    }
    if !crate::fat32::is_mounted() {
        TERM.lock().push_str("rename: no FAT32 disk mounted", ERR_COL);
        return;
    }
    let parent_clus = cwd_cluster().unwrap_or_else(|| crate::fat32::root_cluster());
    let ok = crate::fat32::rename_entry(parent_clus, old.as_bytes(), new.as_bytes());
    let mut t = TERM.lock();
    if ok {
        t.push_str("renamed", TEXT_NORM);
    } else {
        t.push_str("rename: failed", ERR_COL);
    }
}

fn cmd_cp(args: &str) {
    let (src, dst) = match args.find(' ') {
        Some(pos) => (&args[..pos], args[pos + 1..].trim_start()),
        None => {
            TERM.lock().push_str("usage: cp <src> <dst>", ERR_COL);
            return;
        }
    };
    if dst.is_empty() {
        TERM.lock().push_str("usage: cp <src> <dst>", ERR_COL);
        return;
    }
    if !crate::fat32::is_mounted() {
        TERM.lock().push_str("cp: no FAT32 disk mounted", ERR_COL);
        return;
    }
    let dir_c = cwd_cluster().unwrap_or_else(|| crate::fat32::root_cluster());
    let de = match crate::fat32::find_in_dir(dir_c, src.as_bytes()) {
        Some(d) => d,
        None => { TERM.lock().push_str("cp: source not found", ERR_COL); return; }
    };
    if de.is_dir {
        TERM.lock().push_str("cp: directories not supported", ERR_COL);
        return;
    }
    const MAX: usize = 65536;
    let mut buf = [0u8; MAX];
    let n = crate::fat32::read_file(de.cluster, de.size, &mut buf);
    if !crate::fat32::write_file(dir_c, dst.as_bytes(), &buf[..n]) {
        TERM.lock().push_str("cp: write failed", ERR_COL);
        return;
    }
    TERM.lock().push_str("copied", TEXT_NORM);
}

fn cmd_mv(args: &str) {
    let (src, dst) = match args.find(' ') {
        Some(pos) => (&args[..pos], args[pos + 1..].trim_start()),
        None => {
            TERM.lock().push_str("usage: mv <src> <dst>", ERR_COL);
            return;
        }
    };
    if dst.is_empty() {
        TERM.lock().push_str("usage: mv <src> <dst>", ERR_COL);
        return;
    }
    if !crate::fat32::is_mounted() {
        TERM.lock().push_str("mv: no FAT32 disk mounted", ERR_COL);
        return;
    }
    let dir_c = cwd_cluster().unwrap_or_else(|| crate::fat32::root_cluster());
    let ok = crate::fat32::rename_entry(dir_c, src.as_bytes(), dst.as_bytes());
    let mut t = TERM.lock();
    if ok { t.push_str("moved", TEXT_NORM); }
    else  { t.push_str("mv: failed", ERR_COL); }
}

fn cmd_net() {
    let (ready, link, tx, rx) = crate::net::driver::stats();
    let mut t = TERM.lock();
    if !ready {
        t.push_str("NIC: not present (no virtio-net device)", ERR_COL);
        return;
    }
    if link {
        t.push_str("NIC: virtio-net  link UP", 0x66FF66);
    } else {
        t.push_str("NIC: virtio-net  link DOWN", ERR_COL);
    }
    let mac = crate::net::driver::mac_addr();
    let mut mac_buf = [0u8; 24];
    let mut pos = 0usize;
    let pfx = b"MAC: ";
    mac_buf[..pfx.len()].copy_from_slice(pfx);
    pos += pfx.len();
    const HEX: &[u8] = b"0123456789abcdef";
    for i in 0..6 {
        if i > 0 { mac_buf[pos] = b':'; pos += 1; }
        mac_buf[pos] = HEX[(mac[i] >> 4) as usize]; pos += 1;
        mac_buf[pos] = HEX[(mac[i] & 0xF) as usize]; pos += 1;
    }
    let s = unsafe { core::str::from_utf8_unchecked(&mac_buf[..pos]) };
    t.push_str(s, TEXT_NORM);
    let mut buf = [0u8; LINE_BUF];
    let mut p = 0usize;
    let pfx2 = b"TX frames: ";
    buf[..pfx2.len()].copy_from_slice(pfx2);
    p += pfx2.len();
    p += write_dec(&mut buf[p..], tx);
    let s2 = unsafe { core::str::from_utf8_unchecked(&buf[..p]) };
    t.push_str(s2, TEXT_NORM);
    let mut buf3 = [0u8; LINE_BUF];
    let mut p3 = 0usize;
    let pfx3 = b"RX frames: ";
    buf3[..pfx3.len()].copy_from_slice(pfx3);
    p3 += pfx3.len();
    p3 += write_dec(&mut buf3[p3..], rx);
    let s3 = unsafe { core::str::from_utf8_unchecked(&buf3[..p3]) };
    t.push_str(s3, TEXT_NORM);

    // Show IP config if available
    if let Some(cfg) = crate::net::config::get() {
        let ip = cfg.ip;
        let gw = cfg.gateway;
        let mut ibuf = [0u8; LINE_BUF];
        let mut ip = ip;
        let pfx_ip = b"IP:  ";
        let mut pp = pfx_ip.len().min(LINE_BUF);
        ibuf[..pp].copy_from_slice(&pfx_ip[..pp]);
        let ip_arr = [cfg.ip[0], cfg.ip[1], cfg.ip[2], cfg.ip[3]];
        pp += write_ipv4(&mut ibuf[pp..], ip_arr);
        let _ = ip; // suppress unused
        let s4 = unsafe { core::str::from_utf8_unchecked(&ibuf[..pp]) };
        t.push_str(s4, TEXT_NORM);
        let mut gbuf = [0u8; LINE_BUF];
        let pfx_gw = b"GW:  ";
        let mut gp = pfx_gw.len().min(LINE_BUF);
        gbuf[..gp].copy_from_slice(&pfx_gw[..gp]);
        gp += write_ipv4(&mut gbuf[gp..], gw);
        let s5 = unsafe { core::str::from_utf8_unchecked(&gbuf[..gp]) };
        t.push_str(s5, TEXT_NORM);

        // Show RX and TX queue debug state
        let (rx_last, rx_hw) = crate::net::driver::debug_rx_state();
        let (tx_last, tx_hw) = crate::net::driver::debug_tx_state();
        let mut dbuf = [0u8; LINE_BUF];
        let mut dp = 0usize;
        let dpfx = b"RX q: sw=";
        let dl = dpfx.len().min(LINE_BUF);
        dbuf[..dl].copy_from_slice(&dpfx[..dl]); dp += dl;
        dp += write_dec(&mut dbuf[dp..], rx_last as u64);
        if dp + 4 < LINE_BUF { dbuf[dp..dp+4].copy_from_slice(b" hw="); dp += 4; }
        dp += write_dec(&mut dbuf[dp..], rx_hw as u64);
        t.push_str(unsafe { core::str::from_utf8_unchecked(&dbuf[..dp]) },
                   if rx_hw != rx_last { 0x66FF66 } else { TEXT_NORM });
        let mut dbuf2 = [0u8; LINE_BUF];
        let mut dp2 = 0usize;
        let dpfx2 = b"TX q: sw=";
        let dl2 = dpfx2.len().min(LINE_BUF);
        dbuf2[..dl2].copy_from_slice(&dpfx2[..dl2]); dp2 += dl2;
        dp2 += write_dec(&mut dbuf2[dp2..], tx_last as u64);
        if dp2 + 4 < LINE_BUF { dbuf2[dp2..dp2+4].copy_from_slice(b" hw="); dp2 += 4; }
        dp2 += write_dec(&mut dbuf2[dp2..], tx_hw as u64);
        t.push_str(unsafe { core::str::from_utf8_unchecked(&dbuf2[..dp2]) },
                   if tx_hw != tx_last { 0x66FF66 } else { 0xFFAA44 });
    } else {
        t.push_str("IP: not configured", ERR_COL);
    }
}

/// Parse an IPv4 dotted-decimal string into `[u8; 4]`.  Returns None on failure.
fn parse_ip(s: &str) -> Option<[u8; 4]> {
    let mut octets = [0u8; 4];
    let mut idx = 0usize;
    let mut cur: u16 = 0;
    let mut digits = 0usize;
    for b in s.bytes() {
        match b {
            b'0'..=b'9' => {
                cur = cur * 10 + (b - b'0') as u16;
                if cur > 255 { return None; }
                digits += 1;
            }
            b'.' => {
                if digits == 0 || idx >= 3 { return None; }
                octets[idx] = cur as u8;
                idx += 1;
                cur = 0;
                digits = 0;
            }
            _ => return None,
        }
    }
    if idx != 3 || digits == 0 { return None; }
    octets[3] = cur as u8;
    Some(octets)
}

/// Write `ip` as dotted-decimal into `buf`.  Returns bytes written.
fn write_ipv4(buf: &mut [u8], ip: [u8; 4]) -> usize {
    let mut pos = 0usize;
    for (i, &octet) in ip.iter().enumerate() {
        if i > 0 {
            if pos < buf.len() { buf[pos] = b'.'; pos += 1; }
        }
        pos += write_dec(&mut buf[pos..], octet as u64);
    }
    pos
}

/// `ping <ip>` — send 4 ICMP echo requests and report RTT.
fn cmd_ping(args: &str) {
    let target = args.trim();
    if target.is_empty() {
        TERM.lock().push_str("Usage: ping <ip>  e.g. ping 10.0.2.2", ERR_COL);
        return;
    }
    let dst = match parse_ip(target) {
        Some(ip) => ip,
        None => {
            TERM.lock().push_str("ping: invalid IP address", ERR_COL);
            return;
        }
    };

    if !crate::net::driver::is_ready() {
        TERM.lock().push_str("ping: NIC not ready", ERR_COL);
        return;
    }
    if crate::net::config::get().is_none() {
        TERM.lock().push_str("ping: IP not configured", ERR_COL);
        return;
    }

    {
        let mut t = TERM.lock();
        let mut buf = [0u8; LINE_BUF];
        let pfx = b"PING ";
        let mut p = pfx.len();
        buf[..p].copy_from_slice(pfx);
        p += write_ipv4(&mut buf[p..], dst);
        let sfx = b": 32 bytes data";
        let sl = sfx.len().min(LINE_BUF - p);
        buf[p..p + sl].copy_from_slice(&sfx[..sl]);
        p += sl;
        t.push_str(unsafe { core::str::from_utf8_unchecked(&buf[..p]) }, TEXT_NORM);
    }

    // ── Phase 1: ARP resolution ────────────────────────────────────────────
    // Always resolve the target MAC before sending ICMP so we use a unicast
    // destination instead of falling back to broadcast (slirp drops broadcast ICMP).
    let dst_mac = resolve_arp(dst);
    let dst_mac = match dst_mac {
        Some(m) => {
            let mut buf = [0u8; LINE_BUF];
            let pfx = b"ARP  ";
            let mut p = pfx.len();
            buf[..p].copy_from_slice(pfx);
            p += write_ipv4(&mut buf[p..], dst);
            let sfx = b" -> ";
            let sl = sfx.len().min(LINE_BUF - p);
            buf[p..p + sl].copy_from_slice(&sfx[..sl]);
            p += sl;
            p += fmt_mac(&mut buf[p..], m);
            TERM.lock().push_str(unsafe { core::str::from_utf8_unchecked(&buf[..p]) }, 0x88CCFF);
            m
        }
        None => {
            let mut buf = [0u8; LINE_BUF];
            let pfx = b"ARP timeout for ";
            let mut p = pfx.len();
            buf[..p].copy_from_slice(pfx);
            p += write_ipv4(&mut buf[p..], dst);
            TERM.lock().push_str(unsafe { core::str::from_utf8_unchecked(&buf[..p]) }, ERR_COL);
            TERM.lock().push_str("ping: host unreachable (no ARP reply — check NIC RX)", ERR_COL);
            return;
        }
    };

    // ── Phase 2: ICMP echo loop ────────────────────────────────────────────
    const COUNT: u16 = 4;
    const ID: u16 = 0xA57A;
    const WAIT_MS: u64 = 1500;

    for seq in 0..COUNT {
        crate::net::icmp::send_echo_request_to(dst, dst_mac, ID, seq);

        let deadline = crate::arch::x86_64::interrupts::uptime_ms() + WAIT_MS;
        let mut got_reply = false;
        while crate::arch::x86_64::interrupts::uptime_ms() < deadline {
            crate::net::poll_and_dispatch();
            if let Some(reply) = crate::net::icmp::poll_reply() {
                if reply.id == ID && reply.seq == seq {
                    let mut buf = [0u8; LINE_BUF];
                    let pfx = b"Reply from ";
                    let mut p = pfx.len();
                    buf[..p].copy_from_slice(pfx);
                    p += write_ipv4(&mut buf[p..], reply.from);
                    let sfx = b": seq=";
                    let sl = sfx.len().min(LINE_BUF - p);
                    buf[p..p + sl].copy_from_slice(&sfx[..sl]);
                    p += sl;
                    p += write_dec(&mut buf[p..], seq as u64);
                    let sfx2 = b" time=";
                    let sl2 = sfx2.len().min(LINE_BUF - p);
                    buf[p..p + sl2].copy_from_slice(&sfx2[..sl2]);
                    p += sl2;
                    p += write_dec(&mut buf[p..], reply.rtt_ms as u64);
                    if p < LINE_BUF { buf[p] = b'm'; p += 1; }
                    if p < LINE_BUF { buf[p] = b's'; p += 1; }
                    TERM.lock().push_str(
                        unsafe { core::str::from_utf8_unchecked(&buf[..p]) },
                        0x66FF66,
                    );
                    got_reply = true;
                    break;
                }
            }
            crate::arch::x86_64::halt::idle_once();
        }

        if !got_reply {
            let mut buf = [0u8; LINE_BUF];
            let pfx = b"Request timeout  seq=";
            let mut p = pfx.len();
            buf[..p].copy_from_slice(pfx);
            p += write_dec(&mut buf[p..], seq as u64);
            TERM.lock().push_str(
                unsafe { core::str::from_utf8_unchecked(&buf[..p]) },
                ERR_COL,
            );
        }
    }
}

/// Send ARP requests until the target MAC is in the cache, or 1000ms elapses.
fn resolve_arp(ip: [u8; 4]) -> Option<[u8; 6]> {
    crate::net::arp::resolve_with_retry(ip, 1050, 3)
}

/// Format a MAC address into buf as "xx:xx:xx:xx:xx:xx". Returns bytes written.
fn fmt_mac(buf: &mut [u8], mac: [u8; 6]) -> usize {
    const HEX: &[u8] = b"0123456789abcdef";
    let mut p = 0usize;
    for i in 0..6 {
        if i > 0 && p < buf.len() { buf[p] = b':'; p += 1; }
        if p < buf.len() { buf[p] = HEX[(mac[i] >> 4) as usize]; p += 1; }
        if p < buf.len() { buf[p] = HEX[(mac[i] & 0xF) as usize]; p += 1; }
    }
    p
}

/// `dns <hostname>` — resolve a hostname to an IPv4 address via QEMU's DNS at 10.0.2.3.
fn cmd_dns(args: &str) {
    let name = args.trim();
    if name.is_empty() {
        TERM.lock().push_str("Usage: dns <hostname>  e.g. dns google.com", ERR_COL);
        return;
    }
    if !crate::net::driver::is_ready() {
        TERM.lock().push_str("dns: NIC not ready", ERR_COL);
        return;
    }

    {
        let mut t = TERM.lock();
        let mut buf = [0u8; LINE_BUF];
        let pfx = b"Resolving ";
        let mut p = pfx.len().min(LINE_BUF);
        buf[..p].copy_from_slice(&pfx[..p]);
        let nb = name.as_bytes();
        let nl = nb.len().min(LINE_BUF - p);
        buf[p..p + nl].copy_from_slice(&nb[..nl]);
        p += nl;
        if p < LINE_BUF { buf[p] = b'.'; p += 1; }
        if p < LINE_BUF { buf[p] = b'.'; p += 1; }
        if p < LINE_BUF { buf[p] = b'.'; p += 1; }
        t.push_str(unsafe { core::str::from_utf8_unchecked(&buf[..p]) }, TEXT_NORM);
    }

    match crate::net::dns::resolve(name, 3000) {
        Ok(ip) => {
            let mut buf = [0u8; LINE_BUF];
            let pfx = b"  -> ";
            let mut p = pfx.len();
            buf[..p].copy_from_slice(pfx);
            p += write_ipv4(&mut buf[p..], ip);
            TERM.lock().push_str(unsafe { core::str::from_utf8_unchecked(&buf[..p]) }, 0x66FF66);
        }
        Err(crate::net::dns::DnsError::ArpFailed) => {
            TERM.lock().push_str("dns: gateway ARP failed (NIC or slirp unreachable)", ERR_COL);
        }
        Err(crate::net::dns::DnsError::SendFailed) => {
            TERM.lock().push_str("dns: UDP send failed (NIC TX error)", ERR_COL);
        }
        Err(crate::net::dns::DnsError::NxDomain) => {
            TERM.lock().push_str("dns: NXDOMAIN (name does not exist)", ERR_COL);
        }
        Err(crate::net::dns::DnsError::RcodeError(rc)) => {
            let mut buf = [0u8; LINE_BUF];
            let pfx = b"dns: server error RCODE=";
            let mut p = pfx.len().min(LINE_BUF);
            buf[..p].copy_from_slice(&pfx[..p]);
            p += write_dec(&mut buf[p..], rc as u64);
            let hint: &[u8] = match rc {
                2 => b" (SERVFAIL - upstream resolver failed)",
                3 => b" (NXDOMAIN)",
                5 => b" (REFUSED)",
                _ => b"",
            };
            let hl = hint.len().min(LINE_BUF - p);
            buf[p..p+hl].copy_from_slice(&hint[..hl]); p += hl;
            TERM.lock().push_str(unsafe { core::str::from_utf8_unchecked(&buf[..p]) }, ERR_COL);
        }
        Err(_) => {
            TERM.lock().push_str("dns: no response (timeout)", ERR_COL);
        }
    }
}

/// `http <url>` — fetch a URL via HTTP/1.0 GET and display the response body.
/// URL format: http://host[:port]/path   (https not supported)
fn cmd_http(args: &str) {
    let url = args.trim();
    if url.is_empty() {
        TERM.lock().push_str("Usage: http <url>  e.g. http http://example.com/", ERR_COL);
        return;
    }

    // Strip "http://" prefix
    let rest = if url.starts_with("http://") {
        &url[7..]
    } else if url.starts_with("http:/") {
        &url[6..]
    } else {
        url
    };

    // Split host[:port] from path
    let (host_port, path) = if let Some(slash) = rest.find('/') {
        (&rest[..slash], &rest[slash..])
    } else {
        (rest, "/")
    };

    // Split host from optional :port
    let (host, port) = if let Some(colon) = host_port.rfind(':') {
        let port_str = &host_port[colon + 1..];
        let mut p = 0u16;
        let mut ok = true;
        for b in port_str.bytes() {
            if b < b'0' || b > b'9' { ok = false; break; }
            p = p.saturating_mul(10).saturating_add((b - b'0') as u16);
        }
        if ok && p > 0 { (&host_port[..colon], p) } else { (host_port, 80u16) }
    } else {
        (host_port, 80u16)
    };

    {
        let mut t = TERM.lock();
        let mut buf = [0u8; LINE_BUF];
        let pfx = b"GET http://";
        let mut p = pfx.len().min(LINE_BUF);
        buf[..p].copy_from_slice(&pfx[..p]);
        let hb = host.as_bytes();
        let hl = hb.len().min(LINE_BUF - p);
        buf[p..p + hl].copy_from_slice(&hb[..hl]);
        p += hl;
        let pb2 = path.as_bytes();
        let pl2 = pb2.len().min(LINE_BUF - p);
        buf[p..p + pl2].copy_from_slice(&pb2[..pl2]);
        p += pl2;
        t.push_str(unsafe { core::str::from_utf8_unchecked(&buf[..p]) }, TEXT_NORM);
    }

    // Static response buffer (4 KiB — enough for most short responses)
    static mut HTTP_BUF: [u8; 4096] = [0u8; 4096];
    let resp_buf = unsafe { &mut HTTP_BUF };

    match crate::net::http::get(host, port, path, resp_buf) {
        Err(e) => {
            let msg = match e {
                crate::net::http::HttpError::NicNotReady    => "http: NIC not ready",
                crate::net::http::HttpError::DnsTimeout     => "http: DNS timeout",
                crate::net::http::HttpError::ConnectTimeout => "http: connect timeout",
                crate::net::http::HttpError::SendFailed     => "http: send failed",
                crate::net::http::HttpError::ResponseTimeout=> "http: response timeout",
                crate::net::http::HttpError::BufferTooSmall => "http: response truncated (buffer full)",
            };
            TERM.lock().push_str(msg, ERR_COL);
        }
        Ok(n) => {
            // Find end of headers (first \r\n\r\n), display body only
            let body_start = find_body_start(&resp_buf[..n]).unwrap_or(0);
            let body = &resp_buf[body_start..n];
            // Print response line-by-line (terminal push_str takes &str)
            let mut line_start = 0usize;
            let mut lines_shown = 0usize;
            const MAX_LINES: usize = 40;
            while line_start < body.len() && lines_shown < MAX_LINES {
                let line_end = body[line_start..].iter()
                    .position(|&b| b == b'\n')
                    .map(|i| line_start + i + 1)
                    .unwrap_or(body.len());
                let line_bytes = &body[line_start..line_end];
                // Strip trailing \r\n and non-printable bytes for display
                let printable_end = line_bytes.iter()
                    .rposition(|&b| b > b' ')
                    .map(|i| i + 1)
                    .unwrap_or(0);
                if printable_end > 0 {
                    // Replace non-ASCII with '?'
                    let mut display = [0u8; LINE_BUF];
                    let len = printable_end.min(LINE_BUF);
                    for (i, &b) in line_bytes[..len].iter().enumerate() {
                        display[i] = if b >= 0x20 && b < 0x80 { b } else { b'?' };
                    }
                    let s = unsafe { core::str::from_utf8_unchecked(&display[..len]) };
                    TERM.lock().push_str(s, TEXT_NORM);
                    lines_shown += 1;
                }
                line_start = line_end;
            }
            if n > 0 {
                let mut buf2 = [0u8; LINE_BUF];
                let pfx = b"--- ";
                let mut p2 = pfx.len();
                buf2[..p2].copy_from_slice(pfx);
                p2 += write_dec(&mut buf2[p2..], n as u64);
                let sfx = b" bytes received ---";
                let sl = sfx.len().min(LINE_BUF - p2);
                buf2[p2..p2 + sl].copy_from_slice(&sfx[..sl]);
                p2 += sl;
                TERM.lock().push_str(unsafe { core::str::from_utf8_unchecked(&buf2[..p2]) }, 0x88CCFF);
            }
        }
    }
}

/// `netcheck [n]` — run gateway ping + DNS + HTTP checks repeatedly.
/// Default loops: 3. Max loops: 9.
fn cmd_netcheck(args: &str) {
    let loops = {
        let a = args.trim();
        if a.is_empty() {
            3usize
        } else {
            let mut v = 0usize;
            let mut ok = false;
            for b in a.bytes() {
                if b < b'0' || b > b'9' {
                    v = 0;
                    ok = false;
                    break;
                }
                ok = true;
                v = v.saturating_mul(10).saturating_add((b - b'0') as usize);
            }
            if ok { v.clamp(1, 9) } else { 3 }
        }
    };

    if !crate::net::driver::is_ready() {
        TERM.lock().push_str("netcheck: NIC not ready", ERR_COL);
        return;
    }
    let cfg = match crate::net::config::get() {
        Some(c) => c,
        None => {
            TERM.lock().push_str("netcheck: IP not configured", ERR_COL);
            return;
        }
    };

    let mut ping_pass = 0usize;
    let mut dns_pass = 0usize;
    let mut http_pass = 0usize;

    for i in 0..loops {
        // Header: "netcheck run 1/3"
        let mut hdr = [0u8; LINE_BUF];
        let mut hp = 0usize;
        let pfx = b"netcheck run ";
        let pl = pfx.len().min(LINE_BUF);
        hdr[..pl].copy_from_slice(&pfx[..pl]);
        hp += pl;
        hp += write_dec(&mut hdr[hp..], (i + 1) as u64);
        if hp < LINE_BUF { hdr[hp] = b'/'; hp += 1; }
        hp += write_dec(&mut hdr[hp..], loops as u64);
        TERM.lock().push_str(unsafe { core::str::from_utf8_unchecked(&hdr[..hp]) }, 0x88CCFF);

        // Check 1: Ping gateway
        let ping_ok = {
            let gw = cfg.gateway;
            match resolve_arp(gw) {
                Some(dst_mac) => {
                    let id = 0xB200u16;
                    let seq = i as u16;
                    crate::net::icmp::send_echo_request_to(gw, dst_mac, id, seq);
                    let deadline = crate::arch::x86_64::interrupts::uptime_ms() + 1200;
                    let mut got = false;
                    while crate::arch::x86_64::interrupts::uptime_ms() < deadline {
                        crate::net::poll_and_dispatch();
                        if let Some(reply) = crate::net::icmp::poll_reply() {
                            if reply.id == id && reply.seq == seq {
                                got = true;
                                break;
                            }
                        }
                        crate::arch::x86_64::halt::idle_once();
                    }
                    got
                }
                None => false,
            }
        };
        if ping_ok { ping_pass += 1; }
        TERM.lock().push_str(if ping_ok { "  ping: pass" } else { "  ping: fail" },
                             if ping_ok { 0x66FF66 } else { ERR_COL });

        // Check 2: DNS
        let dns_ok = crate::net::dns::resolve("example.com", 3000).is_ok();
        if dns_ok { dns_pass += 1; }
        TERM.lock().push_str(if dns_ok { "  dns:  pass" } else { "  dns:  fail" },
                             if dns_ok { 0x66FF66 } else { ERR_COL });

        // Check 3: HTTP
        static mut NETCHECK_HTTP_BUF: [u8; 4096] = [0u8; 4096];
        let http_result = crate::net::http::get("example.com", 80, "/", unsafe { &mut NETCHECK_HTTP_BUF });
        let http_ok = match http_result {
            Ok(n) => n > 0,
            Err(crate::net::http::HttpError::BufferTooSmall) => true,
            Err(_) => false,
        };
        if http_ok { http_pass += 1; }
        let http_msg: &str = if http_ok { "  http: pass" } else {
            match http_result {
                Err(crate::net::http::HttpError::ConnectTimeout) => "  http: fail (connect timeout)",
                Err(crate::net::http::HttpError::SendFailed)     => "  http: fail (send failed)",
                Err(crate::net::http::HttpError::ResponseTimeout)=> "  http: fail (response timeout)",
                Err(crate::net::http::HttpError::DnsTimeout)     => "  http: fail (dns timeout)",
                _ => "  http: fail",
            }
        };
        TERM.lock().push_str(http_msg, if http_ok { 0x66FF66 } else { ERR_COL });
    }

    // Summary
    let mut l1 = [0u8; LINE_BUF];
    let mut p1 = 0usize;
    let pfx1 = b"summary ping: ";
    l1[..pfx1.len()].copy_from_slice(pfx1);
    p1 += pfx1.len();
    p1 += write_dec(&mut l1[p1..], ping_pass as u64);
    if p1 < LINE_BUF { l1[p1] = b'/'; p1 += 1; }
    p1 += write_dec(&mut l1[p1..], loops as u64);
    TERM.lock().push_str(unsafe { core::str::from_utf8_unchecked(&l1[..p1]) }, if ping_pass == loops { 0x66FF66 } else { ERR_COL });

    let mut l2 = [0u8; LINE_BUF];
    let mut p2 = 0usize;
    let pfx2 = b"summary dns:  ";
    l2[..pfx2.len()].copy_from_slice(pfx2);
    p2 += pfx2.len();
    p2 += write_dec(&mut l2[p2..], dns_pass as u64);
    if p2 < LINE_BUF { l2[p2] = b'/'; p2 += 1; }
    p2 += write_dec(&mut l2[p2..], loops as u64);
    TERM.lock().push_str(unsafe { core::str::from_utf8_unchecked(&l2[..p2]) }, if dns_pass == loops { 0x66FF66 } else { ERR_COL });

    let mut l3 = [0u8; LINE_BUF];
    let mut p3 = 0usize;
    let pfx3 = b"summary http: ";
    l3[..pfx3.len()].copy_from_slice(pfx3);
    p3 += pfx3.len();
    p3 += write_dec(&mut l3[p3..], http_pass as u64);
    if p3 < LINE_BUF { l3[p3] = b'/'; p3 += 1; }
    p3 += write_dec(&mut l3[p3..], loops as u64);
    TERM.lock().push_str(unsafe { core::str::from_utf8_unchecked(&l3[..p3]) }, if http_pass == loops { 0x66FF66 } else { ERR_COL });
}

/// Find the offset of the HTTP body (after \r\n\r\n).
fn find_body_start(data: &[u8]) -> Option<usize> {
    let mut i = 0usize;
    while i + 3 < data.len() {
        if data[i] == b'\r' && data[i+1] == b'\n' && data[i+2] == b'\r' && data[i+3] == b'\n' {
            return Some(i + 4);
        }
        i += 1;
    }
    None
}

fn cmd_exec(args: &str) {
    let prog = args.trim();
    let (elf, prog_name): (&[u8], &'static str) = match prog {
        "hello"    => (crate::loader::HELLO_ELF, "hello"),
        "gui"      => (crate::loader::GUI_DEMO_ELF, "gui"),
        "nxbomb"   => (crate::loader::NXBOMB_ELF, "nxbomb"),
        "stackbomb"=> (crate::loader::STACKBOMB_ELF, "stackbomb"),
        _ => {
            let mut t = TERM.lock();
            t.push_str("exec: unknown program", ERR_COL);
            t.push_str("  known: hello  gui  nxbomb  stackbomb", TEXT_NORM);
            return;
        }
    };

    // Reject if a user process is already running (shared page tables, fixed vaddrs).
    if crate::process::count_running_user() > 0 {
        TERM.lock().push_str("exec: a user process is already running", ERR_COL);
        return;
    }

    match crate::process::spawn_elf_process(prog_name, elf, crate::user::USER_TASK_STACK_VIRT, 128) {
        Some(pid) => {
            let mut t = TERM.lock();
            let mut buf = [0u8; LINE_BUF];
            let mut p = 0usize;
            let pfx = b"spawned  pid=";
            buf[..pfx.len()].copy_from_slice(pfx);
            p += pfx.len();
            p += write_dec(&mut buf[p..], pid.0);
            let s = unsafe { core::str::from_utf8_unchecked(&buf[..p]) };
            t.push_str(s, 0x66FF66);
        }
        None => {
            TERM.lock().push_str("exec: spawn failed", ERR_COL);
        }
    }
}

fn cmd_ps() {
    let (entries, count) = crate::process::list_all();
    let mut t = TERM.lock();
    if count == 0 {
        t.push_str("no processes", TEXT_NORM);
        return;
    }
    t.push_str("PID  STATE    TASK  NAME", TEXT_NORM);
    for i in 0..count {
        let e = &entries[i];
        let state_str: &[u8] = match e.state {
            crate::process::ProcessState::Running => b"running ",
            crate::process::ProcessState::Exited  => b"exited  ",
            crate::process::ProcessState::Empty   => b"empty   ",
        };
        // Build "PID  STATE    TASK  name"
        let mut buf = [0u8; LINE_BUF];
        let mut p = 0usize;
        p += write_dec(&mut buf[p..], e.pid);
        buf[p] = b' '; p += 1;
        let sl = state_str.len().min(LINE_BUF - p);
        buf[p..p + sl].copy_from_slice(&state_str[..sl]); p += sl;
        p += write_dec(&mut buf[p..], e.task_id);
        buf[p] = b' '; p += 1;
        let nl = e.name_len.min(LINE_BUF - p);
        buf[p..p + nl].copy_from_slice(&e.name[..nl]); p += nl;
        let s = unsafe { core::str::from_utf8_unchecked(&buf[..p]) };
        let col = if e.state == crate::process::ProcessState::Running { 0x66FF66 } else { 0xAAAAAA };
        t.push_str(s, col);
    }
}

fn cmd_kill(args: &str) {
    let pid_str = args.trim();
    let mut pid_val = 0u64;
    for b in pid_str.bytes() {
        if b < b'0' || b > b'9' { break; }
        pid_val = pid_val * 10 + (b - b'0') as u64;
    }
    if pid_val == 0 {
        TERM.lock().push_str("kill: usage: kill <pid>", ERR_COL);
        return;
    }
    let pid = crate::process::ProcessId(pid_val);
    match crate::process::main_task(pid) {
        Some(task_id) => {
            crate::scheduler::exit_task(task_id);
            let mut t = TERM.lock();
            let mut buf = [0u8; LINE_BUF];
            let mut p = 0usize;
            let pfx = b"killed pid=";
            buf[..pfx.len()].copy_from_slice(pfx);
            p += pfx.len();
            p += write_dec(&mut buf[p..], pid_val);
            let s = unsafe { core::str::from_utf8_unchecked(&buf[..p]) };
            t.push_str(s, TEXT_NORM);
        }
        None => {
            TERM.lock().push_str("kill: no such process", ERR_COL);
        }
    }
}

// ── Path helpers ──────────────────────────────────────────────────────────────

/// Returns the current FAT32 cluster (root if at /).
fn cwd_cluster() -> Option<u32> {
    if !crate::fat32::is_mounted() { return None; }
    let t = TERM.lock();
    if t.cwd_plen == 0 {
        Some(crate::fat32::root_cluster())
    } else {
        Some(t.cwd_cluster)
    }
}

/// Walk a slash-separated path from root and return its cluster.
fn walk_path_to_cluster(path: &str) -> Option<u32> {
    if !crate::fat32::is_mounted() { return None; }
    let mut cluster = crate::fat32::root_cluster();
    for seg in path.split('/').filter(|s| !s.is_empty()) {
        let de = crate::fat32::find_in_dir(cluster, seg.as_bytes())?;
        if !de.is_dir { return None; }
        cluster = de.cluster;
    }
    Some(cluster)
}

/// Resolve an optional path argument to its FAT32 cluster.
/// If path is empty, returns cwd. Otherwise resolves relative to cwd.
fn resolve_cluster_for_path(path: &str) -> Option<u32> {
    if path.is_empty() { return cwd_cluster(); }
    if path.starts_with('/') {
        // absolute path
        return walk_path_to_cluster(path);
    }
    // relative: prepend cwd
    let parent = cwd_cluster()?;
    let de = crate::fat32::find_in_dir(parent, path.as_bytes())?;
    if de.is_dir { Some(de.cluster) } else { None }
}

fn write_dec(buf: &mut [u8], mut n: u64) -> usize {
    if buf.is_empty() { return 0; }
    if n == 0 { buf[0] = b'0'; return 1; }
    let mut tmp = [0u8; 20];
    let mut pos = tmp.len();
    while n > 0 {
        pos -= 1;
        tmp[pos] = b'0' + (n % 10) as u8;
        n /= 10;
    }
    let len = (tmp.len() - pos).min(buf.len());
    buf[..len].copy_from_slice(&tmp[pos..pos + len]);
    len
}

fn write_hex64(buf: &mut [u8], n: u64) -> usize {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    if buf.len() < 18 { return 0; }
    buf[0] = b'0';
    buf[1] = b'x';
    for i in 0..16 {
        let nyb = ((n >> ((15 - i) * 4)) & 0xF) as usize;
        buf[2 + i] = HEX[nyb];
    }
    18
}

fn cmd_memprobe() {
    use crate::memory::paging::{
        current_cr3_phys, is_user_range, is_user_virt, is_kernel_virt,
        lookup_page_entry_current, KERNEL_SPACE_BASE, PageTableFlags, USER_SPACE_LIMIT,
    };

    let mut t = TERM.lock();
    t.push_str("memprobe: kernel/user isolation diagnostic", TEXT_NORM);

    // Constants line
    {
        let mut buf = [0u8; LINE_BUF];
        let mut p = 0usize;
        let pfx = b"  USER_SPACE_LIMIT  = ";
        buf[..pfx.len()].copy_from_slice(pfx); p += pfx.len();
        p += write_hex64(&mut buf[p..], USER_SPACE_LIMIT as u64);
        let s = unsafe { core::str::from_utf8_unchecked(&buf[..p]) };
        t.push_str(s, TEXT_NORM);
    }
    {
        let mut buf = [0u8; LINE_BUF];
        let mut p = 0usize;
        let pfx = b"  KERNEL_SPACE_BASE = ";
        buf[..pfx.len()].copy_from_slice(pfx); p += pfx.len();
        p += write_hex64(&mut buf[p..], KERNEL_SPACE_BASE as u64);
        let s = unsafe { core::str::from_utf8_unchecked(&buf[..p]) };
        t.push_str(s, TEXT_NORM);
    }
    {
        let mut buf = [0u8; LINE_BUF];
        let mut p = 0usize;
        let pfx = b"  current CR3       = ";
        buf[..pfx.len()].copy_from_slice(pfx); p += pfx.len();
        p += write_hex64(&mut buf[p..], current_cr3_phys() as u64);
        let s = unsafe { core::str::from_utf8_unchecked(&buf[..p]) };
        t.push_str(s, TEXT_NORM);
    }

    // Address-space classifier checks
    let user_addr  = 0x0000_0000_0040_0000usize;
    let kernel_addr = KERNEL_SPACE_BASE;
    let bad_addr   = USER_SPACE_LIMIT; // exactly the boundary, must be neither user nor a valid range

    let line = |t: &mut TermState, label: &[u8], ok: bool| {
        let mut buf = [0u8; LINE_BUF];
        let mut p = 0usize;
        buf[..label.len()].copy_from_slice(label); p += label.len();
        let tail: &[u8] = if ok { b"PASS" } else { b"FAIL" };
        let n = tail.len().min(LINE_BUF - p);
        buf[p..p + n].copy_from_slice(&tail[..n]); p += n;
        let s = unsafe { core::str::from_utf8_unchecked(&buf[..p]) };
        t.push_str(s, if ok { 0x66FF66 } else { ERR_COL });
    };

    line(&mut t, b"  is_user_virt(user_addr)        = ", is_user_virt(user_addr));
    line(&mut t, b"  is_kernel_virt(kernel_addr)    = ", is_kernel_virt(kernel_addr));
    line(&mut t, b"  !is_user_virt(kernel_addr)     = ", !is_user_virt(kernel_addr));
    line(&mut t, b"  !is_user_range(kernel_addr,1)  = ", !is_user_range(kernel_addr, 1));
    line(&mut t, b"  !is_user_range(bad_addr,1)     = ", !is_user_range(bad_addr, 1));

    // Page-table entry checks: kernel mappings must NOT be USER_ACCESSIBLE.
    let kernel_probe = unsafe { lookup_page_entry_current(KERNEL_SPACE_BASE + 0x1000) };
    let kernel_user_bit_clear = match kernel_probe {
        Some(entry) => (entry & PageTableFlags::USER_ACCESSIBLE) == 0,
        None => true, // unmapped is also "not user-accessible"
    };
    line(&mut t, b"  kernel page lacks USER bit     = ", kernel_user_bit_clear);

    // EFER.NXE — required for EXECUTE_DISABLE bit to be honored.
    let efer = crate::arch::x86_64::sysentry::efer();
    let nxe_on = (efer & (1u64 << 11)) != 0;
    line(&mut t, b"  EFER.NXE enabled               = ", nxe_on);

    // CR0.WP — kernel writes respect read-only PTEs.
    let cr0 = crate::arch::x86_64::cpu::cr0();
    let cr4 = crate::arch::x86_64::cpu::cr4();
    let wp_on = (cr0 & (1u64 << 16)) != 0;
    let smep_on = (cr4 & (1u64 << 20)) != 0;
    let smap_on = (cr4 & (1u64 << 21)) != 0;
    let umip_on = (cr4 & (1u64 << 11)) != 0;
    let smep_avail = crate::arch::x86_64::cpu::has_smep();
    let smap_avail = crate::arch::x86_64::cpu::has_smap();
    let umip_avail = crate::arch::x86_64::cpu::has_umip();
    line(&mut t, b"  CR0.WP enabled                 = ", wp_on);
    // SMEP: PASS if enabled, or PASS if not supported by host (TCG often).
    line(&mut t, b"  CR4.SMEP enabled               = ", smep_on || !smep_avail);
    // UMIP: same gating.
    line(&mut t, b"  CR4.UMIP enabled               = ", umip_on || !umip_avail);
    // SMAP: PASS if enabled, or PASS if not supported.
    line(&mut t, b"  CR4.SMAP enabled               = ", smap_on || !smap_avail);
    let _ = smap_avail; // suppress unused warning when SMAP is on
    let _ = smap_on;

    // Process count + currently-tracked owned frames for the running task.
    let (_entries, count) = crate::process::list_all();
    {
        let mut buf = [0u8; LINE_BUF];
        let mut p = 0usize;
        let pfx = b"  user processes tracked        = ";
        buf[..pfx.len()].copy_from_slice(pfx); p += pfx.len();
        p += write_dec(&mut buf[p..], count as u64);
        let s = unsafe { core::str::from_utf8_unchecked(&buf[..p]) };
        t.push_str(s, TEXT_NORM);
    }
    {
        let mut buf = [0u8; LINE_BUF];
        let mut p = 0usize;
        let pfx = b"  free physical frames          = ";
        buf[..pfx.len()].copy_from_slice(pfx); p += pfx.len();
        p += write_dec(&mut buf[p..], crate::memory::frame_allocator::available_frames() as u64);
        let s = unsafe { core::str::from_utf8_unchecked(&buf[..p]) };
        t.push_str(s, TEXT_NORM);
    }
    {
        let snap = crate::syscall::security_authz_snapshot();
        let mut buf = [0u8; LINE_BUF];
        let mut p = 0usize;
        let pfx = b"  syscall authz checks/denied   = ";
        buf[..pfx.len()].copy_from_slice(pfx); p += pfx.len();
        p += write_dec(&mut buf[p..], snap.checks);
        buf[p] = b'/'; p += 1;
        p += write_dec(&mut buf[p..], snap.denied);
        let s = unsafe { core::str::from_utf8_unchecked(&buf[..p]) };
        t.push_str(s, TEXT_NORM);
    }

    t.push_str("memprobe: done", TEXT_NORM);
}

fn cmd_memtest() {
    use crate::memory::paging::{is_user_range, KERNEL_SPACE_BASE, USER_SPACE_LIMIT};
    use crate::syscall::{
        dispatch, SYS_WRITE_CONSOLE, SYS_SEND_MSG, SYS_RECV_MSG,
        SYS_GET_FB_INFO, SYS_DRAW_TEXT,
    };

    let mut t = TERM.lock();
    t.push_str("memtest: pointer-validation regression battery", TEXT_NORM);

    let line = |t: &mut TermState, label: &[u8], pass: bool| {
        let mut buf = [0u8; LINE_BUF];
        let mut p = 0usize;
        buf[..label.len().min(LINE_BUF)].copy_from_slice(&label[..label.len().min(LINE_BUF)]);
        p += label.len().min(LINE_BUF);
        let tail: &[u8] = if pass { b"PASS" } else { b"FAIL" };
        let n = tail.len().min(LINE_BUF.saturating_sub(p));
        buf[p..p + n].copy_from_slice(&tail[..n]); p += n;
        let s = unsafe { core::str::from_utf8_unchecked(&buf[..p]) };
        t.push_str(s, if pass { 0x66FF66 } else { ERR_COL });
    };

    // Range checks (purely arithmetic, deterministic).
    line(&mut t, b"  is_user_range(USER_LIMIT,1)   rejects? ", !is_user_range(USER_SPACE_LIMIT, 1));
    line(&mut t, b"  is_user_range(KERNEL_BASE,1)  rejects? ", !is_user_range(KERNEL_SPACE_BASE, 1));
    line(&mut t, b"  is_user_range(USER_LIMIT-8,16) rejects? ", !is_user_range(USER_SPACE_LIMIT - 8, 16));

    // Syscall validation: each must reject and return 0 (failure sentinel).
    // Running in kernel CR3, so user-range addresses without backing page-tables also fail.
    let kernel_ptr = KERNEL_SPACE_BASE as u64;

    line(&mut t, b"  sys_write_console(KERNEL_PTR) rejects? ", dispatch(SYS_WRITE_CONSOLE, kernel_ptr, 8, 0, 0, 0, 0) == 0);
    line(&mut t, b"  sys_write_console(NULL)       rejects? ", dispatch(SYS_WRITE_CONSOLE, 0, 8, 0, 0, 0, 0) == 0);
    line(&mut t, b"  sys_send_msg(KERNEL_PTR)      rejects? ", dispatch(SYS_SEND_MSG, kernel_ptr, 8, 0, 0, 0, 0) == 0);
    line(&mut t, b"  sys_get_fb_info(KERNEL_PTR)   rejects? ", dispatch(SYS_GET_FB_INFO, kernel_ptr, 0, 0, 0, 0, 0) == 0);
    line(&mut t, b"  sys_draw_text(KERNEL_PTR)     rejects? ", dispatch(SYS_DRAW_TEXT, kernel_ptr, 4, 0, 0, 0xFFFFFF, 0) == 0);

    // recv_msg with a kernel ptr must also fail the writable check; len returned must be 0.
    line(&mut t, b"  sys_recv_msg(KERNEL_PTR)      rejects? ", dispatch(SYS_RECV_MSG, kernel_ptr, 0, 0, 0, 0, 0) == 0);

    t.push_str("memtest: done", TEXT_NORM);
}

fn cmd_cpuinfo() {
    use crate::arch::x86_64::apic;

    let mut t = TERM.lock();
    t.push_str("cpuinfo: CPU and APIC discovery", TEXT_NORM);

    // Vendor (12 bytes from CPUID leaf 0).
    {
        let v = apic::vendor_id();
        let mut buf = [0u8; LINE_BUF];
        let pfx = b"  vendor : ";
        let mut p = 0usize;
        buf[..pfx.len()].copy_from_slice(pfx); p += pfx.len();
        for &b in &v[..12] {
            if b == 0 { break; }
            if p < LINE_BUF { buf[p] = b; p += 1; }
        }
        let s = unsafe { core::str::from_utf8_unchecked(&buf[..p]) };
        t.push_str(s, TEXT_NORM);
    }

    // Brand (48 bytes).
    {
        let br = apic::brand_string();
        let mut buf = [0u8; LINE_BUF];
        let pfx = b"  brand  : ";
        let mut p = 0usize;
        buf[..pfx.len()].copy_from_slice(pfx); p += pfx.len();
        // Skip leading spaces in brand.
        let mut start = 0usize;
        while start < br.len() && br[start] == b' ' { start += 1; }
        for &b in &br[start..] {
            if b == 0 { break; }
            if p < LINE_BUF { buf[p] = b; p += 1; }
        }
        let s = unsafe { core::str::from_utf8_unchecked(&buf[..p]) };
        t.push_str(s, TEXT_NORM);
    }

    // Family/Model/Stepping.
    {
        let (f, m, s) = apic::family_model_stepping();
        let mut buf = [0u8; LINE_BUF];
        let mut p = 0usize;
        let pfx = b"  family/model/step = ";
        buf[..pfx.len()].copy_from_slice(pfx); p += pfx.len();
        p += write_dec(&mut buf[p..], f as u64);
        buf[p] = b'/'; p += 1;
        p += write_dec(&mut buf[p..], m as u64);
        buf[p] = b'/'; p += 1;
        p += write_dec(&mut buf[p..], s as u64);
        let line_str = unsafe { core::str::from_utf8_unchecked(&buf[..p]) };
        t.push_str(line_str, TEXT_NORM);
    }

    // LAPIC ID (current core).
    {
        let mut buf = [0u8; LINE_BUF];
        let mut p = 0usize;
        let pfx = b"  current LAPIC ID    = ";
        buf[..pfx.len()].copy_from_slice(pfx); p += pfx.len();
        p += write_dec(&mut buf[p..], apic::lapic_id() as u64);
        let s = unsafe { core::str::from_utf8_unchecked(&buf[..p]) };
        t.push_str(s, TEXT_NORM);
    }

    // IA32_APIC_BASE breakdown.
    let base = apic::read_apic_base();
    {
        let mut buf = [0u8; LINE_BUF];
        let mut p = 0usize;
        let pfx = b"  APIC_BASE phys      = ";
        buf[..pfx.len()].copy_from_slice(pfx); p += pfx.len();
        p += write_hex64(&mut buf[p..], base.phys);
        let s = unsafe { core::str::from_utf8_unchecked(&buf[..p]) };
        t.push_str(s, TEXT_NORM);
    }

    let line = |t: &mut TermState, label: &[u8], ok: bool| {
        let mut buf = [0u8; LINE_BUF];
        let mut p = 0usize;
        buf[..label.len()].copy_from_slice(label); p += label.len();
        let tail: &[u8] = if ok { b"yes" } else { b"no" };
        let n = tail.len().min(LINE_BUF - p);
        buf[p..p + n].copy_from_slice(&tail[..n]); p += n;
        let s = unsafe { core::str::from_utf8_unchecked(&buf[..p]) };
        t.push_str(s, if ok { 0x66FF66 } else { TEXT_NORM });
    };

    line(&mut t, b"  APIC global enable  = ", base.global_enable);
    line(&mut t, b"  is BSP              = ", base.is_bsp);
    line(&mut t, b"  x2APIC supported    = ", apic::has_x2apic());
    line(&mut t, b"  x2APIC enabled      = ", base.x2apic_enable);
    line(&mut t, b"  APIC feature flag   = ", apic::has_apic());
    line(&mut t, b"  invariant TSC       = ", apic::has_invariant_tsc());
    line(&mut t, b"  long mode           = ", apic::has_long_mode());

    // RSDP from Limine.
    {
        let mut buf = [0u8; LINE_BUF];
        let mut p = 0usize;
        let pfx = b"  ACPI RSDP           = ";
        buf[..pfx.len()].copy_from_slice(pfx); p += pfx.len();
        match crate::boot::protocol::rsdp_address() {
            Some(addr) => { p += write_hex64(&mut buf[p..], addr as u64); }
            None => { let m = b"<missing>"; buf[p..p+m.len()].copy_from_slice(m); p += m.len(); }
        }
        let s = unsafe { core::str::from_utf8_unchecked(&buf[..p]) };
        t.push_str(s, TEXT_NORM);
    }

    // Topology from Limine MP request.
    {
        let mut buf = [0u8; LINE_BUF];
        let mut p = 0usize;
        let pfx = b"  CPUs reported       = ";
        buf[..pfx.len()].copy_from_slice(pfx); p += pfx.len();
        match apic::topology() {
            Some((bsp, n)) => {
                p += write_dec(&mut buf[p..], n as u64);
                let mid = b" (BSP LAPIC ID ";
                buf[p..p+mid.len()].copy_from_slice(mid); p += mid.len();
                p += write_dec(&mut buf[p..], bsp as u64);
                buf[p] = b')'; p += 1;
            }
            None => { let m = b"<unavailable>"; buf[p..p+m.len()].copy_from_slice(m); p += m.len(); }
        }
        let s = unsafe { core::str::from_utf8_unchecked(&buf[..p]) };
        t.push_str(s, TEXT_NORM);
    }

    // Per-CPU table.
    {
        use crate::boot::protocol::CpuEntry;
        let mut entries = [CpuEntry { acpi_id: 0, lapic_id: 0 }; 16];
        let n = crate::boot::protocol::mp_cpus(&mut entries);
        for i in 0..n {
            let mut buf = [0u8; LINE_BUF];
            let mut p = 0usize;
            let pfx = b"    cpu acpi_id=";
            buf[..pfx.len()].copy_from_slice(pfx); p += pfx.len();
            p += write_dec(&mut buf[p..], entries[i].acpi_id as u64);
            let mid = b" lapic_id=";
            buf[p..p+mid.len()].copy_from_slice(mid); p += mid.len();
            p += write_dec(&mut buf[p..], entries[i].lapic_id as u64);
            let s = unsafe { core::str::from_utf8_unchecked(&buf[..p]) };
            t.push_str(s, TEXT_NORM);
        }
    }

    // MADT-derived I/O APIC and IRQ override summary.
    {
        let mut buf = [0u8; LINE_BUF];
        let mut p = 0usize;
        let pfx = b"  MADT revision      = ";
        buf[..pfx.len()].copy_from_slice(pfx); p += pfx.len();
        p += write_dec(&mut buf[p..], crate::acpi::madt_revision() as u64);
        let mid = b"  PCAT-compat=";
        buf[p..p+mid.len()].copy_from_slice(mid); p += mid.len();
        let v: &[u8] = if crate::acpi::pcat_compat() { b"yes" } else { b"no" };
        buf[p..p+v.len()].copy_from_slice(v); p += v.len();
        let s = unsafe { core::str::from_utf8_unchecked(&buf[..p]) };
        t.push_str(s, TEXT_NORM);
    }
    for io in crate::acpi::io_apics() {
        let mut buf = [0u8; LINE_BUF];
        let mut p = 0usize;
        let pfx = b"    ioapic id=";
        buf[..pfx.len()].copy_from_slice(pfx); p += pfx.len();
        p += write_dec(&mut buf[p..], io.id as u64);
        let mid = b" addr=";
        buf[p..p+mid.len()].copy_from_slice(mid); p += mid.len();
        p += write_hex64(&mut buf[p..], io.address as u64);
        let mid = b" gsi_base=";
        buf[p..p+mid.len()].copy_from_slice(mid); p += mid.len();
        p += write_dec(&mut buf[p..], io.gsi_base as u64);
        let s = unsafe { core::str::from_utf8_unchecked(&buf[..p]) };
        t.push_str(s, TEXT_NORM);
    }
    for ov in crate::acpi::overrides() {
        let mut buf = [0u8; LINE_BUF];
        let mut p = 0usize;
        let pfx = b"    irq_override bus=";
        buf[..pfx.len()].copy_from_slice(pfx); p += pfx.len();
        p += write_dec(&mut buf[p..], ov.bus as u64);
        let mid = b" irq=";
        buf[p..p+mid.len()].copy_from_slice(mid); p += mid.len();
        p += write_dec(&mut buf[p..], ov.source_irq as u64);
        let mid = b" gsi=";
        buf[p..p+mid.len()].copy_from_slice(mid); p += mid.len();
        p += write_dec(&mut buf[p..], ov.gsi as u64);
        let mid = b" flags=";
        buf[p..p+mid.len()].copy_from_slice(mid); p += mid.len();
        p += write_hex64(&mut buf[p..], ov.flags as u64);
        let s = unsafe { core::str::from_utf8_unchecked(&buf[..p]) };
        t.push_str(s, TEXT_NORM);
    }

    t.push_str("cpuinfo: done", TEXT_NORM);
}

fn cmd_apictest() {
    use crate::arch::x86_64::{apic, interrupts};

    let mut t = TERM.lock();
    t.push_str("apictest: switching tick source PIT -> LAPIC -> PIT", TEXT_NORM);

    if !apic::lapic_calibrated() {
        t.push_str("  LAPIC timer not calibrated; aborting.", ERR_COL);
        return;
    }

    let line_kv = |t: &mut TermState, label: &[u8], value: u64| {
        let mut buf = [0u8; LINE_BUF];
        let mut p = 0usize;
        buf[..label.len()].copy_from_slice(label); p += label.len();
        p += write_dec(&mut buf[p..], value);
        let s = unsafe { core::str::from_utf8_unchecked(&buf[..p]) };
        t.push_str(s, TEXT_NORM);
    };

    // Drop the terminal lock while we busy-wait so other tasks (the timer
    // tick processing in particular) can run unimpeded.
    drop(t);

    // Baseline: tick rate from PIT for ~250ms.
    let t0_ticks = interrupts::timer_ticks();
    let t0_ms = interrupts::uptime_ms();
    let target_pit = t0_ms + 250;
    while interrupts::uptime_ms() < target_pit {
        core::hint::spin_loop();
    }
    let pit_delta = interrupts::timer_ticks() - t0_ticks;
    let pit_ms = interrupts::uptime_ms() - t0_ms;

    let mut t = TERM.lock();
    line_kv(&mut t, b"  PIT phase ticks   = ", pit_delta);
    line_kv(&mut t, b"  PIT phase ms est  = ", pit_ms);
    drop(t);

    // Switch to LAPIC at the same logical 100Hz.
    let installed = apic::install_lapic_timer(100);

    let mut t = TERM.lock();
    if !installed {
        t.push_str("  install_lapic_timer FAILED; PIT still active", ERR_COL);
        return;
    }
    t.push_str("  LAPIC tick source ENGAGED", 0x66FF66);
    drop(t);

    // Measure LAPIC for ~250ms. Note: uptime_ms() == TIMER_TICKS * 10ms because
    // both PIT and LAPIC fire at 100Hz, so the conversion stays valid.
    let l0_ticks = interrupts::timer_ticks();
    let l0_ms = interrupts::uptime_ms();
    let target_lapic = l0_ms + 250;
    // Hard cycle cap: ~1.5 GHz * 1s = 1.5e9; cap at 4e9 to allow for slow TCG.
    // If uptime_ms hasn't advanced after this many spins, the LAPIC ISR is silent.
    let spin_cap: u64 = 4_000_000_000;
    let mut spins: u64 = 0;
    let mut lapic_silent = false;
    while interrupts::uptime_ms() < target_lapic {
        core::hint::spin_loop();
        spins = spins.wrapping_add(1);
        if spins > spin_cap {
            lapic_silent = true;
            break;
        }
    }
    let lapic_delta = interrupts::timer_ticks() - l0_ticks;
    let lapic_ms = interrupts::uptime_ms() - l0_ms;

    // Restore PIT immediately so the rest of the system keeps running on the
    // proven tick path.
    let _ = apic::uninstall_lapic_timer();

    let mut t = TERM.lock();
    line_kv(&mut t, b"  LAPIC phase ticks = ", lapic_delta);
    line_kv(&mut t, b"  LAPIC phase ms est= ", lapic_ms);
    t.push_str("  PIT tick source RESTORED", 0x66FF66);

    if lapic_silent {
        t.push_str("apictest: FAIL (LAPIC ISR did not fire within spin cap)", ERR_COL);
        return;
    }

    let ok = lapic_delta > 0 && (lapic_delta as i64 - pit_delta as i64).abs() <= (pit_delta as i64 / 4 + 5);
    if ok {
        t.push_str("apictest: PASS", 0x66FF66);
    } else {
        t.push_str("apictest: FAIL (LAPIC tick rate diverged)", ERR_COL);
    }
}

// ── App-trait wrapper ─────────────────────────────────────────────────────────

use crate::app::{App, AppAction};

/// Single-instance terminal window.  Delegates all state to module-level globals.
pub struct TerminalApp;

impl TerminalApp {
    pub fn new() -> Self {
        init_if_needed();
        TerminalApp
    }
}

impl App for TerminalApp {
    fn title(&self) -> &str { "Terminal" }
    fn preferred_size(&self) -> (usize, usize) { (700, 460) }
    fn app_id(&self) -> &'static str { "terminal" }
    fn allow_multiple_instances(&self) -> bool { false }

    fn render(&self, cx: usize, cy: usize, cw: usize, ch: usize) {
        render(cx, cy, cw, ch);
    }

    fn input_region_height(&self) -> Option<usize> {
        Some(INPUT_REGION_H)
    }

    fn render_input_region(&self, cx: usize, cy: usize, cw: usize, ch: usize) {
        render_input_line(cx, cy, cw, ch);
    }

    fn handle_key(&mut self, key: crate::input::Key) -> AppAction {
        match handle_key(key) {
            TermAction::Close      => AppAction::Close,
            TermAction::RedrawAll  => AppAction::RedrawAll,
            TermAction::RedrawInput => AppAction::RedrawInput,
            TermAction::Nothing    => AppAction::Nothing,
        }
    }

    fn handle_mouse_scroll(&mut self, delta: i32) -> AppAction {
        let mut t = TERM.lock();
        if delta > 0 {
            t.scroll_off = t.scroll_off.saturating_add(delta as usize);
        } else if delta < 0 {
            t.scroll_off = t.scroll_off.saturating_sub((-delta) as usize);
        }
        AppAction::RedrawAll
    }

    fn refresh_interval_ms(&self) -> Option<u64> { None }
}
