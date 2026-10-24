//! Screen brightness, for the `brightness` button action.
//!
//! * **Windows** — external monitors through **DDC/CI** (the monitor's own
//!   control channel over the video cable; most monitors support it, some
//!   need it enabled in their on-screen menu). If no monitor answers — for
//!   example on a laptop — the built-in panel is changed through Windows'
//!   WMI brightness interface.
//! * **Linux** — the kernel's backlight interface (`/sys/class/backlight`),
//!   which covers laptop panels. Writing it needs permission (see the
//!   README).
//!
//! Talking to monitors is slow (tens of milliseconds per monitor), so every
//! change runs on a short-lived background thread and never delays the
//! handling of other buttons.

use log::{debug, warn};

use crate::config::BrightnessChange;

/// Changes the brightness of every screen, in the background.
pub fn adjust(change: BrightnessChange) {
    let spawned = std::thread::Builder::new()
        .name("brightness".into())
        .spawn(move || match imp::adjust(change) {
            Ok(done) => debug!("brightness: {done}"),
            Err(e) => warn!("cannot change the screen brightness: {e}"),
        });
    if let Err(e) = spawned {
        warn!("cannot start the brightness thread: {e}");
    }
}

/// The new value for a screen whose brightness goes from `min` to `max`.
pub fn target(change: BrightnessChange, min: u32, current: u32, max: u32) -> u32 {
    let range = max.saturating_sub(min).max(1);
    let wanted = match change {
        BrightnessChange::To(p) => i64::from(min) + i64::from(range) * i64::from(p) / 100,
        BrightnessChange::By(p) => i64::from(current) + i64::from(range) * i64::from(p) / 100,
    };
    wanted.clamp(i64::from(min), i64::from(max)) as u32
}

#[cfg(windows)]
mod imp {
    use std::os::windows::process::CommandExt;
    use std::process::Command;

    use windows_sys::Win32::Devices::Display::{
        DestroyPhysicalMonitors, GetMonitorBrightness, GetNumberOfPhysicalMonitorsFromHMONITOR,
        GetPhysicalMonitorsFromHMONITOR, SetMonitorBrightness, PHYSICAL_MONITOR,
    };
    use windows_sys::Win32::Foundation::{BOOL, LPARAM, RECT};
    use windows_sys::Win32::Graphics::Gdi::{EnumDisplayMonitors, HDC, HMONITOR};

    use super::target;
    use crate::config::BrightnessChange;

    /// Do not open a console window for the PowerShell fallback.
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;

    pub fn adjust(change: BrightnessChange) -> Result<String, String> {
        let changed = adjust_ddc(change);
        if changed > 0 {
            return Ok(format!("{changed} monitor(s) changed over DDC/CI"));
        }
        adjust_wmi(change)
    }

    /// Changes every monitor that supports DDC/CI; returns how many did.
    fn adjust_ddc(change: BrightnessChange) -> usize {
        let mut monitors: Vec<HMONITOR> = Vec::new();
        // SAFETY: the callback only pushes into the Vec passed as LPARAM,
        // which outlives the call.
        unsafe {
            EnumDisplayMonitors(
                std::ptr::null_mut(),
                std::ptr::null(),
                Some(collect),
                &mut monitors as *mut Vec<HMONITOR> as LPARAM,
            );
        }
        let mut changed = 0;
        for hmon in monitors {
            let mut count = 0u32;
            // SAFETY: valid monitor handle from EnumDisplayMonitors.
            if unsafe { GetNumberOfPhysicalMonitorsFromHMONITOR(hmon, &mut count) } == 0
                || count == 0
            {
                continue;
            }
            // SAFETY: zeroed PHYSICAL_MONITOR is valid; the array has `count` entries.
            let mut physical: Vec<PHYSICAL_MONITOR> =
                vec![unsafe { std::mem::zeroed() }; count as usize];
            if unsafe { GetPhysicalMonitorsFromHMONITOR(hmon, count, physical.as_mut_ptr()) } == 0 {
                continue;
            }
            for m in &physical {
                let (mut min, mut cur, mut max) = (0u32, 0u32, 0u32);
                // SAFETY: valid physical monitor handle.
                let ok = unsafe {
                    GetMonitorBrightness(m.hPhysicalMonitor, &mut min, &mut cur, &mut max)
                } != 0;
                if ok && max > min {
                    let new = target(change, min, cur, max);
                    // SAFETY: valid physical monitor handle.
                    if unsafe { SetMonitorBrightness(m.hPhysicalMonitor, new) } != 0 {
                        changed += 1;
                    }
                }
            }
            // SAFETY: frees the handles obtained above.
            unsafe { DestroyPhysicalMonitors(count, physical.as_ptr()) };
        }
        changed
    }

