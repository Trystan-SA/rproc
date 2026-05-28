//! Thin wrapper over `systemctl --user` for the rprocd autostart toggle.
//!
//! Systemd is the source of truth: nothing here is persisted in rproc's
//! own config — every call re-asks systemd. The unit file itself is
//! shipped by the package at `/usr/lib/systemd/user/rprocd.service`;
//! `systemctl --global enable` from the install script seeds the
//! default-on state for all users.

use std::process::{Command, Stdio};

const UNIT: &str = "rprocd.service";

/// Whether the autostart toggle should be interactable. False when
/// `systemctl --user` isn't reachable (e.g. Flatpak sandbox, no session
/// bus) or when the rprocd unit file isn't installed.
pub fn available() -> bool {
    // `is-enabled` prints one of these tokens on stdout when the unit
    // exists and systemd-user is reachable. Anything else (empty stdout,
    // ENOENT, missing DBUS_SESSION_BUS_ADDRESS) means "unavailable" —
    // we keep the check loose so a future systemd token doesn't grey
    // out the option for users who could otherwise use it.
    let Ok(output) = Command::new("systemctl")
        .args(["--user", "is-enabled", UNIT])
        .stderr(Stdio::null())
        .output()
    else {
        return false;
    };
    matches!(
        String::from_utf8_lossy(&output.stdout).trim(),
        "enabled" | "enabled-runtime" | "disabled" | "masked" | "static" | "alias" | "linked"
    )
}

/// True if the unit is currently enabled (will start at next login).
/// A masked unit reports `masked` with a non-zero exit, which we treat
/// as "not enabled" — the user explicitly opted out.
pub fn is_enabled() -> bool {
    Command::new("systemctl")
        .args(["--user", "is-enabled", UNIT])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// Enable + start the unit. `unmask` first so a prior opt-out (which we
/// implement as a per-user mask) is lifted before enabling; `--now` also
/// brings it up immediately so the next GUI launch this session also
/// benefits, not just the next login.
pub fn enable() -> std::io::Result<()> {
    run(&["--user", "unmask", UNIT])?;
    run(&["--user", "enable", "--now", UNIT])
}

/// Mask + stop the unit. A per-user mask is the only way to neutralise
/// the `--global enable` from the package install: `systemctl --user
/// disable` only removes per-user symlinks, while the install-time
/// symlinks live under `/etc/systemd/user/` and would otherwise leave
/// the unit reporting `enabled` and starting at next login.
pub fn disable() -> std::io::Result<()> {
    run(&["--user", "mask", "--now", UNIT])
}

fn run(args: &[&str]) -> std::io::Result<()> {
    let status = Command::new("systemctl").args(args).status()?;
    if !status.success() {
        return Err(std::io::Error::other(format!(
            "systemctl {args:?} exited with {status}"
        )));
    }
    Ok(())
}
