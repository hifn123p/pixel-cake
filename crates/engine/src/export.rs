//! 16bit TIFF 导出（文档 §4.10）。
//!
//! 手写最小 TIFF 编码（16bit RGB、uncompressed、little-endian），
//! 保持 16bit 全链路精度，避免 8bit 色带。可被标准图像查看器打开。

use crate::image::ImageBuf;

fn write_u16(buf: &mut [u8], pos: usize, v: u16) {
    buf[pos] = (v & 0xff) as u8;
    buf[pos + 1] = (v >> 8) as u8;
}

fn write_u32(buf: &mut [u8], pos: usize, v: u32) {
    buf[pos] = (v & 0xff) as u8;
    buf[pos + 1] = ((v >> 8) & 0xff) as u8;
    buf[pos + 2] = ((v >> 16) & 0xff) as u8;
    buf[pos + 3] = ((v >> 24) & 0xff) as u8;
}

/// 写入一个 IFD 条目（12 字节）。
fn write_entry(buf: &mut [u8], pos: usize, tag: u16, typ: u16, count: u32, value: u32) {
    write_u16(buf, pos, tag);
    write_u16(buf, pos + 2, typ);
    write_u32(buf, pos + 4, count);
    if typ == 3 && count == 1 {
        write_u16(buf, pos + 8, value as u16);
        write_u16(buf, pos + 10, 0);
    } else {
        write_u32(buf, pos + 8, value);
    }
}

/// 把线性 f32 图像编码为 16bit RGB TIFF（字节数组）。
pub fn encode_tiff(img: &ImageBuf) -> Vec<u8> {
    let w = img.width;
    let h = img.height;
    let pixel_bytes = (w as usize) * (h as usize) * 3 * 2;

    // 布局：header(8) + ifd_count(2) + 10*entry(120) + next_ifd(4) + bps_data(6) + image
    let ifd_count_pos = 8;
    let entries_start = ifd_count_pos + 2;
    const N_ENTRIES: usize = 10;
    let next_ifd_pos = entries_start + N_ENTRIES * 12;
    let bps_data_pos = next_ifd_pos + 4;
    let image_data_pos = bps_data_pos + 6;
    let total = image_data_pos + pixel_bytes;

    let mut buf = vec![0u8; total];

    // Header（II = little-endian，magic 42）
    buf[0] = b'I';
    buf[1] = b'I';
    write_u16(&mut buf, 2, 42);
    write_u32(&mut buf, 4, ifd_count_pos as u32);

    // IFD
    write_u16(&mut buf, ifd_count_pos, N_ENTRIES as u16);
    let mut e = entries_start;
    write_entry(&mut buf, e, 256, 4, 1, w); // ImageWidth
    e += 12;
    write_entry(&mut buf, e, 257, 4, 1, h); // ImageLength
    e += 12;
    write_entry(&mut buf, e, 258, 3, 3, bps_data_pos as u32); // BitsPerSample
    e += 12;
    write_entry(&mut buf, e, 259, 3, 1, 1); // Compression = none
    e += 12;
    write_entry(&mut buf, e, 262, 3, 1, 2); // Photometric = RGB
    e += 12;
    write_entry(&mut buf, e, 273, 4, 1, image_data_pos as u32); // StripOffsets
    e += 12;
    write_entry(&mut buf, e, 277, 3, 1, 3); // SamplesPerPixel
    e += 12;
    write_entry(&mut buf, e, 278, 4, 1, h); // RowsPerStrip
    e += 12;
    write_entry(&mut buf, e, 279, 4, 1, pixel_bytes as u32); // StripByteCounts
    e += 12;
    write_entry(&mut buf, e, 284, 3, 1, 1); // PlanarConfiguration = chunky

    write_u32(&mut buf, next_ifd_pos, 0); // 无下一个 IFD

    // BitsPerSample = 16,16,16
    write_u16(&mut buf, bps_data_pos, 16);
    write_u16(&mut buf, bps_data_pos + 2, 16);
    write_u16(&mut buf, bps_data_pos + 4, 16);

    // 图像数据（16bit RGB 交错，f32 0..1 → u16 0..65535）
    let mut pos = image_data_pos;
    for y in 0..h {
        for x in 0..w {
            let p = img.pixel(x, y);
            for c in 0..3 {
                let v = (p[c].clamp(0.0, 1.0) * 65535.0).round() as u16;
                write_u16(&mut buf, pos, v);
                pos += 2;
            }
        }
    }

    buf
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::image::ColorSpace;

    #[test]
    fn tiff_header() {
        let mut img = ImageBuf::new(4, 3, ColorSpace::Linear);
        img.set_pixel(0, 0, [1.0, 0.5, 0.0, 1.0]);
        let buf = encode_tiff(&img);
        // II + magic 42
        assert_eq!(&buf[0..2], b"II");
        assert_eq!(u16::from_le_bytes([buf[2], buf[3]]), 42);
    }

    #[test]
    fn tiff_size() {
        let img = ImageBuf::new(4, 3, ColorSpace::Linear);
        let buf = encode_tiff(&img);
        // header(8)+ifd(2+120+4)+bps(6) + image(4*3*3*2)
        assert_eq!(buf.len(), 8 + 2 + 120 + 4 + 6 + 4 * 3 * 3 * 2);
    }

    #[test]
    fn tiff_pixel_value() {
        let mut img = ImageBuf::new(1, 1, ColorSpace::Linear);
        img.set_pixel(0, 0, [1.0, 0.0, 0.5, 1.0]);
        let buf = encode_tiff(&img);
        let img_start = 8 + 2 + 120 + 4 + 6;
        let r = u16::from_le_bytes([buf[img_start], buf[img_start + 1]]);
        let g = u16::from_le_bytes([buf[img_start + 2], buf[img_start + 3]]);
        let b = u16::from_le_bytes([buf[img_start + 4], buf[img_start + 5]]);
        assert_eq!(r, 65535);
        assert_eq!(g, 0);
        assert_eq!(b, 32768);
    }
}
