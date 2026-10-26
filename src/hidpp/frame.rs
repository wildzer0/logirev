//! HID++ wire format: report sizes, frame layout and error codes.
//!
//! Every HID++ message is a HID *output* or *input* report with this layout:
//!
//! ```text
//! byte 0   report id      0x10 = short (7 bytes), 0x11 = long (20 bytes)
//! byte 1   device index   0xFF = receiver itself / directly connected device,
//!                         1..=6 = device paired to a receiver slot
//! byte 2   HID++ 1.0: sub-id (register command)  | HID++ 2.0: feature index
//! byte 3   HID++ 1.0: register address           | HID++ 2.0: (function << 4) | software id
//! byte 4.. parameters (zero padded to the report size)
//! ```
//!
//! A response echoes bytes 1..=3 of the request, which is how requests and
//! responses are paired. Unsolicited *notifications* (button events,
//! connection changes) use software id `0`, so they can never be mistaken
//! for a response to one of our requests, which always use [`SOFTWARE_ID`].

use std::fmt;

/// Report id of a short HID++ report (7 bytes total).
pub const REPORT_SHORT: u8 = 0x10;
/// Report id of a long HID++ report (20 bytes total).
pub const REPORT_LONG: u8 = 0x11;

/// Device index addressing the receiver itself (or a device connected
/// directly over USB / Bluetooth, which then answers as index `0xFF`).
pub const RECEIVER_INDEX: u8 = 0xFF;

/// Software id placed in the low nibble of byte 3 of every request we send.
///
/// Any non-zero value works; a distinctive one keeps our traffic easy to spot
/// in a capture and avoids clashing with other HID++ software (Options+ uses
/// small values that change at runtime).
pub const SOFTWARE_ID: u8 = 0x0A;

/// HID++ 1.0 sub-id that marks an error response.
pub const SUB_ID_ERROR_10: u8 = 0x8F;
/// HID++ 2.0 feature index that marks an error response.
pub const FEATURE_INDEX_ERROR_20: u8 = 0xFF;

/// Total length (report id included) of a report with the given id.
pub fn report_len(report_id: u8) -> Option<usize> {
    match report_id {
        REPORT_SHORT => Some(7),
        REPORT_LONG => Some(20),
        0x12 => Some(64),
        _ => None,
    }
}

/// One complete HID++ report, as sent to or received from the hardware.
#[derive(Clone, PartialEq, Eq)]
pub struct Frame {
    bytes: Vec<u8>,
}

impl Frame {
    /// Builds a frame, padding (or truncating) `payload` to the report size.
    ///
    /// `payload` starts at byte 2 (sub-id / feature index).
    pub fn new(report_id: u8, device_index: u8, payload: &[u8]) -> Self {
        let len = report_len(report_id).expect("unsupported HID++ report id");
        debug_assert!(
            payload.len() <= len - 2,
            "HID++ payload of {} bytes does not fit report {report_id:#04x}",
            payload.len()
        );
        let mut bytes = Vec::with_capacity(len);
        bytes.push(report_id);
        bytes.push(device_index);
        bytes.extend_from_slice(payload);
        bytes.resize(len, 0);
        Self { bytes }
    }

    /// Wraps raw bytes read from the device. Returns `None` for reports that
    /// are not HID++ (unknown report id or too short to carry a header).
    pub fn from_raw(raw: &[u8]) -> Option<Self> {
        let expected = report_len(*raw.first()?)?;
        if raw.len() < 4 {
            return None;
        }
        let mut bytes = raw[..raw.len().min(expected)].to_vec();
        bytes.resize(expected, 0);
        Some(Self { bytes })
    }

    /// The whole report, report id included.
    pub fn as_bytes(&self) -> &[u8] {
        &self.bytes
    }

    /// Byte 0: [`REPORT_SHORT`] or [`REPORT_LONG`].
    pub fn report_id(&self) -> u8 {
        self.bytes[0]
    }

    /// Byte 1: receiver slot 1–6, or [`RECEIVER_INDEX`].
    pub fn device_index(&self) -> u8 {
        self.bytes[1]
    }

    /// Byte 2: HID++ 2.0 feature index or HID++ 1.0 sub-id.
    pub fn b2(&self) -> u8 {
        self.bytes[2]
    }

    /// Byte 3: HID++ 2.0 function/software id or HID++ 1.0 register address.
    pub fn b3(&self) -> u8 {
        self.bytes[3]
    }

    /// HID++ 2.0 function number (high nibble of byte 3).
    pub fn function(&self) -> u8 {
        self.bytes[3] >> 4
    }

    /// HID++ 2.0 software id (low nibble of byte 3); `0` for notifications.
    pub fn software_id(&self) -> u8 {
        self.bytes[3] & 0x0F
    }

