use super::error;
use crate::Result;
use serde_json::{json, Value};
use std::{
    io::{Read, Write},
    net::{SocketAddr, TcpStream},
    time::Instant,
};

// The ADB host service returns the same serial, transport state and attributes as adb devices -l.
// It never sends a device command, authorizes a device or selects a default serial.
pub(super) fn inventory(address: SocketAddr, deadline: Instant) -> Result<Value> {
    let remaining = || {
        deadline
            .checked_duration_since(Instant::now())
            .ok_or_else(|| {
                std::io::Error::new(std::io::ErrorKind::TimedOut, "ADB observation timed out")
            })
    };
    let query = || -> std::io::Result<Vec<u8>> {
        let mut socket = TcpStream::connect_timeout(&address, remaining()?)?;
        socket.set_write_timeout(Some(remaining()?))?;
        socket.write_all(b"000ehost:devices-l")?;
        let mut read = |buf: &mut [u8]| -> std::io::Result<()> {
            let mut offset = 0;
            while offset < buf.len() {
                socket.set_read_timeout(Some(remaining()?))?;
                let n = socket.read(&mut buf[offset..])?;
                if n == 0 {
                    return Err(std::io::ErrorKind::UnexpectedEof.into());
                }
                offset += n;
            }
            Ok(())
        };
        let mut header = [0; 4];
        read(&mut header)?;
        if &header != b"OKAY" {
            return Err(std::io::Error::other("ADB refused device enumeration"));
        }
        read(&mut header)?;
        let length = std::str::from_utf8(&header)
            .ok()
            .and_then(|s| usize::from_str_radix(s, 16).ok())
            .ok_or_else(|| std::io::Error::other("Invalid ADB reply length"))?;
        let mut bytes = vec![0; length];
        read(&mut bytes)?;
        Ok(bytes)
    };
    let bytes = query().map_err(|e| {
        error(format!(
            "Cannot list the runner's devices. Check the local ADB server: {e}"
        ))
    })?;
    let list = std::str::from_utf8(&bytes).map_err(|_| error("Invalid ADB inventory"))?;
    let mut devices = vec![];
    for line in list.lines().filter(|l| !l.trim().is_empty()) {
        let mut parts = line.split_whitespace();
        let (Some(serial), Some(state)) = (parts.next(), parts.next()) else {
            return Err(error("Incomplete ADB device record"));
        };
        let attributes: std::collections::BTreeMap<_, _> =
            parts.filter_map(|s| s.split_once(':')).collect();
        devices.push(json!({"serial":serial,"state":state,"attributes":attributes}));
    }
    Ok(Value::Array(devices))
}
