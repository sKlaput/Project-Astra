/// Guarded deeper framebuffer probe.
/// Off by default; enable with Cargo feature `gui-fb-kernel-deep-probe`.
pub(crate) const GUI_FB_DEEP_PROBE: bool = cfg!(feature = "gui-fb-kernel-deep-probe");

/// Experimental ring-3 framebuffer map validation probe.
/// Off by default; enable with Cargo feature `gui-fb-user-deep-probe`.
pub(crate) const GUI_FB_USER_DEEP_PROBE: bool = cfg!(feature = "gui-fb-user-deep-probe");
