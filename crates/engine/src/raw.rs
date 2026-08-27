//! RAW / 图像解码模块（文档 §2 第③层 RawDecode 步骤）。
//!
//! `RawDecoder` 抽象各类相机 RAW（CR2/ARW/NEF 等）的解码；生产实现为
//! LibRaw FFI 绑定（随安装包内置 libraw.dll），此处以 PPM(P6) 纯 Rust
//! 解码器作占位与测试，用于打通「解码 → 管线 → 导出」链路。

use crate::image::{srgb_to_linear, ColorSpace, ImageBuf};

/// RAW 解码错误。
#[derive(Debug)]
pub enum DecodeError {
    /// 不支持的格式。
    UnsupportedFormat,
    /// 头部损坏。
    InvalidHeader,
    /// 数据不足。
    Truncated,
}

impl std::fmt::Display for DecodeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DecodeError::UnsupportedFormat => write!(f, "不支持的图像格式"),
            DecodeError::InvalidHeader => write!(f, "图像头部损坏"),
            DecodeError::Truncated => write!(f, "图像数据不足"),
        }
    }
}

/// RAW 解码器抽象：输入文件字节，输出 16bit 线性 `ImageBuf`。
pub trait RawDecoder: Send + Sync {
    fn decode(&self, bytes: &[u8]) -> Result<ImageBuf, DecodeError>;
}

/// PPM(P6) 解码器（8bit / 16bit），纯 Rust 占位实现。
pub struct PpmDecoder;

impl RawDecoder for PpmDecoder {
    fn decode(&self, bytes: &[u8]) -> Result<ImageBuf, DecodeError> {
        decode_ppm(bytes)
    }
}

/// LibRaw FFI 解码器（CR2/ARW/NEF/DNG 等相机 RAW）。
///
/// 经 `libraw-rs`（vendored 静态编译 LibRaw）解码 + dcraw 处理为 16bit RGB。
/// dcraw 输出为 sRGB（带 gamma），此处转线性，与 PPM 路径保持一致。
pub struct LibRawDecoder;

impl RawDecoder for LibRawDecoder {
    fn decode(&self, bytes: &[u8]) -> Result<ImageBuf, DecodeError> {
        let processor = libraw::Processor::new();
        let image = processor
            .process_16bit(bytes)
            .map_err(|_| DecodeError::UnsupportedFormat)?;
        let w = image.width();
        let h = image.height();
        let data: &[u16] = &image;

        let mut img = ImageBuf::new(w, h, ColorSpace::Linear);
        for y in 0..h {
            for x in 0..w {
                let idx = ((y * w + x) * 3) as usize;
                if idx + 2 >= data.len() {
                    return Err(DecodeError::Truncated);
                }
                let c = [
                    srgb_to_linear(data[idx] as f32 / 65535.0),
                    srgb_to_linear(data[idx + 1] as f32 / 65535.0),
                    srgb_to_linear(data[idx + 2] as f32 / 65535.0),
                ];
                img.set_pixel(x, y, [c[0], c[1], c[2], 1.0]);
            }
        }
        Ok(img)
    }
}

/// 根据文件扩展名自动选择解码器（PPM / LibRaw）。
///
/// 支持的 RAW 扩展名：cr2/cr3/arw/nef/dng/raf/orf/rw2/pef/srw。
/// 支持的通用图片：jpg/jpeg/png（经 `image` crate）。
/// 其他格式返回 [`DecodeError::UnsupportedFormat`]。
pub fn decode_auto(path: &str, bytes: &[u8]) -> Result<ImageBuf, DecodeError> {
    let ext = std::path::Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();
    match ext.as_str() {
        "ppm" | "pnm" => PpmDecoder.decode(bytes),
        "cr2" | "cr3" | "arw" | "nef" | "dng" | "raf" | "orf" | "rw2" | "pef" | "srw" => {
            LibRawDecoder.decode(bytes)
        }
        "jpg" | "jpeg" | "png" => decode_image(bytes),
        _ => Err(DecodeError::UnsupportedFormat),
    }
}

/// 通用图片解码（JPEG/PNG，经 `image` crate），sRGB → 线性。
pub fn decode_image(bytes: &[u8]) -> Result<ImageBuf, DecodeError> {
    let img = image::load_from_memory(bytes).map_err(|_| DecodeError::UnsupportedFormat)?;
    let rgb = img.to_rgb8();
    let (w, h) = rgb.dimensions();
    let mut buf = ImageBuf::new(w, h, ColorSpace::Linear);
    for y in 0..h {
        for x in 0..w {
            let p = rgb.get_pixel(x, y);
            buf.set_pixel(
                x,
                y,
                [
                    srgb_to_linear(p[0] as f32 / 255.0),
                    srgb_to_linear(p[1] as f32 / 255.0),
                    srgb_to_linear(p[2] as f32 / 255.0),
                    1.0,
                ],
            );
        }
    }
    Ok(buf)
}