    /// Parameter bytes (everything after the 4-byte header).
    pub fn params(&self) -> &[u8] {
        &self.bytes[4..]
    }

    /// If this frame is an error answer to a request whose bytes 2..=3 were
    /// `(b2, b3)`, returns the decoded error.
    ///
    /// Both error flavours are checked because a receiver may answer a
    /// HID++ 2.0 request with a HID++ 1.0 error (e.g. "unknown device" when
    /// the addressed slot is empty or asleep).
    pub fn error_for(&self, b2: u8, b3: u8) -> Option<HidppError> {
        let p = &self.bytes;
        if p.len() < 6 || p[3] != b2 || p[4] != b3 {
            return None;
        }
        match p[2] {
            FEATURE_INDEX_ERROR_20 => Some(HidppError::Protocol20(p[5])),
            SUB_ID_ERROR_10 => Some(HidppError::Protocol10(p[5])),
            _ => None,
        }
    }
}

impl fmt::Debug for Frame {
    /// Hex dump, e.g. `11 02 0A 1A 00 00 ...` (used by the trace log).
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (i, b) in self.bytes.iter().enumerate() {
            if i > 0 {
                f.write_str(" ")?;
            }
            write!(f, "{b:02X}")?;
        }
        Ok(())
    }
}

/// Error reported by the device or by the transport layer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HidppError {
    /// HID++ 1.0 error code (register access on a receiver).
    Protocol10(u8),
    /// HID++ 2.0 error code (feature call on a device).
    Protocol20(u8),
    /// The device did not answer within the allotted time.
    Timeout,
    /// The device does not implement the requested feature.
    FeatureNotSupported(u16),
    /// The answer was shorter than the layout we expect.
    MalformedResponse,
    /// The operating system refused the read or write.
    Io(String),
}

impl fmt::Display for HidppError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Protocol10(code) => {
                write!(f, "HID++ 1.0 error {code:#04x} ({})", error_name_10(*code))
            }
            Self::Protocol20(code) => {
                write!(f, "HID++ 2.0 error {code:#04x} ({})", error_name_20(*code))
            }
            Self::Timeout => f.write_str("no answer from the device (timeout)"),
            Self::FeatureNotSupported(id) => write!(f, "feature {id:#06x} not supported"),
            Self::MalformedResponse => f.write_str("malformed HID++ response"),
            Self::Io(msg) => write!(f, "HID I/O error: {msg}"),
        }
    }
}

impl std::error::Error for HidppError {}

/// Human-readable name of a HID++ 2.0 error code.
pub fn error_name_20(code: u8) -> &'static str {
    match code {
        0 => "NoError",
        1 => "Unknown",
        2 => "InvalidArgument",
        3 => "OutOfRange",
        4 => "HardwareError",
        5 => "NotAllowed",
        6 => "InvalidFeatureIndex",
        7 => "InvalidFunctionId",
        8 => "Busy",
        9 => "Unsupported",
        _ => "?",
    }
}

/// Human-readable name of a HID++ 1.0 error code.
pub fn error_name_10(code: u8) -> &'static str {
    match code {
        0 => "Success",
        1 => "InvalidSubId",
        2 => "InvalidAddress",
        3 => "InvalidValue",
        4 => "ConnectFail",
        5 => "TooManyDevices",
        6 => "AlreadyExists",
        7 => "Busy",
        8 => "UnknownDevice",
        9 => "ResourceError",
        10 => "RequestUnavailable",
        11 => "InvalidParamValue",
        12 => "WrongPinCode",
        _ => "?",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_pads_to_report_size() {
        let f = Frame::new(REPORT_SHORT, 1, &[0x00, 0x1A]);
        assert_eq!(f.as_bytes(), &[0x10, 0x01, 0x00, 0x1A, 0, 0, 0]);
        assert_eq!(Frame::new(REPORT_LONG, 1, &[]).as_bytes().len(), 20);
    }

    #[test]
    fn decodes_both_error_flavours() {
        let e20 = Frame::from_raw(&[0x11, 2, 0xFF, 0x05, 0x1A, 0x09, 0]).unwrap();
        assert_eq!(e20.error_for(0x05, 0x1A), Some(HidppError::Protocol20(9)));
        let e10 = Frame::from_raw(&[0x10, 3, 0x8F, 0x00, 0x1A, 0x08, 0]).unwrap();
        assert_eq!(e10.error_for(0x00, 0x1A), Some(HidppError::Protocol10(8)));
        assert_eq!(e10.error_for(0x00, 0x2A), None);
    }

    #[test]
    fn rejects_non_hidpp_reports() {
        assert!(Frame::from_raw(&[0x02, 0, 0, 0]).is_none());
        assert!(Frame::from_raw(&[0x10, 1]).is_none());
    }
}
