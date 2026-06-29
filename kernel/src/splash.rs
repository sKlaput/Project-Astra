/// Astra OS boot splash screen.
/// Drawn once at boot, immediately after the framebuffer is available.
use crate::framebuffer;

const BG: u32 = 0x0A0B15;
const TITLE: u32 = 0xE8F4FD;
const ACCENT: u32 = 0x4FC3F7;
const RULE: u32 = 0x1E3A5F;
const SUBTITLE: u32 = 0x546E7A;

pub fn draw_boot_splash() {
    if !framebuffer::ensure_ready() {
        return;
    }
    let Some((w, h)) = framebuffer::dimensions() else {
        return;
    };

    framebuffer::clear(BG);

    // "ASTRA" — large title
    let scale: usize = 5;
    let char_step = 6 * scale;
    let glyph_h = 7 * scale;

    let title = "ASTRA";
    let title_px_w = title.len() * char_step;

    // "OS" at smaller scale, baseline-aligned
    let os_scale: usize = 3;
    let os_char_step = 6 * os_scale;
    let os_px_w = 2 * os_char_step;
    let gap = scale * 2;
    let total_w = title_px_w + gap + os_px_w;

    let block_x = w.saturating_sub(total_w) / 2;
    let block_y = h.saturating_sub(glyph_h + 60) / 2;

    framebuffer::draw_text_scaled(block_x, block_y, title, TITLE, scale);

    let os_x = block_x + title_px_w + gap;
    let os_y = block_y + glyph_h - 7 * os_scale;
    framebuffer::draw_text_scaled(os_x, os_y, "OS", ACCENT, os_scale);

    // Separator
    let rule_y = block_y + glyph_h + 14;
    framebuffer::fill_rect(block_x, rule_y, total_w, 1, RULE);

    // Subtitle
    framebuffer::draw_text_at(block_x, rule_y + 8, "Loading system...", SUBTITLE);

    // Present the composed splash to screen
    framebuffer::present_full();
}
