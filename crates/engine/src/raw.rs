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
/// dcraw 输出为 sRGB（带 gamma），此处先直存，后续在 image 层统一转线性。
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
                    data[idx] as f32 / 65535.0,
                    data[idx + 1] as f32 / 65535.0,
                    data[idx + 2] as f32 / 65535.0,
                ];
                img.set_pixel(x, y, [c[0], c[1], c[2], 1.0]);
            }
        }
        Ok(img)
    }
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
}
