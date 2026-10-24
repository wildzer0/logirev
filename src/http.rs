//! Minimal HTTPS downloads, used only by the update checker.
//!
//! * **Windows** — WinHTTP, the HTTP stack built into Windows: it uses the
//!   system's certificates and proxy settings and needs no extra library.
//! * **Linux** — the `curl` command, present on practically every
//!   distribution.

/// Program name sent to the server (GitHub requires one).
const AGENT: &str = concat!("logirev/", env!("CARGO_PKG_VERSION"));

/// Downloads `url` (HTTPS only, redirects followed) and returns the body.
pub fn get(url: &str) -> Result<Vec<u8>, String> {
    imp::get(url)
}

/// Splits `https://host/path?query` into `(host, "/path?query")`.
fn split_https(url: &str) -> Result<(&str, &str), String> {
    let rest = url
        .strip_prefix("https://")
        .ok_or_else(|| format!("not an https address: {url}"))?;
    Ok(match rest.find('/') {
        Some(i) => (&rest[..i], &rest[i..]),
        None => (rest, "/"),
    })
}

#[cfg(windows)]
mod imp {
    use windows_sys::Win32::Networking::WinHttp::{
        WinHttpCloseHandle, WinHttpConnect, WinHttpOpen, WinHttpOpenRequest,
        WinHttpQueryDataAvailable, WinHttpQueryHeaders, WinHttpReadData, WinHttpReceiveResponse,
        WinHttpSendRequest, WinHttpSetTimeouts, INTERNET_DEFAULT_HTTPS_PORT,
        WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY, WINHTTP_FLAG_SECURE, WINHTTP_QUERY_FLAG_NUMBER,
        WINHTTP_QUERY_STATUS_CODE,
    };

    use super::{split_https, AGENT};

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }

    /// Closes a WinHTTP handle when dropped.
    struct Handle(*mut core::ffi::c_void);

    impl Drop for Handle {
        fn drop(&mut self) {
            if !self.0.is_null() {
                // SAFETY: the handle came from WinHTTP and is closed once.
                unsafe { WinHttpCloseHandle(self.0) };
            }
        }
    }

    fn check(h: *mut core::ffi::c_void, what: &str) -> Result<Handle, String> {
        if h.is_null() {
            Err(format!(
                "{what} failed ({})",
                std::io::Error::last_os_error()
            ))
        } else {
            Ok(Handle(h))
        }
    }

    pub fn get(url: &str) -> Result<Vec<u8>, String> {
        let (host, path) = split_https(url)?;
        let (agent, host_w, path_w, verb) = (wide(AGENT), wide(host), wide(path), wide("GET"));
        // SAFETY: standard WinHTTP sequence; all strings are NUL-terminated
        // and outlive the calls; handles are closed by `Handle`.
        unsafe {
            let session = check(
                WinHttpOpen(
                    agent.as_ptr(),
                    WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY,
                    std::ptr::null(),
                    std::ptr::null(),
                    0,
                ),
                "opening the connection",
            )?;
            WinHttpSetTimeouts(session.0, 10_000, 10_000, 30_000, 60_000);
            let connect = check(
                WinHttpConnect(session.0, host_w.as_ptr(), INTERNET_DEFAULT_HTTPS_PORT, 0),
                "connecting",
            )?;
            let request = check(
                WinHttpOpenRequest(
                    connect.0,
                    verb.as_ptr(),
                    path_w.as_ptr(),
                    std::ptr::null(),
                    std::ptr::null(),
                    std::ptr::null(),
                    WINHTTP_FLAG_SECURE,
                ),
                "preparing the request",
            )?;
            if WinHttpSendRequest(request.0, std::ptr::null(), 0, std::ptr::null(), 0, 0, 0) == 0
                || WinHttpReceiveResponse(request.0, std::ptr::null_mut()) == 0
            {
                return Err(format!(
                    "no answer from {host} ({})",
                    std::io::Error::last_os_error()
                ));
            }
            let (mut status, mut size) = (0u32, 4u32);
            WinHttpQueryHeaders(
                request.0,
                WINHTTP_QUERY_STATUS_CODE | WINHTTP_QUERY_FLAG_NUMBER,
                std::ptr::null(),
                (&mut status as *mut u32).cast(),
                &mut size,
                std::ptr::null_mut(),
            );
            if status != 200 {
                return Err(format!("{host} answered with HTTP status {status}"));
            }
            let mut body = Vec::new();
            loop {
                let mut available = 0u32;
                if WinHttpQueryDataAvailable(request.0, &mut available) == 0 {
                    return Err("download interrupted".into());
                }
                if available == 0 {
                    break;
                }
                let start = body.len();
                body.resize(start + available as usize, 0);
                let mut read = 0u32;
                if WinHttpReadData(
                    request.0,
                    body[start..].as_mut_ptr().cast(),
                    available,
                    &mut read,
                ) == 0
                {
                    return Err("download interrupted".into());
                }
                body.truncate(start + read as usize);
            }
            Ok(body)
        }
    }
}

#[cfg(not(windows))]
mod imp {
    use super::{split_https, AGENT};

    pub fn get(url: &str) -> Result<Vec<u8>, String> {
        split_https(url)?;
        let out = std::process::Command::new("curl")
            .args([
                "--fail",
                "--silent",
                "--show-error",
                "--location",
                "--max-time",
                "120",
                "--user-agent",
                AGENT,
                url,
            ])
            .output()
            .map_err(|e| format!("cannot run curl: {e}"))?;
        if out.status.success() {
            Ok(out.stdout)
        } else {
            Err(String::from_utf8_lossy(&out.stderr).trim().to_owned())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_urls() {
        assert_eq!(
            split_https("https://api.github.com/repos/a/b").unwrap(),
            ("api.github.com", "/repos/a/b")
        );
        assert_eq!(
            split_https("https://example.com").unwrap(),
            ("example.com", "/")
        );
        assert!(split_https("http://insecure.example").is_err());
    }
}
