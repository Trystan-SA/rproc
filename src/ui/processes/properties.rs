use std::path::PathBuf;

use crate::monitor;

/// Heavy lookups behind the Properties modal — each walks `/proc`. Computed when
/// the modal opens, then re-used until the user clicks Reload or opens another
/// PID. The app turns this into the modal's stat / path rows.
pub struct ProcessPropertiesView {
    pub pid: u32,
    pub cwd: Option<String>,
    pub fd_count: Option<usize>,
    pub configs: Vec<PathBuf>,
}

pub fn build_properties_view(p: &monitor::processes::ProcInfo) -> ProcessPropertiesView {
    ProcessPropertiesView {
        pid: p.pid,
        cwd: monitor::processes::read_cwd(p.pid),
        fd_count: monitor::processes::read_fd_count(p.pid),
        configs: monitor::processes::find_config_paths(&p.name, &p.exe),
    }
}