    unsafe extern "system" fn collect(
        hmon: HMONITOR,
        _dc: HDC,
        _rect: *mut RECT,
        data: LPARAM,
    ) -> BOOL {
        let list = &mut *(data as *mut Vec<HMONITOR>);
        list.push(hmon);
        1
    }

    /// Built-in panels (laptops), through Windows' WMI brightness classes.
    fn adjust_wmi(change: BrightnessChange) -> Result<String, String> {
        let value = match change {
            BrightnessChange::To(p) => format!("{p}"),
            BrightnessChange::By(p) => format!("$b.CurrentBrightness + ({p})"),
        };
        let script = format!(
            "$b = Get-CimInstance -Namespace root/WMI -ClassName WmiMonitorBrightness -ErrorAction Stop | Select-Object -First 1; \
             $n = [Math]::Max(0, [Math]::Min(100, {value})); \
             Get-CimInstance -Namespace root/WMI -ClassName WmiMonitorBrightnessMethods -ErrorAction Stop | \
             Invoke-CimMethod -MethodName WmiSetBrightness -Arguments @{{Timeout=0; Brightness=[byte]$n}} | Out-Null"
        );
        let out = Command::new("powershell.exe")
            .args(["-NoProfile", "-NonInteractive", "-Command", &script])
            .creation_flags(CREATE_NO_WINDOW)
            .output()
            .map_err(|e| format!("cannot run PowerShell: {e}"))?;
        if out.status.success() {
            Ok("built-in screen changed".into())
        } else {
            Err(
                "no monitor accepts brightness changes (enable DDC/CI in the monitor's menu)"
                    .into(),
            )
        }
    }
}

#[cfg(target_os = "linux")]
mod imp {
    use std::fs;
    use std::path::PathBuf;

    use super::target;
    use crate::config::BrightnessChange;

    pub fn adjust(change: BrightnessChange) -> Result<String, String> {
        let dirs: Vec<PathBuf> = fs::read_dir("/sys/class/backlight")
            .map_err(|e| format!("no backlight interface ({e})"))?
            .filter_map(|e| e.ok().map(|e| e.path()))
            .collect();
        if dirs.is_empty() {
            return Err("no screen with an adjustable backlight".into());
        }
        let read = |p: PathBuf| -> Result<u32, String> {
            fs::read_to_string(&p)
                .map_err(|e| format!("{}: {e}", p.display()))?
                .trim()
                .parse()
                .map_err(|e| format!("{}: {e}", p.display()))
        };
        let mut done = 0;
        for dir in dirs {
            let (cur, max) = (
                read(dir.join("brightness"))?,
                read(dir.join("max_brightness"))?,
            );
            let new = target(change, 0, cur, max);
            fs::write(dir.join("brightness"), new.to_string())
                .map_err(|e| format!("{}: {e} (see README: Linux permissions)", dir.display()))?;
            done += 1;
        }
        Ok(format!("{done} backlight(s) changed"))
    }
}

#[cfg(not(any(windows, target_os = "linux")))]
mod imp {
    use crate::config::BrightnessChange;

    pub fn adjust(_change: BrightnessChange) -> Result<String, String> {
        Err("not supported on this system".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn computes_targets() {
        assert_eq!(target(BrightnessChange::By(10), 0, 50, 100), 60);
        assert_eq!(target(BrightnessChange::By(-10), 0, 5, 100), 0);
        assert_eq!(target(BrightnessChange::By(10), 0, 95, 100), 100);
        assert_eq!(target(BrightnessChange::To(50), 0, 10, 255), 127);
        assert_eq!(target(BrightnessChange::To(0), 10, 50, 110), 10);
    }
}
