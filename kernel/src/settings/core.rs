impl SettingsApp {
    pub fn new() -> Self {
        SettingsApp { tab: 0, row: 0 }
    }

    fn max_rows(&self) -> usize {
        match self.tab {
            0 => NUM_SYSINFO,
            1 => NUM_THEMES,
            2 => 2,
            _ => 0,
        }
    }
}
