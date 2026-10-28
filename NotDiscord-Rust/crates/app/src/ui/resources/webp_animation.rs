//! Bounded animated WebP support for the existing GIF player.
use super::*;
pub(super) fn decode(
    bytes: &[u8],
    cancelled: impl Fn() -> bool,
) -> Result<DecodedAnimation, String> {
    use image::ImageDecoder;
    let invalid = || "Decodificación WebP: contenedor de animación no válido.".to_owned();
    if bytes.len() < 12 || &bytes[..4] != b"RIFF" || &bytes[8..12] != b"WEBP" {
        return Err(invalid());
    }
    let end = u32::from_le_bytes(bytes[4..8].try_into().unwrap()) as usize;
    if end.checked_add(8) != Some(bytes.len()) {
        return Err(invalid());
    }
    let u24 =
        |data: &[u8]| u32::from(data[0]) | (u32::from(data[1]) << 8) | (u32::from(data[2]) << 16);
    let mut canvas = None;
    let mut count = 0usize;
    let mut position = 12usize;
    let mut chunks = 0usize;
    while position < bytes.len() {
        if cancelled() {
            return Err("Carga de animación cancelada.".into());
        }
        if position.checked_add(8).is_none_or(|end| end > bytes.len()) {
            return Err(invalid());
        }
        let length =
            u32::from_le_bytes(bytes[position + 4..position + 8].try_into().unwrap()) as usize;
        let start = position + 8;
        let next = start
            .checked_add(length)
            .filter(|end| *end <= bytes.len())
            .ok_or_else(invalid)?;
        let data = &bytes[start..next];
        match &bytes[position..position + 4] {
            b"VP8X" => {
                if canvas.is_some() || data.len() != 10 {
                    return Err(invalid());
                }
                if data[0] & 2 == 0 {
                    return Err("El proveedor entregó una imagen WebP estática, no la animación. Se conserva la vista previa.".into());
                }
                let (width, height) = (u24(&data[4..7]) + 1, u24(&data[7..10]) + 1);
                if width > 1024 || height > 1024 {
                    return Err("Presupuesto local WebP: máximo 1024 píxeles por lado.".into());
                }
                canvas = Some((width, height));
            }
            b"ANMF" => {
                let (width, height) = canvas.ok_or_else(invalid)?;
                if data.len() < 24 {
                    return Err(invalid());
                }
                let (x, y, w, h) = (
                    u24(&data[0..3]) * 2,
                    u24(&data[3..6]) * 2,
                    u24(&data[6..9]) + 1,
                    u24(&data[9..12]) + 1,
                );
                if x + w > width || y + h > height {
                    return Err(invalid());
                }
                count += 1;
                if count > 400 || u64::from(width) * u64::from(height) * count as u64 > 64_000_000 {
                    return Err("Presupuesto local WebP: máximo 400 fotogramas y 64 millones de píxeles procesados.".into());
                }
            }
            _ => {}
        }
        chunks += 1;
        if chunks > 1616 {
            return Err(invalid());
        }
        position = next
            .checked_add(length % 2)
            .filter(|end| *end <= bytes.len())
            .ok_or_else(invalid)?;
    }
    let (width, height) = canvas.ok_or_else(|| {
        "El proveedor entregó una imagen WebP estática, no la animación.".to_string()
    })?;
    if count == 0 {
        return Err(invalid());
    }
    let scale = (BUDGET as f64 / (4. * f64::from(width) * f64::from(height) * count as f64))
        .sqrt()
        .min(1.)
        .min(360. / f64::from(width))
        .min(220. / f64::from(height));
    let target_w = ((f64::from(width) * scale) as u32).max(1);
    let target_h = ((f64::from(height) * scale) as u32).max(1);
    if target_w as usize * target_h as usize * 4 * count > BUDGET {
        return Err("Presupuesto local WebP: máximo 8 MiB de fotogramas reducidos.".into());
    }
    let decoder =
        image::codecs::webp::WebPDecoder::new(Cursor::new(bytes)).map_err(|_| invalid())?;
    if !decoder.has_animation() || decoder.dimensions() != (width, height) {
        return Err(invalid());
    }
    let mut frames = Vec::with_capacity(count);
    for frame in decoder.into_frames() {
        if cancelled() || frames.len() >= count {
            return Err("Carga de animación cancelada o secuencia no válida.".into());
        }
        let frame = frame.map_err(|_| invalid())?;
        let (n, d) = frame.delay().numer_denom_ms();
        let duration = animation_clock::source_delay(n, d);
        let image = frame.into_buffer();
        if image.dimensions() != (width, height) {
            return Err(invalid());
        }
        let thumbnail = image::imageops::resize(
            &image,
            target_w,
            target_h,
            image::imageops::FilterType::Nearest,
        );
        frames.push((target_w, target_h, thumbnail.into_raw(), duration));
    }
    if frames.len() != count {
        Err(invalid())
    } else {
        Ok(DecodedAnimation { canvas: (width, height), frames })
    }
}
