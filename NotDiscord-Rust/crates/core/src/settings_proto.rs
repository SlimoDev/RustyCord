//! Focused protobuf compatibility for PreloadedUserSettings field 11.
//! Unknown fields are skipped; unrelated account settings are never rewritten.
use crate::api::ServiceError;
use base64::{Engine, engine::general_purpose::STANDARD};

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct StatusValues {
    pub status: Option<String>,
    pub show_current_game: Option<bool>,
}
#[derive(Debug)]
struct Field<'a> {
    number: u64,
    wire: u8,
    data: &'a [u8],
    scalar: u64,
}
fn invalid() -> ServiceError {
    ServiceError::new("Ajustes protobuf no válidos.")
}
fn varint(bytes: &[u8], cursor: &mut usize) -> Result<u64, ServiceError> {
    let mut n = 0;
    for shift in (0..70).step_by(7) {
        let part = *bytes.get(*cursor).ok_or_else(invalid)?;
        *cursor += 1;
        if shift == 63 && part > 1 {
            return Err(invalid());
        }
        n |= u64::from(part & 127) << shift;
        if part & 128 == 0 {
            return Ok(n);
        }
    }
    Err(invalid())
}
fn fields(mut bytes: &[u8]) -> Result<Vec<Field<'_>>, ServiceError> {
    let mut out = Vec::new();
    while !bytes.is_empty() {
        if out.len() >= 10000 {
            return Err(invalid());
        }
        let mut cursor = 0;
        let tag = varint(bytes, &mut cursor)?;
        let number = tag >> 3;
        let wire = (tag & 7) as u8;
        if number == 0 {
            return Err(invalid());
        }
        let mut scalar = 0;
        let length = match wire {
            0 => {
                scalar = varint(bytes, &mut cursor)?;
                0
            }
            1 => 8,
            5 => 4,
            2 => {
                let n = varint(bytes, &mut cursor)?;
                if n > 512 * 1024 {
                    return Err(invalid());
                }
                n as usize
            }
            _ => return Err(invalid()),
        };
        if length > bytes.len() - cursor {
            return Err(invalid());
        }
        out.push(Field {
            number,
            wire,
            data: if wire == 2 {
                &bytes[cursor..cursor + length]
            } else {
                &[]
            },
            scalar,
        });
        bytes = &bytes[cursor + length..]
    }
    Ok(out)
}
pub fn read(encoded: &str, full_settings: bool) -> Result<StatusValues, ServiceError> {
    if encoded.len() > 700000 {
        return Err(ServiceError::new(
            "Los ajustes superan el límite permitido.",
        ));
    }
    let bytes = STANDARD.decode(encoded).map_err(|_| invalid())?;
    if bytes.len() > 512 * 1024 {
        return Err(invalid());
    }
    let mut values = StatusValues {
        status: None,
        show_current_game: full_settings.then_some(true),
    };
    for field in fields(&bytes)? {
        if field.number != 11 || field.wire != 2 {
            continue;
        }
        for setting in fields(field.data)? {
            if setting.number == 1 && setting.wire == 2 {
                for value in fields(setting.data)? {
                    if value.number == 1 && value.wire == 2 {
                        let selected = std::str::from_utf8(value.data).map_err(|_| invalid())?;
                        if valid_presence(selected) {
                            values.status = Some(selected.into())
                        }
                    }
                }
            }
            if setting.number == 3 && setting.wire == 2 {
                values.show_current_game = Some(false);
                for value in fields(setting.data)? {
                    if value.number == 1 && value.wire == 0 {
                        values.show_current_game = Some(value.scalar != 0)
                    }
                }
            }
        }
    }
    Ok(values)
}
pub fn valid_presence(status: &str) -> bool {
    matches!(status, "online" | "idle" | "dnd" | "invisible")
}
pub fn visibility_delta(enabled: bool) -> String {
    STANDARD.encode(if enabled {
        &[0x5a, 4, 0x1a, 2, 8, 1][..]
    } else {
        &[0x5a, 2, 0x1a, 0][..]
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn delta_exact() {
        assert_eq!(
            STANDARD.decode(visibility_delta(true)).unwrap(),
            [0x5a, 4, 0x1a, 2, 8, 1]
        );
        assert_eq!(
            STANDARD.decode(visibility_delta(false)).unwrap(),
            [0x5a, 2, 0x1a, 0]
        )
    }
    #[test]
    fn empty_bool_means_false() {
        assert_eq!(
            read(&visibility_delta(false), false)
                .unwrap()
                .show_current_game,
            Some(false)
        )
    }
    #[test]
    fn absent_full_and_partial_differ() {
        assert_eq!(read("", true).unwrap().show_current_game, Some(true));
        assert_eq!(read("", false).unwrap().show_current_game, None)
    }
    #[test]
    fn status_wrapper() {
        let bytes = [
            0x5a, 10, 0x0a, 8, 0x0a, 6, b'o', b'n', b'l', b'i', b'n', b'e',
        ];
        assert_eq!(
            read(&STANDARD.encode(bytes), false)
                .unwrap()
                .status
                .as_deref(),
            Some("online")
        )
    }
    #[test]
    fn unknown_skipped() {
        assert_eq!(
            read(&STANDARD.encode([0x08, 7]), false).unwrap(),
            StatusValues::default()
        )
    }
    #[test]
    fn malformed_and_excessive_rejected() {
        for bytes in [vec![0], vec![0x5a, 5], vec![0x08, 0xff], vec![0x0b]] {
            assert!(read(&STANDARD.encode(bytes), false).is_err())
        }
        assert!(read(&"a".repeat(700001), false).is_err())
    }
}
