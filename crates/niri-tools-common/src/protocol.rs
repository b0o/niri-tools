use std::io::{Read, Write};

use serde::{de::DeserializeOwned, Deserialize, Serialize};

use crate::error::{NiriToolsError, Result};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum Command {
    Toggle { name: Option<String> },
    Hide,
    ToggleFloat { name: Option<String> },
    Float { name: Option<String> },
    Tile { name: Option<String> },
    DaemonStop,
    DaemonRestart,
    DaemonStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum Response {
    Ok,
    Status {
        pid: u32,
        cmdline: String,
        ppid: u32,
        parent_cmdline: String,
        socket: String,
    },
    Error(String),
}

/// Maximum message size (16 MiB). Protects against malicious/corrupted length prefixes.
const MAX_MESSAGE_SIZE: u32 = 16 * 1024 * 1024;

/// Serializes a message with bincode and prepends a 4-byte little-endian length prefix.
pub fn encode_message<T: Serialize>(msg: &T) -> Result<Vec<u8>> {
    let payload = bincode::serde::encode_to_vec(msg, bincode::config::standard())
        .map_err(|e| NiriToolsError::Serialization(e.to_string()))?;
    let len: u32 = payload
        .len()
        .try_into()
        .map_err(|_| NiriToolsError::Serialization("payload too large".to_string()))?;
    let mut buf = Vec::with_capacity(4 + payload.len());
    buf.extend_from_slice(&len.to_le_bytes());
    buf.extend_from_slice(&payload);
    Ok(buf)
}

/// Reads a 4-byte LE length prefix, then deserializes the bincode payload from the buffer.
pub fn decode_message<T: DeserializeOwned>(buf: &[u8]) -> Result<T> {
    if buf.len() < 4 {
        return Err(NiriToolsError::Serialization(
            "buffer too short for length prefix".to_string(),
        ));
    }
    let len = u32::from_le_bytes([buf[0], buf[1], buf[2], buf[3]]) as usize;
    let payload = &buf[4..];
    if payload.len() < len {
        return Err(NiriToolsError::Serialization(format!(
            "buffer too short: expected {} bytes, got {}",
            len,
            payload.len()
        )));
    }
    let (msg, _) = bincode::serde::decode_from_slice(&payload[..len], bincode::config::standard())
        .map_err(|e| NiriToolsError::Serialization(e.to_string()))?;
    Ok(msg)
}

/// Reads a length-prefixed message from a reader.
pub fn read_message<T: DeserializeOwned>(reader: &mut impl Read) -> Result<T> {
    let mut len_buf = [0u8; 4];
    reader.read_exact(&mut len_buf)?;
    let len = u32::from_le_bytes(len_buf);
    if len > MAX_MESSAGE_SIZE {
        return Err(NiriToolsError::Serialization(format!(
            "message size {len} exceeds maximum {MAX_MESSAGE_SIZE}"
        )));
    }
    let mut payload = vec![0u8; len as usize];
    reader.read_exact(&mut payload)?;
    let (msg, _) = bincode::serde::decode_from_slice(&payload, bincode::config::standard())
        .map_err(|e| NiriToolsError::Serialization(e.to_string()))?;
    Ok(msg)
}

/// Writes a length-prefixed message to a writer.
pub fn write_message<T: Serialize>(writer: &mut impl Write, msg: &T) -> Result<()> {
    let payload = bincode::serde::encode_to_vec(msg, bincode::config::standard())
        .map_err(|e| NiriToolsError::Serialization(e.to_string()))?;
    let len: u32 = payload
        .len()
        .try_into()
        .map_err(|_| NiriToolsError::Serialization("payload too large".to_string()))?;
    writer.write_all(&len.to_le_bytes())?;
    writer.write_all(&payload)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    // -- Command round-trip serialization tests --

    #[test]
    fn command_toggle_with_name_roundtrip() {
        let cmd = Command::Toggle {
            name: Some("term".to_string()),
        };
        let encoded = encode_message(&cmd).unwrap();
        let decoded: Command = decode_message(&encoded).unwrap();
        assert_eq!(cmd, decoded);
    }

    #[test]
    fn command_toggle_without_name_roundtrip() {
        let cmd = Command::Toggle { name: None };
        let encoded = encode_message(&cmd).unwrap();
        let decoded: Command = decode_message(&encoded).unwrap();
        assert_eq!(cmd, decoded);
    }

    #[test]
    fn command_hide_roundtrip() {
        let cmd = Command::Hide;
        let encoded = encode_message(&cmd).unwrap();
        let decoded: Command = decode_message(&encoded).unwrap();
        assert_eq!(cmd, decoded);
    }

    #[test]
    fn command_toggle_float_roundtrip() {
        let cmd = Command::ToggleFloat {
            name: Some("browser".to_string()),
        };
        let encoded = encode_message(&cmd).unwrap();
        let decoded: Command = decode_message(&encoded).unwrap();
        assert_eq!(cmd, decoded);
    }

    #[test]
    fn command_float_roundtrip() {
        let cmd = Command::Float { name: None };
        let encoded = encode_message(&cmd).unwrap();
        let decoded: Command = decode_message(&encoded).unwrap();
        assert_eq!(cmd, decoded);
    }

    #[test]
    fn command_tile_roundtrip() {
        let cmd = Command::Tile {
            name: Some("editor".to_string()),
        };
        let encoded = encode_message(&cmd).unwrap();
        let decoded: Command = decode_message(&encoded).unwrap();
        assert_eq!(cmd, decoded);
    }

    #[test]
    fn command_daemon_stop_roundtrip() {
        let cmd = Command::DaemonStop;
        let encoded = encode_message(&cmd).unwrap();
        let decoded: Command = decode_message(&encoded).unwrap();
        assert_eq!(cmd, decoded);
    }

    #[test]
    fn command_daemon_restart_roundtrip() {
        let cmd = Command::DaemonRestart;
        let encoded = encode_message(&cmd).unwrap();
        let decoded: Command = decode_message(&encoded).unwrap();
        assert_eq!(cmd, decoded);
    }

    #[test]
    fn command_daemon_status_roundtrip() {
        let cmd = Command::DaemonStatus;
        let encoded = encode_message(&cmd).unwrap();
        let decoded: Command = decode_message(&encoded).unwrap();
        assert_eq!(cmd, decoded);
    }

    // -- Response round-trip serialization tests --

    #[test]
    fn response_ok_roundtrip() {
        let resp = Response::Ok;
        let encoded = encode_message(&resp).unwrap();
        let decoded: Response = decode_message(&encoded).unwrap();
        assert_eq!(resp, decoded);
    }

    #[test]
    fn response_status_roundtrip() {
        let resp = Response::Status {
            pid: 1234,
            cmdline: "niri-tools-daemon".to_string(),
            ppid: 1,
            parent_cmdline: "systemd".to_string(),
            socket: "/run/user/1000/niri-tools.sock".to_string(),
        };
        let encoded = encode_message(&resp).unwrap();
        let decoded: Response = decode_message(&encoded).unwrap();
        assert_eq!(resp, decoded);
    }

    #[test]
    fn response_error_roundtrip() {
        let resp = Response::Error("something went wrong".to_string());
        let encoded = encode_message(&resp).unwrap();
        let decoded: Response = decode_message(&encoded).unwrap();
        assert_eq!(resp, decoded);
    }

    // -- Wire format encode/decode tests --

    #[test]
    fn encode_message_has_length_prefix() {
        let cmd = Command::Hide;
        let encoded = encode_message(&cmd).unwrap();
        // First 4 bytes are LE length
        let len = u32::from_le_bytes([encoded[0], encoded[1], encoded[2], encoded[3]]) as usize;
        assert_eq!(encoded.len(), 4 + len);
    }

    #[test]
    fn decode_message_rejects_short_buffer() {
        let result: std::result::Result<Command, _> = decode_message(&[0, 1]);
        assert!(result.is_err());
    }

    #[test]
    fn decode_message_rejects_truncated_payload() {
        let cmd = Command::Hide;
        let mut encoded = encode_message(&cmd).unwrap();
        // Truncate the payload
        encoded.truncate(5);
        // Set length to something larger than remaining payload
        let fake_len: u32 = 100;
        encoded[0..4].copy_from_slice(&fake_len.to_le_bytes());
        let result: std::result::Result<Command, _> = decode_message(&encoded);
        assert!(result.is_err());
    }

    // -- Reader/Writer round-trip tests --

    #[test]
    fn write_and_read_message_roundtrip() {
        let cmd = Command::Toggle {
            name: Some("test".to_string()),
        };
        let mut buf = Vec::new();
        write_message(&mut buf, &cmd).unwrap();

        let mut cursor = Cursor::new(&buf);
        let decoded: Command = read_message(&mut cursor).unwrap();
        assert_eq!(cmd, decoded);
    }

    #[test]
    fn write_and_read_multiple_messages() {
        let cmd1 = Command::DaemonStatus;
        let resp1 = Response::Status {
            pid: 42,
            cmdline: "daemon".to_string(),
            ppid: 1,
            parent_cmdline: "init".to_string(),
            socket: "/tmp/test.sock".to_string(),
        };

        let mut buf = Vec::new();
        write_message(&mut buf, &cmd1).unwrap();
        write_message(&mut buf, &resp1).unwrap();

        let mut cursor = Cursor::new(&buf);
        let decoded_cmd: Command = read_message(&mut cursor).unwrap();
        let decoded_resp: Response = read_message(&mut cursor).unwrap();
        assert_eq!(cmd1, decoded_cmd);
        assert_eq!(resp1, decoded_resp);
    }

    #[test]
    fn read_message_from_empty_reader_returns_error() {
        let mut cursor = Cursor::new(Vec::<u8>::new());
        let result: std::result::Result<Command, _> = read_message(&mut cursor);
        assert!(result.is_err());
    }
}
