//! Field-preserving transformation of the favoriteGifs subtree only.
//! Contract: public Discord web asset, documented in docs/gif-060-evidence.md.
use base64::Engine;
const MAX: usize = 1024 * 1024;
struct Field<'a> {
    number: u64,
    wire: u8,
    data: &'a [u8],
    raw: &'a [u8],
    scalar: u64,
}
fn varint(bytes: &[u8], cursor: &mut usize) -> Result<u64, String> {
    let mut value = 0;
    for shift in (0..70).step_by(7) {
        let byte = *bytes.get(*cursor).ok_or("Protobuf truncado")?;
        *cursor += 1;
        if shift == 63 && byte > 1 {
            return Err("Varint inválido".into());
        }
        value |= ((byte & 127) as u64) << shift;
        if byte & 128 == 0 {
            return Ok(value);
        }
    }
    Err("Varint inválido".into())
}
fn fields(bytes: &[u8]) -> Result<Vec<Field<'_>>, String> {
    let mut out = Vec::new();
    let mut cursor = 0;
    while cursor < bytes.len() {
        if out.len() >= 6000 {
            return Err("Demasiados campos de ajustes".into());
        }
        let start = cursor;
        let tag = varint(bytes, &mut cursor)?;
        let number = tag >> 3;
        let wire = (tag & 7) as u8;
        if number == 0 {
            return Err("Campo de ajustes inválido".into());
        }
        let scalar = if wire == 0 {
            varint(bytes, &mut cursor)?
        } else {
            0
        };
        let size = match wire {
            0 => 0,
            1 => 8,
            5 => 4,
            2 => usize::try_from(varint(bytes, &mut cursor)?)
                .map_err(|_| "Campo demasiado grande")?,
            _ => return Err("Tipo protobuf no compatible".into()),
        };
        let end = cursor
            .checked_add(size)
            .filter(|end| *end <= bytes.len())
            .ok_or("Protobuf truncado")?;
        out.push(Field {
            number,
            wire,
            data: &bytes[cursor..end],
            raw: &bytes[start..end],
            scalar,
        });
        cursor = end;
    }
    Ok(out)
}
fn decode(encoded: &str) -> Result<Vec<u8>, String> {
    if encoded.len() > 1_400_000 {
        return Err("Ajustes GIF demasiado grandes".into());
    }
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(encoded)
        .map_err(|_| "Ajustes GIF inválidos")?;
    if bytes.len() > MAX {
        return Err("Ajustes GIF demasiado grandes".into());
    }
    Ok(bytes)
}
fn append_length(out: &mut Vec<u8>, mut length: usize) {
    while length >= 128 {
        out.push((length as u8 & 127) | 128);
        length >>= 7;
    }
    out.push(length as u8);
}
fn favorite_subtree<'a>(root: &'a [Field<'a>]) -> Result<Option<&'a [u8]>, String> {
    let values = root
        .iter()
        .filter(|field| field.number == 2)
        .collect::<Vec<_>>();
    if values.len() > 1 || values.first().is_some_and(|field| field.wire != 2) {
        return Err("Subárbol GIF ambiguo; no se modificaron ajustes".into());
    }
    Ok(values.first().map(|field| field.data))
}
fn map_key(entry: &[u8]) -> Result<String, String> {
    let values = fields(entry)?;
    let keys = values
        .iter()
        .filter(|field| field.number == 1)
        .collect::<Vec<_>>();
    if keys.len() != 1 || keys[0].wire != 2 {
        return Err("Clave GIF ambigua; no se modificaron ajustes".into());
    }
    String::from_utf8(keys[0].data.to_vec()).map_err(|_| "Clave GIF inválida".into())
}
pub struct RemovalPatch {
    pub encoded: String,
    pub data_version: u64,
    pub changed: bool,
}
pub fn has_favorites(encoded: &str) -> Result<bool, String> {
    let bytes = decode(encoded)?;
    let root = fields(&bytes)?;
    Ok(favorite_subtree(&root)?.is_some())
}
pub fn contains(encoded: &str, url: &str) -> Result<bool, String> {
    let bytes = decode(encoded)?;
    let root = fields(&bytes)?;
    let Some(subtree) = favorite_subtree(&root)? else {
        return Ok(false);
    };
    let mut found = false;
    for entry in fields(subtree)? {
        if entry.number == 1 {
            if entry.wire != 2 {
                return Err("Mapa GIF inválido".into());
            }
            found |= map_key(entry.data)? == url;
        }
    }
    Ok(found)
}
pub fn remove(encoded: &str, url: &str) -> Result<RemovalPatch, String> {
    let bytes = decode(encoded)?;
    let root = fields(&bytes)?;
    let versions = root
        .iter()
        .filter(|field| field.number == 1)
        .collect::<Vec<_>>();
    if versions.len() != 1 || versions[0].wire != 2 {
        return Err("No se confirmó la revisión de ajustes; no se modificó Discord".into());
    }
    let version_fields = fields(versions[0].data)?;
    let version = version_fields
        .iter()
        .filter(|field| field.number == 3)
        .collect::<Vec<_>>();
    if version.len() > 1 || version.first().is_some_and(|field| field.wire != 0) {
        return Err("Revisión de ajustes ambigua".into());
    }
    let data_version = version.first().map_or(0, |field| field.scalar);
    if data_version > u32::MAX as u64 {
        return Err("Revisión de ajustes inválida".into());
    }
    let Some(subtree) = favorite_subtree(&root)? else {
        return Ok(RemovalPatch {
            encoded: String::new(),
            data_version,
            changed: false,
        });
    };
    let mut updated = Vec::new();
    let mut changed = false;
    for field in fields(subtree)? {
        if field.number == 1 {
            if field.wire != 2 {
                return Err("Mapa GIF inválido".into());
            }
            if map_key(field.data)? == url {
                changed = true;
                continue;
            }
        }
        updated.extend_from_slice(field.raw);
    }
    // A partial root contains only field2, with every untouched map entry,
    // hideTooltip and unknown subtree byte preserved. Other top-level settings
    // are omitted rather than rewritten from a filtered presentation model.
    let mut partial = vec![0x12];
    append_length(&mut partial, updated.len());
    partial.extend(updated);
    Ok(RemovalPatch {
        encoded: base64::engine::general_purpose::STANDARD.encode(partial),
        data_version,
        changed,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn field(number: u8, data: &[u8]) -> Vec<u8> {
        let mut bytes = vec![number << 3 | 2];
        append_length(&mut bytes, data.len());
        bytes.extend_from_slice(data);
        bytes
    }
    fn entry(url: &str, metadata: &[u8]) -> Vec<u8> {
        let mut map = field(1, url.as_bytes());
        map.extend(field(2, metadata));
        field(1, &map)
    }
    fn encoded(favorites: &[u8]) -> String {
        let mut root = field(1, &[0x18, 7]);
        root.extend(field(2, favorites));
        root.extend(field(12, &[0x08, 99]));
        base64::engine::general_purpose::STANDARD.encode(root)
    }
    #[test]
    fn removes_only_exact_map_key_and_preserves_opaque_subtree_bytes() {
        let retained = entry("https://tenor.com/view/retained", &[0x08, 2, 0x98, 6, 7]);
        let mut favorites = entry("https://tenor.com/view/remove", &[0x08, 1]);
        favorites.extend(&retained);
        favorites.extend([0x10, 1, 0x98, 6, 123]);
        let original = encoded(&favorites);
        let patch = remove(&original, "https://tenor.com/view/remove").unwrap();
        assert_eq!(patch.data_version, 7);
        assert!(patch.changed);
        let partial = decode(&patch.encoded).unwrap();
        let root = fields(&partial).unwrap();
        assert_eq!(root.len(), 1);
        assert_eq!(root[0].number, 2);
        let mut expected = retained;
        expected.extend([0x10, 1, 0x98, 6, 123]);
        assert_eq!(root[0].data, expected);
        assert!(contains(&original, "https://tenor.com/view/remove").unwrap());
        assert!(!contains(&patch.encoded, "https://tenor.com/view/remove").unwrap());
    }
    #[test]
    fn an_empty_subtree_is_explicit_and_absent_intent_is_not_written() {
        let original = encoded(&entry("https://tenor.com/view/only", &[]));
        let patch = remove(&original, "https://tenor.com/view/only").unwrap();
        assert!(patch.changed);
        assert_eq!(decode(&patch.encoded).unwrap(), [0x12, 0]);
        assert!(
            !remove(&original, "https://tenor.com/view/other")
                .unwrap()
                .changed
        );
    }
    #[test]
    fn unknown_or_ambiguous_revision_fails_closed() {
        let without = base64::engine::general_purpose::STANDARD.encode(field(2, &[]));
        assert!(remove(&without, "https://tenor.com/view/x").is_err());
        let mut duplicated = field(1, &[0x18, 7, 0x18, 8]);
        duplicated.extend(field(2, &[]));
        assert!(
            remove(
                &base64::engine::general_purpose::STANDARD.encode(duplicated),
                "https://tenor.com/view/x"
            )
            .is_err()
        );
    }
}