/// 解析 PPM P6（binary RGB）。
pub fn decode_ppm(bytes: &[u8]) -> Result<ImageBuf, DecodeError> {
    if bytes.len() < 2 || &bytes[0..2] != b"P6" {
        return Err(DecodeError::UnsupportedFormat);
    }
    let mut i = 2;

    i = skip_ws(bytes, i)?;
    let (w, ni) = parse_num(bytes, i)?;
    i = skip_ws(bytes, ni)?;
    let (h, ni) = parse_num(bytes, i)?;
    i = skip_ws(bytes, ni)?;
    let (maxval, ni) = parse_num(bytes, i)?;
    i = skip_ws(bytes, ni)?;

    let data = &bytes[i..];
    let mut img = ImageBuf::new(w, h, ColorSpace::Linear);

    if maxval <= 255 {
        let need = (w as usize) * (h as usize) * 3;
        if data.len() < need {
            return Err(DecodeError::Truncated);
        }
        for y in 0..h {
            for x in 0..w {
                let idx = ((y * w + x) * 3) as usize;
                let c = [
                    srgb_to_linear(data[idx] as f32 / 255.0),
                    srgb_to_linear(data[idx + 1] as f32 / 255.0),
                    srgb_to_linear(data[idx + 2] as f32 / 255.0),
                ];
                img.set_pixel(x, y, [c[0], c[1], c[2], 1.0]);
            }
        }
    } else {
        let need = (w as usize) * (h as usize) * 3 * 2;
        if data.len() < need {
            return Err(DecodeError::Truncated);
        }
        for y in 0..h {
            for x in 0..w {
                let idx = ((y * w + x) * 3) as usize;
                let c = [
                    srgb_to_linear(u16::from_be_bytes([data[idx * 2], data[idx * 2 + 1]]) as f32 / 65535.0),
                    srgb_to_linear(u16::from_be_bytes([data[idx * 2 + 2], data[idx * 2 + 3]]) as f32 / 65535.0),
                    srgb_to_linear(u16::from_be_bytes([data[idx * 2 + 4], data[idx * 2 + 5]]) as f32 / 65535.0),
                ];
                img.set_pixel(x, y, [c[0], c[1], c[2], 1.0]);
            }
        }
    }

    Ok(img)
}

fn skip_ws(bytes: &[u8], mut i: usize) -> Result<usize, DecodeError> {
    while i < bytes.len() && (bytes[i] == b' ' || bytes[i] == b'\n' || bytes[i] == b'\r' || bytes[i] == b'\t') {
        i += 1;
    }
    if i >= bytes.len() {
        return Err(DecodeError::Truncated);
    }
    Ok(i)
}

fn parse_num(bytes: &[u8], mut i: usize) -> Result<(u32, usize), DecodeError> {
    let mut v = 0u32;
    let mut any = false;
    while i < bytes.len() && bytes[i].is_ascii_digit() {
        v = v * 10 + (bytes[i] - b'0') as u32;
        i += 1;
        any = true;
    }
    if !any {
        return Err(DecodeError::InvalidHeader);
    }
    Ok((v, i))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ppm_8bit(w: u32, h: u32, rgb: [u8; 3]) -> Vec<u8> {
        let mut v = format!("P6\n{w} {h}\n255\n").into_bytes();
        for _ in 0..(w * h) {
            v.extend_from_slice(&rgb);
        }
        v
    }

    #[test]
    fn decode_8bit_solid() {
        let bytes = ppm_8bit(2, 2, [128, 64, 32]);
        let img = decode_ppm(&bytes).unwrap();
        assert_eq!((img.width, img.height), (2, 2));
        // 128/255 经 sRGB→Linear 后应约为 0.2158
        let p = img.pixel(0, 0);
        assert!((p[0] - 0.2158).abs() < 1e-3);
    }

    #[test]
    fn rejects_non_ppm() {
        let bytes = b"not an image";
        assert!(decode_ppm(bytes).is_err());
    }

    #[test]
    fn truncated_data() {
        let bytes = b"P6\n4 4\n255\n123"; // 数据不足
        assert!(decode_ppm(bytes).is_err());
    }

    #[test]
    fn decode_auto_dispatches_by_extension() {
        // .ppm → PPM 解码
        let bytes = ppm_8bit(2, 2, [128, 64, 32]);
        let img = decode_auto("photo.ppm", &bytes).unwrap();
        assert_eq!((img.width, img.height), (2, 2));

        // 未知扩展名 → 不支持
        assert!(matches!(
            decode_auto("photo.jpg", &bytes),
            Err(DecodeError::UnsupportedFormat)
        ));
        assert!(matches!(
            decode_auto("photo.xyz", &bytes),
            Err(DecodeError::UnsupportedFormat)
        ));
    }

    #[test]
    fn decode_image_png_roundtrip() {
        // 用 image crate 编码 2x2 PNG，再解码验证
        let mut bytes = Vec::new();
        let img = image::RgbImage::from_fn(2, 2, |x, y| {
            image::Rgb([x as u8 * 100, y as u8 * 100, 128])
        });
        image::DynamicImage::ImageRgb8(img)
            .write_to(&mut std::io::Cursor::new(&mut bytes), image::ImageFormat::Png)
            .unwrap();

        let decoded = decode_image(&bytes).unwrap();
        assert_eq!((decoded.width, decoded.height), (2, 2));
        // (0,0) = [0, 0, 128]，B 通道应转线性
        let p = decoded.pixel(0, 0);
        assert!(p[0] < 1e-4);
        assert!(p[1] < 1e-4);
        assert!((p[2] - srgb_to_linear(128.0 / 255.0)).abs() < 1e-4);
    }
}
