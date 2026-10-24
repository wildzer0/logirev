//! Packet capture of the HID++ traffic, in the classic `.pcap` format.
//!
//! Every report we send or receive is stored as a USB *interrupt transfer*
//! using the **USBPcap** link type (the format produced by the USBPcap
//! capture driver on Windows). Wireshark understands it out of the box: it
//! shows direction (host → device / device → host), timing and the raw
//! report bytes. The optional Lua plugin in `tools/hidpp.lua` additionally
//! decodes the HID++ header (device index, feature, function, errors).
//!
//! The capture is size-limited: when the file would exceed the configured
//! maximum it is renamed to `<name>.1.pcap` (replacing any older backup) and a
//! fresh file is started, so disk usage never exceeds twice the limit.

use std::fs::{self, File};
use std::io::{self, BufWriter, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

/// `LINKTYPE_USBPCAP`, see <https://www.tcpdump.org/linktypes.html>.
const LINKTYPE_USBPCAP: u32 = 249;
/// Size of the USBPcap per-packet pseudo header.
const USBPCAP_HEADER_LEN: u16 = 27;
/// `URB_FUNCTION_BULK_OR_INTERRUPT_TRANSFER`.
const URB_FUNCTION_INTERRUPT: u16 = 0x0009;
/// `USBPCAP_TRANSFER_INTERRUPT`.
const TRANSFER_INTERRUPT: u8 = 1;
/// Size of the pcap global header.
const GLOBAL_HEADER_LEN: u64 = 24;
/// Size of the pcap per-record header.
const RECORD_HEADER_LEN: u64 = 16;

/// Which way a report travelled.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    /// Host → device (a request we wrote).
    Out,
    /// Device → host (a response or notification we read).
    In,
}

/// Writes HID++ reports to a size-capped `.pcap` file.
pub struct PcapWriter {
    path: PathBuf,
    max_size: u64,
    out: BufWriter<File>,
    written: u64,
    next_irp: u64,
}

impl PcapWriter {
    /// Creates (truncating) the capture file at `path`.
    pub fn create(path: &Path, max_size: u64) -> io::Result<Self> {
        let (out, written) = Self::open_fresh(path)?;
        Ok(Self {
            path: path.to_path_buf(),
            max_size: max_size.max(GLOBAL_HEADER_LEN + 1024),
            out,
            written,
            next_irp: 1,
        })
    }

    /// Appends one report. The HID++ `device_index` is stored as the USB
    /// device address, so each paired device gets its own "address" in
    /// Wireshark's source/destination columns and can be filtered on.
    pub fn write(&mut self, direction: Direction, device_index: u8, data: &[u8]) -> io::Result<()> {
        let captured = u64::from(USBPCAP_HEADER_LEN) + data.len() as u64;
        if self.written + RECORD_HEADER_LEN + captured > self.max_size {
            self.rotate()?;
        }

        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default();
        let captured = captured as u32;
        let mut rec = Vec::with_capacity(RECORD_HEADER_LEN as usize + captured as usize);
        // pcap record header: ts_sec, ts_usec, incl_len, orig_len.
        rec.extend_from_slice(&(now.as_secs() as u32).to_le_bytes());
        rec.extend_from_slice(&now.subsec_micros().to_le_bytes());
        rec.extend_from_slice(&captured.to_le_bytes());
        rec.extend_from_slice(&captured.to_le_bytes());

        // USBPcap pseudo header (all little endian).
        let (info, endpoint) = match direction {
            Direction::Out => (0u8, 0x02u8), // FDO -> PDO, OUT endpoint 2
            Direction::In => (1u8, 0x83u8),  // PDO -> FDO, IN endpoint 3
        };
        rec.extend_from_slice(&USBPCAP_HEADER_LEN.to_le_bytes());
        rec.extend_from_slice(&self.next_irp.to_le_bytes());
        rec.extend_from_slice(&0u32.to_le_bytes()); // USBD_STATUS_SUCCESS
        rec.extend_from_slice(&URB_FUNCTION_INTERRUPT.to_le_bytes());
        rec.push(info);
        rec.extend_from_slice(&1u16.to_le_bytes()); // bus
        rec.extend_from_slice(&u16::from(device_index).to_le_bytes()); // device address
        rec.push(endpoint);
        rec.push(TRANSFER_INTERRUPT);
        rec.extend_from_slice(&(data.len() as u32).to_le_bytes());
        rec.extend_from_slice(data);

        self.next_irp = self.next_irp.wrapping_add(1);
        self.out.write_all(&rec)?;
        // Flush per record: captures are small and must survive a crash.
        self.out.flush()?;
        self.written += rec.len() as u64;
        Ok(())
    }

    fn rotate(&mut self) -> io::Result<()> {
        self.out.flush()?;
        let backup = backup_path(&self.path, "pcap");
        // Ignore errors: the backup may not exist yet.
        let _ = fs::remove_file(&backup);
        fs::rename(&self.path, &backup)?;
        let (out, written) = Self::open_fresh(&self.path)?;
        self.out = out;
        self.written = written;
        Ok(())
    }

    fn open_fresh(path: &Path) -> io::Result<(BufWriter<File>, u64)> {
        let mut out = BufWriter::new(File::create(path)?);
        // Global header: magic, version 2.4, thiszone, sigfigs, snaplen, linktype.
        out.write_all(&0xA1B2_C3D4u32.to_le_bytes())?;
        out.write_all(&2u16.to_le_bytes())?;
        out.write_all(&4u16.to_le_bytes())?;
        out.write_all(&0i32.to_le_bytes())?;
        out.write_all(&0u32.to_le_bytes())?;
        out.write_all(&65_535u32.to_le_bytes())?;
        out.write_all(&LINKTYPE_USBPCAP.to_le_bytes())?;
        out.flush()?;
        Ok((out, GLOBAL_HEADER_LEN))
    }
}

/// `dir/name.ext` → `dir/name.1.ext` (used by both the log and the capture).
pub fn backup_path(path: &Path, default_ext: &str) -> PathBuf {
    let stem = path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let ext = path
        .extension()
        .map(|e| e.to_string_lossy().into_owned())
        .unwrap_or_else(|| default_ext.to_owned());
    path.with_file_name(format!("{stem}.1.{ext}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_valid_headers_and_rotates() {
        let dir = std::env::temp_dir().join(format!("logirev-pcap-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("t.pcap");
        let mut w = PcapWriter::create(&path, 2048).unwrap();
        let frame = [0x10u8, 0xFF, 0x81, 0x02, 0, 0, 0];
        w.write(Direction::Out, 0xFF, &frame).unwrap();

        let bytes = fs::read(&path).unwrap();
        assert_eq!(&bytes[..4], &0xA1B2_C3D4u32.to_le_bytes());
        assert_eq!(&bytes[20..24], &LINKTYPE_USBPCAP.to_le_bytes());
        let rec_len = u32::from_le_bytes(bytes[32..36].try_into().unwrap());
        assert_eq!(rec_len as usize, 27 + frame.len());
        assert_eq!(&bytes[bytes.len() - frame.len()..], &frame);

        for _ in 0..100 {
            w.write(Direction::In, 1, &frame).unwrap();
        }
        assert!(fs::metadata(&path).unwrap().len() <= 2048);
        assert!(backup_path(&path, "pcap").exists());
        fs::remove_dir_all(&dir).ok();
    }
}
