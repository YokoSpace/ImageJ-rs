use ij_core::{
    CoreError, DisplayRange, ImageDocument, LineRoi, Lut256, PixelPlane, PolygonRoi, RectRoi, Roi,
};
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use thiserror::Error;

mod raster_formats;
pub use raster_formats::{ImageFileFormat, detect_image_format, load_image, save_jpeg, save_png};

#[derive(Debug, Error)]
pub enum IoError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("image codec error: {0}")]
    ImageCodec(#[from] image::ImageError),
    #[error("invalid image: {0}")]
    InvalidImage(String),
    #[error("core model error: {0}")]
    Core(#[from] CoreError),
    #[error("invalid TIFF header or unsupported byte order")]
    InvalidHeader,
    #[error("malformed TIFF: {0}")]
    Malformed(String),
    #[error("TIFF exceeds supported limits: {0}")]
    TooLarge(String),
    #[error("unsupported TIFF format: {0}")]
    UnsupportedFormat(String),
}

const TAG_IMAGE_WIDTH: u16 = 256;
const TAG_IMAGE_LENGTH: u16 = 257;
const TAG_BITS_PER_SAMPLE: u16 = 258;
const TAG_COMPRESSION: u16 = 259;
const TAG_PHOTO_INTERP: u16 = 262;
const TAG_IMAGE_DESCRIPTION: u16 = 270;
const TAG_STRIP_OFFSETS: u16 = 273;
const TAG_ORIENTATION: u16 = 274;
const TAG_SAMPLES_PER_PIXEL: u16 = 277;
const TAG_ROWS_PER_STRIP: u16 = 278;
const TAG_STRIP_BYTE_COUNTS: u16 = 279;
const TAG_X_RESOLUTION: u16 = 282;
const TAG_Y_RESOLUTION: u16 = 283;
const TAG_PLANAR_CONFIG: u16 = 284;
const TAG_RESOLUTION_UNIT: u16 = 296;
const TAG_PREDICTOR: u16 = 317;
const TAG_COLOR_MAP: u16 = 320;
const TAG_TILE_WIDTH: u16 = 322;
const TAG_TILE_LENGTH: u16 = 323;
const TAG_SAMPLE_FORMAT: u16 = 339;
// ImageJ private metadata tags from TiffDecoder.java (META_DATA_BYTE_COUNTS /
// META_DATA) and TiffEncoder.writeMetaData.
const TAG_META_DATA_BYTE_COUNTS: u16 = 50838;
const TAG_META_DATA: u16 = 50839;

const META_MAGIC: u32 = 0x494a_494a; // "IJIJ"
const META_INFO: u32 = 0x696e_666f; // "info"
const META_LABELS: u32 = 0x6c61_626c; // "labl"
const META_RANGES: u32 = 0x7261_6e67; // "rang"
const META_LUTS: u32 = 0x6c75_7473; // "luts"
const META_PLOT: u32 = 0x706c_6f74; // "plot"
const META_ROI: u32 = 0x726f_6920; // "roi "
const META_OVERLAY: u32 = 0x6f76_6572; // "over"
const META_PROPERTIES: u32 = 0x7072_6f70; // "prop"

// ImageJ ROI record layout from RoiDecoder.java. The record is always
// big-endian, independent of the TIFF byte order (RoiEncoder.putShort).
const ROI_HEADER_SIZE: usize = 64;
const ROI_HEADER2_SIZE: usize = 64;
const ROI_VERSION: u16 = 229;
const ROI_TYPE_POLYGON: u8 = 0;
const ROI_TYPE_RECT: u8 = 1;
const ROI_TYPE_OVAL: u8 = 2;
const ROI_TYPE_LINE: u8 = 3;
const ROI_SUB_PIXEL_RESOLUTION: u16 = 128;

const NUM_IFD_TAGS: u16 = 10;
const HEADER_SIZE: u32 = 8;

// Compressed pixel bytes need an independent bound: their size is no longer
// bounded by the input file length. Share this budget across the whole stack.
const MAX_DECODED_TIFF_BYTES: usize = 512 * 1024 * 1024;

#[derive(Clone, Copy)]
enum TiffCompression {
    None,
    PackBits,
    Lzw,
    Deflate,
}

impl TiffCompression {
    fn from_tag(tag: u16) -> Result<Self, IoError> {
        match tag {
            0 | 1 => Ok(Self::None),
            32773 => Ok(Self::PackBits),
            5 => Ok(Self::Lzw),
            8 | 32946 => Ok(Self::Deflate),
            _ => Err(IoError::UnsupportedFormat(format!(
                "TIFF Compression={tag}"
            ))),
        }
    }

    fn is_compressed(self) -> bool {
        !matches!(self, Self::None)
    }
}

fn decompress_tiff_strip(
    input: &[u8],
    compression: TiffCompression,
    expected: usize,
) -> Result<Vec<u8>, IoError> {
    let malformed = |message: &str| IoError::Malformed(format!("compressed TIFF strip: {message}"));
    let mut output = Vec::new();
    match compression {
        TiffCompression::None => unreachable!("uncompressed strips are copied directly"),
        TiffCompression::PackBits => {
            let mut position = 0;
            while output.len() < expected && position < input.len() {
                let control = input[position] as i8;
                position += 1;
                match control {
                    0..=127 => {
                        let count = control as usize + 1;
                        let end = position + count;
                        let literal = input
                            .get(position..end)
                            .ok_or_else(|| malformed("truncated PackBits literal"))?;
                        if count > expected - output.len() {
                            return Err(malformed("PackBits output exceeds strip size"));
                        }
                        output.extend_from_slice(literal);
                        position = end;
                    }
                    -127..=-1 => {
                        let count = (1 - i16::from(control)) as usize;
                        let value = *input
                            .get(position)
                            .ok_or_else(|| malformed("truncated PackBits repeat"))?;
                        position += 1;
                        if count > expected - output.len() {
                            return Err(malformed("PackBits output exceeds strip size"));
                        }
                        output.resize(output.len() + count, value);
                    }
                    -128 => {}
                }
            }
        }
        TiffCompression::Lzw => {
            // ImageReader.java 1030-1032 switches at 511/1023/2047, one code
            // before GIF LZW. ImageJ stops at the image length without an EOI.
            let mut decoder =
                weezl::decode::Configuration::with_tiff_size_switch(weezl::BitOrder::Msb, 8)
                    .with_yield_on_full_buffer(true)
                    .build();
            let mut position = 0;
            let mut buffer = [0u8; 8192];
            while output.len() < expected {
                let limit = buffer.len().min(expected - output.len());
                let result = decoder.decode_bytes(&input[position..], &mut buffer[..limit]);
                position += result.consumed_in;
                output.extend_from_slice(&buffer[..result.consumed_out]);
                match result.status.map_err(|e| malformed(&format!("LZW: {e}")))? {
                    weezl::LzwStatus::Done => break,
                    weezl::LzwStatus::NoProgress => return Err(malformed("truncated LZW data")),
                    weezl::LzwStatus::Ok => {}
                }
            }
        }
        TiffCompression::Deflate => {
            // Both TIFF Deflate tags use Java Inflater's zlib wrapper, not
            // a raw DEFLATE stream (ImageReader.java 957-975).
            let mut decoder = flate2::Decompress::new(true);
            let mut buffer = [0u8; 8192];
            loop {
                let before_in = decoder.total_in();
                let before_out = decoder.total_out();
                let limit = buffer.len().min(expected - output.len() + 1);
                let status = decoder
                    .decompress(
                        &input[before_in as usize..],
                        &mut buffer[..limit],
                        flate2::FlushDecompress::None,
                    )
                    .map_err(|e| malformed(&format!("Deflate: {e}")))?;
                let count = (decoder.total_out() - before_out) as usize;
                if count > expected - output.len() {
                    return Err(malformed("Deflate output exceeds strip size"));
                }
                output.extend_from_slice(&buffer[..count]);
                if status == flate2::Status::StreamEnd {
                    break;
                }
                if decoder.total_in() == before_in && count == 0 {
                    return Err(malformed("truncated Deflate data"));
                }
            }
        }
    }
    if output.len() != expected {
        return Err(malformed(&format!(
            "decoded {} of {expected} bytes",
            output.len()
        )));
    }
    Ok(output)
}

fn undo_tiff_predictor(raw: &mut [u8], width: usize, bits: u16, samples: u16, le: bool) {
    let stride = width * usize::from(samples) * usize::from(bits / 8);
    for row in raw.chunks_exact_mut(stride) {
        if bits == 16 {
            for x in 1..width {
                let read = |i| {
                    if le {
                        u16::from_le_bytes([row[i], row[i + 1]])
                    } else {
                        u16::from_be_bytes([row[i], row[i + 1]])
                    }
                };
                let value = read(x * 2).wrapping_add(read((x - 1) * 2));
                let bytes = if le {
                    value.to_le_bytes()
                } else {
                    value.to_be_bytes()
                };
                row[x * 2..x * 2 + 2].copy_from_slice(&bytes);
            }
        } else {
            let samples = usize::from(samples);
            for i in samples..stride {
                row[i] = row[i].wrapping_add(row[i - samples]);
            }
        }
    }
}

struct PlaneEncodingInfo {
    bits_per_sample: u16,
    samples_per_pixel: u16,
    sample_format: u16,
    photometric: u16,
    palette: Option<Box<Lut256>>,
    data_len: usize,
}

#[derive(Clone)]
struct PlaneFormat {
    bits_per_sample: u16,
    samples_per_pixel: u16,
    sample_format: u16,
    photometric: u16,
    palette: Option<Box<Lut256>>,
    pixel_count: usize,
    plane_bytes: usize,
}

impl PlaneFormat {
    fn decode(&self, raw: &[u8], le: bool) -> Result<PixelPlane, IoError> {
        if raw.len() < self.plane_bytes {
            return Err(IoError::Malformed(format!(
                "plane data is short: got {} of {} bytes",
                raw.len(),
                self.plane_bytes
            )));
        }
        let plane = match (
            self.bits_per_sample,
            self.samples_per_pixel,
            self.sample_format,
        ) {
            (8, 1, 1) if self.photometric == 3 => {
                let lut = self
                    .palette
                    .as_ref()
                    .ok_or_else(|| IoError::UnsupportedFormat("missing ColorMap".into()))?;
                PixelPlane::Color256 {
                    pixels: raw[..self.plane_bytes].to_vec(),
                    lut: lut.clone(),
                }
            }
            (8, 1, 1) if self.photometric <= 1 => {
                PixelPlane::Gray8(raw[..self.plane_bytes].to_vec())
            }
            (16, 1, 1) if self.photometric <= 1 => {
                let mut v = Vec::with_capacity(self.pixel_count);
                for chunk in raw.chunks_exact(2).take(self.pixel_count) {
                    let val = if le {
                        u16::from_le_bytes([chunk[0], chunk[1]])
                    } else {
                        u16::from_be_bytes([chunk[0], chunk[1]])
                    };
                    v.push(val);
                }
                PixelPlane::Gray16(v)
            }
            (32, 1, 3) if self.photometric <= 1 => {
                let mut v = Vec::with_capacity(self.pixel_count);
                for chunk in raw.chunks_exact(4).take(self.pixel_count) {
                    let val = if le {
                        f32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]])
                    } else {
                        f32::from_be_bytes([chunk[0], chunk[1], chunk[2], chunk[3]])
                    };
                    v.push(val);
                }
                PixelPlane::Gray32(v)
            }
            (8, 3, 1) if self.photometric == 2 => {
                let mut v = Vec::with_capacity(self.pixel_count);
                for chunk in raw.chunks_exact(3).take(self.pixel_count) {
                    let argb = 0xff00_0000
                        | ((chunk[0] as u32) << 16)
                        | ((chunk[1] as u32) << 8)
                        | (chunk[2] as u32);
                    v.push(argb);
                }
                PixelPlane::ColorRgb(v)
            }
            _ => {
                return Err(IoError::UnsupportedFormat(format!(
                    "bps={}, spp={}, fmt={}, photometric={}",
                    self.bits_per_sample,
                    self.samples_per_pixel,
                    self.sample_format,
                    self.photometric
                )));
            }
        };
        Ok(plane)
    }
}

/// One ImageJ private metadata entry (`MetaData` tag), grouped by its four
/// character type code. ImageJ's TiffEncoder writes these as a header followed
/// by sequential payloads (TiffEncoder.java 371-472).
struct ImageJMetadata {
    kind: u32,
    payloads: Vec<Vec<u8>>,
}

fn validate_lut(lut: &Lut256) -> Result<(), IoError> {
    if lut.r.len() != 256 || lut.g.len() != 256 || lut.b.len() != 256 {
        return Err(IoError::UnsupportedFormat(
            "palette LUT must have exactly 256 entries per channel".into(),
        ));
    }
    Ok(())
}

/// Describes a slice's on-disk encoding and encoded byte length without
/// copying pixels, so an oversized stack can be rejected before allocating.
fn plane_encoding_info(plane: &PixelPlane) -> Result<PlaneEncodingInfo, IoError> {
    let (bits_per_sample, samples_per_pixel, sample_format, photometric, palette, bytes_per_pixel) =
        match plane {
            PixelPlane::Gray8(_) => (8, 1, 1, 1, None, 1usize),
            PixelPlane::Color256 { lut, .. } => {
                validate_lut(lut)?;
                (8, 1, 1, 3, Some(lut.clone()), 1usize)
            }
            PixelPlane::Gray16(_) => (16, 1, 1, 1, None, 2),
            PixelPlane::Gray32(_) => (32, 1, 3, 1, None, 4),
            PixelPlane::ColorRgb(_) => (8, 3, 1, 2, None, 3),
        };
    let data_len = plane
        .len()
        .checked_mul(bytes_per_pixel)
        .ok_or_else(|| IoError::TooLarge("slice pixel data length overflow".into()))?;
    Ok(PlaneEncodingInfo {
        bits_per_sample,
        samples_per_pixel,
        sample_format,
        photometric,
        palette,
        data_len,
    })
}

fn encode_plane(plane: &PixelPlane) -> Vec<u8> {
    match plane {
        PixelPlane::Gray8(v) | PixelPlane::Color256 { pixels: v, .. } => v.clone(),
        PixelPlane::Gray16(v) => {
            let mut bytes = Vec::with_capacity(v.len() * 2);
            for &px in v {
                bytes.extend_from_slice(&px.to_le_bytes());
            }
            bytes
        }
        PixelPlane::Gray32(v) => {
            let mut bytes = Vec::with_capacity(v.len() * 4);
            for &px in v {
                bytes.extend_from_slice(&px.to_le_bytes());
            }
            bytes
        }
        PixelPlane::ColorRgb(v) => {
            let mut bytes = Vec::with_capacity(v.len() * 3);
            for &px in v {
                bytes.push(((px >> 16) & 0xff) as u8);
                bytes.push(((px >> 8) & 0xff) as u8);
                bytes.push((px & 0xff) as u8);
            }
            bytes
        }
    }
}

/// True when RoiDecoder.getShort would read `value` back unchanged. Values
/// below -5000 are read as unsigned by original ImageJ (`RoiDecoder.java`
/// 567-574), so those coordinates need the float ROI encoding to stay exact.
fn roi_short_roundtrips(value: i32) -> bool {
    if !((i32::from(i16::MIN))..=(i32::from(u16::MAX))).contains(&value) {
        return false;
    }
    let raw = (value as u32 & 0xffff) as u16;
    let signed = raw as i16 as i32;
    let decoded = if signed < -5000 { raw as i32 } else { signed };
    decoded == value
}

/// ImageJ stores ROI coordinates as 32-bit floats in the subpixel encoding.
/// Reject integers that a float cannot represent exactly instead of silently
/// changing the selection.
fn roi_exact_float(value: i32) -> Option<f32> {
    let float = value as f32;
    if f64::from(float) == f64::from(value) {
        Some(float)
    } else {
        None
    }
}

/// Java `Line.getBounds()` uses the open-line convention of
/// `Roi.setIntBounds`, which encloses the one-pixel draw area (`Line.java`
/// 350-359, `Roi.java` 538-543). The Rust `LineRoi::bounds` keeps the raw
/// endpoint span, which is not what RoiEncoder stores.
fn java_line_bounds(line: &LineRoi) -> (i32, i32, i32, i32) {
    let min_x = line.x1.min(line.x2);
    let min_y = line.y1.min(line.y2);
    let dx = (i64::from(line.x2) - i64::from(line.x1)).abs() as f64;
    let dy = (i64::from(line.y2) - i64::from(line.y1)).abs() as f64;
    let x = (f64::from(min_x) + 0.5).floor() as i32;
    let y = (f64::from(min_y) + 0.5).floor() as i32;
    let width = ((f64::from(min_x) + dx + 1.5).floor() as i32).saturating_sub(x);
    let height = ((f64::from(min_y) + dy + 1.5).floor() as i32).saturating_sub(y);
    (x, y, width, height)
}

fn put_roi_short(out: &mut [u8], offset: usize, value: i32) {
    out[offset..offset + 2].copy_from_slice(&(value as u16).to_be_bytes());
}

fn put_roi_int(out: &mut [u8], offset: usize, value: i32) {
    out[offset..offset + 4].copy_from_slice(&value.to_be_bytes());
}

fn put_roi_float(out: &mut [u8], offset: usize, value: f32) {
    out[offset..offset + 4].copy_from_slice(&value.to_be_bytes());
}

fn unsupported_roi(detail: impl std::fmt::Display) -> IoError {
    IoError::UnsupportedFormat(format!(
        "ImageJ TIFF ROI metadata {detail} cannot be represented; only rectangle, oval, \
         polygon and line selections with exact integral coordinates are supported"
    ))
}

/// Encodes the selection as an ImageJ ROI record (RoiEncoder.java 80-290)
/// for the TIFF `roi ` metadata block written by TiffEncoder.writeMetaData.
fn encode_roi_metadata(roi: &Roi) -> Result<Vec<u8>, IoError> {
    match roi {
        Roi::Rect(rect) => encode_rect_or_oval(rect, ROI_TYPE_RECT),
        Roi::Oval(rect) => encode_rect_or_oval(rect, ROI_TYPE_OVAL),
        Roi::Polygon(polygon) => encode_polygon_roi(polygon),
        Roi::Line(line) => encode_line_roi(line),
    }
}

fn encode_rect_or_oval(rect: &RectRoi, roi_type: u8) -> Result<Vec<u8>, IoError> {
    if rect.width <= 0 || rect.height <= 0 {
        return Err(unsupported_roi(format!(
            "with non-positive bounds {}x{}",
            rect.width, rect.height
        )));
    }
    let right = rect
        .x
        .checked_add(rect.width)
        .ok_or_else(|| unsupported_roi("with overflowing right edge"))?;
    let bottom = rect
        .y
        .checked_add(rect.height)
        .ok_or_else(|| unsupported_roi("with overflowing bottom edge"))?;
    let use_floats = ![rect.x, rect.y, right, bottom]
        .into_iter()
        .all(roi_short_roundtrips);

    let mut out = vec![0u8; ROI_HEADER_SIZE + ROI_HEADER2_SIZE];
    out[..4].copy_from_slice(b"Iout");
    put_roi_short(&mut out, 4, i32::from(ROI_VERSION));
    out[6] = roi_type;
    put_roi_short(&mut out, 8, rect.y);
    put_roi_short(&mut out, 10, rect.x);
    put_roi_short(&mut out, 12, bottom);
    put_roi_short(&mut out, 14, right);
    put_roi_short(&mut out, 16, 0);
    let mut options = 0u16;
    if use_floats {
        for (index, value) in [rect.x, rect.y, rect.width, rect.height]
            .into_iter()
            .enumerate()
        {
            let float = roi_exact_float(value).ok_or_else(|| {
                unsupported_roi(format!(
                    "with coordinate {value} that a 32-bit float cannot hold"
                ))
            })?;
            put_roi_float(&mut out, 18 + index * 4, float);
        }
        options |= ROI_SUB_PIXEL_RESOLUTION;
    }
    put_roi_short(&mut out, 50, i32::from(options));
    put_roi_int(&mut out, 60, ROI_HEADER_SIZE as i32);
    Ok(out)
}

fn encode_polygon_roi(polygon: &PolygonRoi) -> Result<Vec<u8>, IoError> {
    if polygon.xpoints.len() != polygon.ypoints.len() || polygon.xpoints.len() < 3 {
        return Err(unsupported_roi("with fewer than three polygon points"));
    }
    let n = polygon.xpoints.len();
    if n > u16::MAX as usize {
        return Err(unsupported_roi(format!(
            "with {n} points (ImageJ saves more than 65535 points as a ShapeRoi)"
        )));
    }
    let (left, top, width, height) = polygon.bounds();
    if width == 0 && height == 0 {
        // ImagePlus.setRoi deletes an area selection whose width and height
        // are both zero (ImagePlus.java 2073-2076), so this file would open
        // with no selection at all.
        return Err(unsupported_roi(
            "with zero width and height, which ImageJ drops on open",
        ));
    }
    let right = left
        .checked_add(width)
        .ok_or_else(|| unsupported_roi("with overflowing right edge"))?;
    let bottom = top
        .checked_add(height)
        .ok_or_else(|| unsupported_roi("with overflowing bottom edge"))?;

    let mut relative = Vec::with_capacity(n);
    let mut use_floats = ![left, top, right, bottom]
        .into_iter()
        .all(roi_short_roundtrips);
    for (&x, &y) in polygon.xpoints.iter().zip(&polygon.ypoints) {
        let rx = x
            .checked_sub(left)
            .ok_or_else(|| unsupported_roi("with an overflowing relative x coordinate"))?;
        let ry = y
            .checked_sub(top)
            .ok_or_else(|| unsupported_roi("with an overflowing relative y coordinate"))?;
        if rx < 0 || ry < 0 || !roi_short_roundtrips(rx) || !roi_short_roundtrips(ry) {
            use_floats = true;
        }
        relative.push((rx, ry));
    }
    let float_points = if use_floats {
        // FloatPolygon initializes maxX/maxY with Float.MIN_VALUE, a small
        // *positive* value, instead of -Float.MAX_VALUE (FloatPolygon.java
        // 89-105). An all-negative axis is therefore decoded with a bound
        // extending to 0, and an axis whose minimum and maximum are both 0
        // is decoded with width/height 1. Refuse to create a file Java reads
        // with bounds that differ from the Rust polygon.
        if right < 0 || bottom < 0 || (left == 0 && right == 0) || (top == 0 && bottom == 0) {
            return Err(unsupported_roi(
                "with a subpixel axis whose ImageJ FloatPolygon bounds are incorrect",
            ));
        }
        Some(
            polygon
                .xpoints
                .iter()
                .zip(&polygon.ypoints)
                .map(|(&x, &y)| {
                    Ok((
                        roi_exact_float(x).ok_or_else(|| {
                            unsupported_roi(format!(
                                "with coordinate {x} that a 32-bit float cannot hold"
                            ))
                        })?,
                        roi_exact_float(y).ok_or_else(|| {
                            unsupported_roi(format!(
                                "with coordinate {y} that a 32-bit float cannot hold"
                            ))
                        })?,
                    ))
                })
                .collect::<Result<Vec<(f32, f32)>, IoError>>()?,
        )
    } else {
        None
    };

    let float_size = if use_floats { n * 8 } else { 0 };
    let mut out = vec![0u8; ROI_HEADER_SIZE + n * 4 + float_size + ROI_HEADER2_SIZE];
    out[..4].copy_from_slice(b"Iout");
    put_roi_short(&mut out, 4, i32::from(ROI_VERSION));
    out[6] = ROI_TYPE_POLYGON;
    put_roi_short(&mut out, 8, top);
    put_roi_short(&mut out, 10, left);
    put_roi_short(&mut out, 12, bottom);
    put_roi_short(&mut out, 14, right);
    put_roi_short(&mut out, 16, n as i32);
    let mut options = 0u16;
    if use_floats {
        options |= ROI_SUB_PIXEL_RESOLUTION;
    }
    put_roi_short(&mut out, 50, i32::from(options));
    for (index, &(rx, ry)) in relative.iter().enumerate() {
        put_roi_short(&mut out, 64 + index * 2, rx);
        put_roi_short(&mut out, 64 + n * 2 + index * 2, ry);
    }
    if let Some(float_points) = &float_points {
        let x_base = 64 + n * 4;
        let y_base = x_base + n * 4;
        for (index, &(x, y)) in float_points.iter().enumerate() {
            put_roi_float(&mut out, x_base + index * 4, x);
            put_roi_float(&mut out, y_base + index * 4, y);
        }
    }
    put_roi_int(&mut out, 60, (ROI_HEADER_SIZE + n * 4 + float_size) as i32);
    Ok(out)
}

fn encode_line_roi(line: &LineRoi) -> Result<Vec<u8>, IoError> {
    let (left, top, width, height) = java_line_bounds(line);
    let right = left.saturating_add(width);
    let bottom = top.saturating_add(height);
    let floats: Vec<f32> = [line.x1, line.y1, line.x2, line.y2]
        .into_iter()
        .map(|value| {
            roi_exact_float(value).ok_or_else(|| {
                unsupported_roi(format!(
                    "with line endpoint {value} that a 32-bit float cannot hold"
                ))
            })
        })
        .collect::<Result<_, _>>()?;

    let mut out = vec![0u8; ROI_HEADER_SIZE + ROI_HEADER2_SIZE];
    out[..4].copy_from_slice(b"Iout");
    put_roi_short(&mut out, 4, i32::from(ROI_VERSION));
    out[6] = ROI_TYPE_LINE;
    put_roi_short(&mut out, 8, top);
    put_roi_short(&mut out, 10, left);
    put_roi_short(&mut out, 12, bottom);
    put_roi_short(&mut out, 14, right);
    put_roi_short(&mut out, 16, 0);
    for (index, value) in floats.into_iter().enumerate() {
        put_roi_float(&mut out, 18 + index * 4, value);
    }
    put_roi_short(&mut out, 50, 0);
    put_roi_int(&mut out, 60, ROI_HEADER_SIZE as i32);
    Ok(out)
}

fn encode_document_metadata(doc: &ImageDocument) -> Result<Option<(Vec<u8>, Vec<u8>)>, IoError> {
    if doc.slice_labels.len() != doc.slices.len() {
        return Err(IoError::UnsupportedFormat(
            "slice label count differs from stack size".into(),
        ));
    }
    let has_labels = doc
        .slice_labels
        .iter()
        .any(|label| label.as_ref().is_some_and(|label| !label.is_empty()));
    let roi = doc.roi.as_ref().map(encode_roi_metadata).transpose()?;
    if !has_labels && roi.is_none() {
        return Ok(None);
    }
    let types = u32::from(has_labels) + u32::from(roi.is_some());
    let header_size = 4 + types * 8;
    let mut counts = Vec::new();
    counts.extend_from_slice(&header_size.to_le_bytes());
    let mut data = Vec::new();
    data.extend_from_slice(&META_MAGIC.to_le_bytes());
    if has_labels {
        data.extend_from_slice(&META_LABELS.to_le_bytes());
        data.extend_from_slice(&(doc.slice_labels.len() as u32).to_le_bytes());
    }
    if roi.is_some() {
        data.extend_from_slice(&META_ROI.to_le_bytes());
        data.extend_from_slice(&1u32.to_le_bytes());
    }
    let mut payload = Vec::new();
    if has_labels {
        for label in &doc.slice_labels {
            let encoded: Vec<u16> = label.as_deref().unwrap_or("").encode_utf16().collect();
            let byte_len = encoded
                .len()
                .checked_mul(2)
                .and_then(|len| u32::try_from(len).ok())
                .ok_or_else(|| IoError::TooLarge("slice label exceeds TIFF limits".into()))?;
            counts.extend_from_slice(&byte_len.to_le_bytes());
            for unit in encoded {
                payload.extend_from_slice(&unit.to_le_bytes());
            }
        }
    }
    if let Some(roi) = roi {
        let byte_len = u32::try_from(roi.len())
            .map_err(|_| IoError::TooLarge("ROI metadata exceeds TIFF limits".into()))?;
        counts.extend_from_slice(&byte_len.to_le_bytes());
        payload.extend_from_slice(&roi);
    }
    data.extend_from_slice(&payload);
    Ok(Some((counts, data)))
}

/// Writes one uncompressed little-endian TIFF IFD per stack slice. The pixel
/// arrays are contiguous at the start of the file and the IFD chain follows
/// them, so ImageJ's `Opener.allSameSizeAndType` sees a contiguous stack
/// (original_imagej/ij/io/Opener.java lines 795-813).
///
/// # Metadata (ImageJ FileSaver/TiffEncoder format)
///
/// These `ImageDocument` fields are persisted and read back by ImageJ 1.54u:
///
/// - `display_range`: written as `min=`/`max=` for GRAY8 and COLOR_256 only
///   when non-default (0..255); always for GRAY16/GRAY32.
/// - `calibration.pixel_width`/`pixel_height` and `calibration.unit`: written
///   as XResolution/YResolution RATIONALs plus ResolutionUnit and a `unit=`
///   line, exactly like `TiffEncoder.writeScale` and
///   `FileSaver.getDescriptionString`. Units are canonicalized like
///   `Calibration.setUnit` (`um` becomes `µm`, `A` becomes `Å`).
/// - `calibration.pixel_depth`: written as `spacing=` for stacks only.
/// - `calibration.invert_y`: written as `inverty=true`.
/// - C/Z/T stack shape and `hyperstack=true` via the ImageDescription.
/// - `roi`: written as an ImageJ ROI record in the private `roi ` metadata
///   block (FileSaver.java 117, TiffEncoder.writeMetaData 371-472) for
///   rectangular, oval, polygon and line selections.
/// - `slice_labels`: written as ImageJ's UTF-16 `labl` metadata.
///
/// Unsupported non-default state is rejected instead of dropped silently:
///
/// - a non-default `pixel_depth` on a single-plane image (ImageJ never writes
///   `spacing=` for single images);
/// - a non-default `display_range` on RGB images (ImageJ does not persist a
///   range for `COLOR_RGB`);
/// - a `0/0` display range unless the image-derived range is also `0/0`
///   (ImageJ treats a stored `0/0` as "not set", `FileOpener.java` 452-463);
/// - non-finite or non-positive scales while calibration is active;
/// - an ROI whose integer coordinates are outside the exact range of ImageJ's
///   signed-short header and 32-bit float subpixel encoding.
///
/// Not part of the Rust model and therefore not written: x/y/z origins, time
/// units/frame interval/fps/loop, y/z units, density calibration (`cf`/
/// `vunit`), overlays, image properties, and per-channel
/// composite display ranges/LUTs.
pub fn save_tiff(path: impl AsRef<Path>, doc: &ImageDocument) -> Result<(), IoError> {
    let path = path.as_ref();
    let width = doc.width;
    let height = doc.height;
    if doc.slices.is_empty() {
        return Err(IoError::UnsupportedFormat("empty image stack".into()));
    }

    let infos: Vec<PlaneEncodingInfo> = doc
        .slices
        .iter()
        .map(plane_encoding_info)
        .collect::<Result<_, _>>()?;
    let first = &infos[0];
    for (index, info) in infos.iter().enumerate() {
        if info.bits_per_sample != first.bits_per_sample
            || info.samples_per_pixel != first.samples_per_pixel
            || info.sample_format != first.sample_format
            || info.palette != first.palette
        {
            return Err(IoError::UnsupportedFormat(format!(
                "stack slice {index} has a different pixel type or palette"
            )));
        }
        if info.data_len != first.data_len {
            return Err(IoError::UnsupportedFormat(format!(
                "stack slice {index} has a different pixel data size"
            )));
        }
    }

    let bits_per_sample = first.bits_per_sample;
    let samples_per_pixel = first.samples_per_pixel;
    let sample_format = first.sample_format;
    let photo_interp = first.photometric;
    let palette = first.palette.as_deref();
    let n = infos.len() as u64;
    let data_start = u64::from(HEADER_SIZE);
    let data_len = first.data_len as u64;
    let too_large = || IoError::TooLarge("classic TIFF offsets are limited to 4 GiB".into());
    let checked = |value: Option<u64>| value.ok_or_else(too_large);

    let ifd_start = checked(
        data_start
            .checked_add(checked(n.checked_mul(data_len))?)
            .and_then(|v| {
                if v % 2 == 1 {
                    v.checked_add(1)
                } else {
                    Some(v)
                }
            }),
    )?;

    // ImageJ reads C/Z/T, display range and spatial calibration from the
    // ImageDescription created by FileSaver
    // (FileOpener.decodeDescriptionString, FileOpener.java lines 605-650).
    let description: Option<Vec<u8>> = image_description(doc)?.map(|mut text| {
        text.push('\0');
        text.into_bytes()
    });
    let metadata = encode_document_metadata(doc)?;
    let scaled = calibration_is_scaled(doc);
    let num_tags: u16 = NUM_IFD_TAGS
        + u16::from(description.is_some())
        + u16::from(palette.is_some())
        + if scaled { 3 } else { 0 }
        + if metadata.is_some() { 2 } else { 0 };
    let ifd_size = 2 + u64::from(num_tags) * 12 + 4;
    let extra_bps_size: u64 = if samples_per_pixel == 3 { 6 } else { 0 };
    let tag_data_offset = checked(ifd_start.checked_add(checked(n.checked_mul(ifd_size))?))?;
    let description_size = description.as_ref().map_or(0, |d| d.len() as u64);
    let color_map_size: u64 = if palette.is_some() { 768 * 2 } else { 0 };
    let scale_size: u64 = if scaled { 16 } else { 0 };
    let description_offset = checked(tag_data_offset.checked_add(extra_bps_size))?;
    // TIFF values should start on a word boundary. Padding is needed when a
    // palette or the XResolution/YResolution rationals follow the description.
    let description_block_size = if palette.is_some() || scaled {
        (description_size + 1) & !1
    } else {
        description_size
    };
    let color_map_offset = checked(description_offset.checked_add(description_block_size))?;
    let after_color_map = checked(color_map_offset.checked_add(color_map_size))?;
    let scale_offset = if scaled && after_color_map % 2 == 1 {
        checked(after_color_map.checked_add(1))?
    } else {
        after_color_map
    };
    let after_scale = checked(scale_offset.checked_add(scale_size))?;
    let metadata_counts_size = metadata
        .as_ref()
        .map_or(0, |(counts, _)| counts.len() as u64);
    let metadata_block_size = metadata.as_ref().map_or(0, |(_, data)| data.len() as u64);
    let metadata_counts_offset = if metadata.is_some() && after_scale % 2 == 1 {
        checked(after_scale.checked_add(1))?
    } else {
        after_scale
    };
    let metadata_offset = checked(metadata_counts_offset.checked_add(metadata_counts_size))?;
    let total_size = checked(metadata_offset.checked_add(metadata_block_size))?;
    if total_size > u64::from(u32::MAX) {
        return Err(too_large());
    }
    let total_size = usize::try_from(total_size).map_err(|_| too_large())?;

    // Encode only after the layout is known to fit in classic TIFF.
    let encoded: Vec<Vec<u8>> = doc.slices.iter().map(encode_plane).collect();
    let mut out = Vec::with_capacity(total_size);
    out.extend_from_slice(b"II");
    out.extend_from_slice(&42u16.to_le_bytes());
    out.extend_from_slice(&(ifd_start as u32).to_le_bytes());

    for bytes in &encoded {
        out.extend_from_slice(bytes);
    }
    while (out.len() as u64) < ifd_start {
        out.push(0);
    }

    for i in 0..n {
        let strip_offset = (data_start + i * data_len) as u32;
        let next_ifd = if i + 1 < n {
            (ifd_start + (i + 1) * ifd_size) as u32
        } else {
            0
        };
        out.extend_from_slice(&num_tags.to_le_bytes());
        write_ifd_entry(&mut out, TAG_IMAGE_WIDTH, 4, 1, width);
        write_ifd_entry(&mut out, TAG_IMAGE_LENGTH, 4, 1, height);
        if samples_per_pixel == 3 {
            write_ifd_entry(&mut out, TAG_BITS_PER_SAMPLE, 3, 3, tag_data_offset as u32);
        } else {
            write_ifd_entry(&mut out, TAG_BITS_PER_SAMPLE, 3, 1, bits_per_sample as u32);
        }
        write_ifd_entry(&mut out, TAG_COMPRESSION, 3, 1, 1);
        write_ifd_entry(&mut out, TAG_PHOTO_INTERP, 3, 1, photo_interp as u32);
        if let Some(desc) = &description {
            write_ifd_entry(
                &mut out,
                TAG_IMAGE_DESCRIPTION,
                2,
                desc.len() as u32,
                description_offset as u32,
            );
        }
        write_ifd_entry(&mut out, TAG_STRIP_OFFSETS, 4, 1, strip_offset);
        write_ifd_entry(
            &mut out,
            TAG_SAMPLES_PER_PIXEL,
            3,
            1,
            samples_per_pixel as u32,
        );
        write_ifd_entry(&mut out, TAG_ROWS_PER_STRIP, 4, 1, height);
        write_ifd_entry(&mut out, TAG_STRIP_BYTE_COUNTS, 4, 1, data_len as u32);
        if scaled {
            // TiffEncoder.writeIFD (TiffEncoder.java 302-312) writes one
            // RATIONAL per axis plus a ResolutionUnit.
            write_ifd_entry(&mut out, TAG_X_RESOLUTION, 5, 1, scale_offset as u32);
            write_ifd_entry(&mut out, TAG_Y_RESOLUTION, 5, 1, (scale_offset + 8) as u32);
            write_ifd_entry(
                &mut out,
                TAG_RESOLUTION_UNIT,
                3,
                1,
                resolution_unit(doc) as u32,
            );
        }
        if palette.is_some() {
            write_ifd_entry(&mut out, TAG_COLOR_MAP, 3, 768, color_map_offset as u32);
        }
        write_ifd_entry(&mut out, TAG_SAMPLE_FORMAT, 3, 1, sample_format as u32);
        if let Some((counts, data)) = &metadata {
            // MetaDataByteCounts is a UINT32 array; MetaData is a byte blob.
            write_ifd_entry(
                &mut out,
                TAG_META_DATA_BYTE_COUNTS,
                4,
                (counts.len() / 4) as u32,
                metadata_counts_offset as u32,
            );
            write_ifd_entry(
                &mut out,
                TAG_META_DATA,
                1,
                data.len() as u32,
                metadata_offset as u32,
            );
        }
        out.extend_from_slice(&next_ifd.to_le_bytes());
    }

    if samples_per_pixel == 3 {
        out.extend_from_slice(&8u16.to_le_bytes());
        out.extend_from_slice(&8u16.to_le_bytes());
        out.extend_from_slice(&8u16.to_le_bytes());
    }
    if let Some(desc) = &description {
        out.extend_from_slice(desc);
        while (out.len() as u64) < color_map_offset {
            out.push(0);
        }
    }
    if let Some(lut) = palette {
        // TIFF ColorMap entries are 16-bit; ImageJ reads the high byte
        // (TiffDecoder.getColorMap, TiffDecoder.java lines 159-182), so
        // v*257 keeps both TIFF readers and ImageJ lossless.
        for channel in [&lut.r, &lut.g, &lut.b] {
            for &value in channel {
                out.extend_from_slice(&(u16::from(value) * 257).to_le_bytes());
            }
        }
    }
    if scaled {
        while (out.len() as u64) < scale_offset {
            out.push(0);
        }
        let (x_num, x_den, y_num, y_den) = resolution_rationals(doc);
        out.extend_from_slice(&x_num.to_le_bytes());
        out.extend_from_slice(&x_den.to_le_bytes());
        out.extend_from_slice(&y_num.to_le_bytes());
        out.extend_from_slice(&y_den.to_le_bytes());
    }
    if let Some((counts, data)) = &metadata {
        while (out.len() as u64) < metadata_counts_offset {
            out.push(0);
        }
        out.extend_from_slice(counts);
        while (out.len() as u64) < metadata_offset {
            out.push(0);
        }
        out.extend_from_slice(data);
    }

    write_atomic(path, &out)?;
    Ok(())
}

/// Names of in-flight temporary files use a hidden prefix plus a per-process
/// sequence number; `create_new` makes the selection exclusive.
static TEMP_FILE_SEQ: AtomicU64 = AtomicU64::new(0);

/// Number of final-symlink hops followed before giving up. Linux allows 40 and
/// macOS 32; a fixed limit also stops cycles.
const MAX_SYMLINK_HOPS: usize = 40;

/// Filesystem seam for the atomic writer. Production code uses [`RealFs`]; the
/// unit tests in this module substitute a wrapper that injects partial writes
/// and flush/rename failures at each stage.
trait AtomicFileSystem {
    fn symlink_metadata(&self, path: &Path) -> io::Result<fs::Metadata>;
    fn metadata(&self, path: &Path) -> io::Result<fs::Metadata>;
    fn read_link(&self, path: &Path) -> io::Result<PathBuf>;
    fn create_new(&self, path: &Path) -> io::Result<fs::File>;
    fn write_all(&self, file: &mut fs::File, bytes: &[u8]) -> io::Result<()>;
    fn sync_all(&self, file: &fs::File) -> io::Result<()>;
    fn set_permissions(&self, path: &Path, permissions: fs::Permissions) -> io::Result<()>;
    fn rename(&self, from: &Path, to: &Path) -> io::Result<()>;
    fn remove_file(&self, path: &Path) -> io::Result<()>;
}

struct RealFs;

impl AtomicFileSystem for RealFs {
    fn symlink_metadata(&self, path: &Path) -> io::Result<fs::Metadata> {
        fs::symlink_metadata(path)
    }

    fn metadata(&self, path: &Path) -> io::Result<fs::Metadata> {
        fs::metadata(path)
    }

    fn read_link(&self, path: &Path) -> io::Result<PathBuf> {
        fs::read_link(path)
    }

    fn create_new(&self, path: &Path) -> io::Result<fs::File> {
        fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)
    }

    fn write_all(&self, file: &mut fs::File, bytes: &[u8]) -> io::Result<()> {
        file.write_all(bytes)
    }

    fn sync_all(&self, file: &fs::File) -> io::Result<()> {
        file.sync_all()
    }

    fn set_permissions(&self, path: &Path, permissions: fs::Permissions) -> io::Result<()> {
        fs::set_permissions(path, permissions)
    }

    fn rename(&self, from: &Path, to: &Path) -> io::Result<()> {
        fs::rename(from, to)
    }

    fn remove_file(&self, path: &Path) -> io::Result<()> {
        fs::remove_file(path)
    }
}

/// Removes an uncommitted temporary file, including on an early `?` return or
/// panic. A cleanup failure is reported by [`Self::remove`] but is otherwise a
/// best-effort operation; the destination is untouched either way.
struct TempFileGuard<'a> {
    path: Option<PathBuf>,
    fs: &'a dyn AtomicFileSystem,
}

impl TempFileGuard<'_> {
    fn path(&self) -> &Path {
        self.path
            .as_deref()
            .expect("temporary path is present until disarmed")
    }

    fn remove(&mut self) -> io::Result<()> {
        match self.path.take() {
            Some(path) => self.fs.remove_file(&path),
            None => Ok(()),
        }
    }

    fn disarm(&mut self) {
        self.path = None;
    }
}

impl Drop for TempFileGuard<'_> {
    fn drop(&mut self) {
        if let Some(path) = self.path.take() {
            let _ = self.fs.remove_file(&path);
        }
    }
}

/// Writes `bytes` to `path` through a same-directory temporary file, then
/// renames it into place so a failed save never truncates the existing file.
///
/// # Guarantees
///
/// - Every failure before the final rename leaves the previous destination
///   byte-for-byte intact and removes the temporary file.
/// - On success the destination contains the complete encoding and keeps the
///   permission bits it had before the save (a new file gets the process
///   umask defaults).
/// - The replacement is a single `rename(2)`/`MoveFileExW` within one
///   directory, so a concurrent reader observes either the old file or the new
///   one, never a partial file.
///
/// # Intentional behavior
///
/// - A destination that is a symbolic link is followed and its target is
///   replaced, leaving the link itself in place.
/// - Replacing an existing file is authorized by write permission on the
///   containing directory, not on the old file (standard atomic-replace
///   semantics; callers own the overwrite decision).
///
/// # Limitations
///
/// - Hard links to the old inode keep the old contents.
/// - Ownership, ACLs and extended attributes are not copied. On Windows only
///   the read-only attribute is representable and is carried over.
/// - This is not a crash-durability guarantee. File contents are `fsync`ed.
///   On Unix the parent directory is `fsync`ed best-effort after the rename,
///   but a directory-sync failure is not reported because the rename has
///   already committed. Filesystems that reject directory sync, and Windows
///   (no directory fsync), may lose the rename across a power failure even
///   though `save_tiff` returned `Ok`.
fn write_atomic(path: &Path, bytes: &[u8]) -> io::Result<()> {
    write_atomic_with(&RealFs, path, bytes)
}

fn write_atomic_with(fs_impl: &dyn AtomicFileSystem, path: &Path, bytes: &[u8]) -> io::Result<()> {
    let destination = resolve_final_symlink(fs_impl, path)?;
    let existing_permissions = match fs_impl.metadata(&destination) {
        Ok(metadata) if metadata.is_dir() => {
            return Err(io::Error::new(
                io::ErrorKind::IsADirectory,
                format!("cannot save TIFF over directory {}", destination.display()),
            ));
        }
        Ok(metadata) if !metadata.is_file() => {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!(
                    "cannot save TIFF over non-regular file {}",
                    destination.display()
                ),
            ));
        }
        Ok(metadata) => Some(metadata.permissions()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => None,
        Err(error) => return Err(error),
    };

    let directory = destination
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let (temp_path, mut file) = create_temp_file(fs_impl, directory)?;
    let mut guard = TempFileGuard {
        path: Some(temp_path),
        fs: fs_impl,
    };

    let prepared = write_and_sync(
        fs_impl,
        &mut file,
        guard.path(),
        existing_permissions,
        bytes,
    );
    // Close the handle before rename/remove so Windows delete sharing is not
    // the deciding factor.
    drop(file);

    let outcome = match prepared {
        Ok(()) => {
            let temp_path = guard.path().to_path_buf();
            fs_impl.rename(&temp_path, &destination).map(|()| {
                // The rename already committed. A directory sync failure cannot
                // be rolled back, so it is intentionally not surfaced (see the
                // durability limitation above).
                let _ = sync_directory(directory);
            })
        }
        Err(error) => Err(error),
    };

    match outcome {
        Ok(()) => {
            guard.disarm();
            Ok(())
        }
        Err(error) => {
            let temp_display = guard.path().display().to_string();
            match guard.remove() {
                Ok(()) => Err(error),
                Err(cleanup) => Err(io::Error::new(
                    error.kind(),
                    format!(
                        "{error}; additionally failed to remove temporary file {temp_display}: {cleanup}"
                    ),
                )),
            }
        }
    }
}

fn write_and_sync(
    fs_impl: &dyn AtomicFileSystem,
    file: &mut fs::File,
    temp_path: &Path,
    existing_permissions: Option<fs::Permissions>,
    bytes: &[u8],
) -> io::Result<()> {
    fs_impl.write_all(file, bytes)?;
    if let Some(permissions) = existing_permissions {
        fs_impl.set_permissions(temp_path, permissions)?;
    }
    fs_impl.sync_all(file)
}

/// Resolves only the final path component while it is a symbolic link. Parent
/// components are resolved by the operating system when the temp file is
/// created, so the temp file always lands in the target's real directory.
fn resolve_final_symlink(fs_impl: &dyn AtomicFileSystem, path: &Path) -> io::Result<PathBuf> {
    let mut current = path.to_path_buf();
    for _ in 0..MAX_SYMLINK_HOPS {
        match fs_impl.symlink_metadata(&current) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                let target = fs_impl.read_link(&current)?;
                current = if target.is_absolute() {
                    target
                } else {
                    match current.parent() {
                        Some(parent) if !parent.as_os_str().is_empty() => parent.join(target),
                        _ => target,
                    }
                };
            }
            Ok(_) => return Ok(current),
            // A dangling symlink has a creatable target path.
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(current),
            Err(error) => return Err(error),
        }
    }
    Err(io::Error::other(format!(
        "too many levels of symbolic links resolving {}",
        path.display()
    )))
}

fn create_temp_file(
    fs_impl: &dyn AtomicFileSystem,
    directory: &Path,
) -> io::Result<(PathBuf, fs::File)> {
    for _ in 0..16 {
        let sequence = TEMP_FILE_SEQ.fetch_add(1, Ordering::Relaxed);
        let candidate = directory.join(format!(".ij-save-{}-{sequence}.tmp", std::process::id()));
        match fs_impl.create_new(&candidate) {
            Ok(file) => return Ok((candidate, file)),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        }
    }
    Err(io::Error::new(
        io::ErrorKind::AlreadyExists,
        format!(
            "could not create a unique temporary file in {}",
            directory.display()
        ),
    ))
}

/// Makes the directory entry durable where the platform supports it. Failures
/// are ignored because the rename has already replaced the destination.
#[cfg(unix)]
fn sync_directory(directory: &Path) -> io::Result<()> {
    fs::File::open(directory)?.sync_all()
}

#[cfg(not(unix))]
fn sync_directory(_directory: &Path) -> io::Result<()> {
    Ok(())
}

fn calibration_is_scaled(doc: &ImageDocument) -> bool {
    // Calibration.scaled(): any non-unit scale or a non-default unit
    // (original_imagej/ij/measure/Calibration.java lines 91-94).
    doc.calibration.pixel_width != 1.0
        || doc.calibration.pixel_height != 1.0
        || doc.calibration.pixel_depth != 1.0
        || doc.calibration.unit != "pixel"
}

fn resolution_unit(doc: &ImageDocument) -> u16 {
    // TiffEncoder.writeIFD maps only inch and cm to TIFF units 2 and 3;
    // every other unit is written through the ImageDescription `unit=` line.
    match doc.calibration.unit.as_str() {
        "inch" => 2,
        "cm" => 3,
        _ => 1,
    }
}

/// Mirrors TiffEncoder.writeScale (TiffEncoder.java 337-348), including the
/// truncating integer cast and 1e6 fixed-point scale.
fn resolution_rationals(doc: &ImageDocument) -> (u32, u32, u32, u32) {
    let xscale = 1.0 / doc.calibration.pixel_width;
    let yscale = 1.0 / doc.calibration.pixel_height;
    let mut scale = 1_000_000.0f64;
    if xscale * scale > i32::MAX as f64 || yscale * scale > i32::MAX as f64 {
        scale = (i32::MAX as f64 / xscale.max(yscale)) as i32 as f64;
    }
    (
        (xscale * scale) as i32 as u32,
        scale as i32 as u32,
        (yscale * scale) as i32 as u32,
        scale as i32 as u32,
    )
}

fn format_java_double(value: f64) -> String {
    if value.is_nan() {
        "NaN".into()
    } else if value == f64::INFINITY {
        "Infinity".into()
    } else if value == f64::NEG_INFINITY {
        "-Infinity".into()
    } else {
        format!("{value:?}")
    }
}

/// Builds the ImageDescription that FileSaver.getDescriptionString writes
/// (original_imagej/ij/io/FileSaver.java lines 714-808).
///
/// Metadata the Rust model cannot represent is rejected instead of being
/// dropped silently.
fn image_description(doc: &ImageDocument) -> Result<Option<String>, IoError> {
    let stack_size = doc.slices.len();
    let first_plane = &doc.slices[0];
    if stack_size == 1 && doc.calibration.pixel_depth != 1.0 {
        return Err(IoError::UnsupportedFormat(
            "ImageJ TIFF cannot store a non-default pixel depth (z-spacing) for a single-plane image; \
             ImageJ only writes `spacing=` for stacks (FileSaver.java 763-772)"
                .into(),
        ));
    }
    if matches!(first_plane, PixelPlane::ColorRgb(_))
        && (doc.display_range.min != 0.0 || doc.display_range.max != 255.0)
    {
        return Err(IoError::UnsupportedFormat(format!(
            "ImageJ TIFF cannot store a non-default display range for RGB images \
             (range {}..{} would be lost; FileSaver.java 775-784)",
            doc.display_range.min, doc.display_range.max
        )));
    }
    // FileOpener treats a stored 0/0 range as "not set" (FileOpener.java
    // 452-463). That is only lossless when the image-derived range is also
    // 0/0 (an all-zero 16/32-bit image).
    if doc.display_range.min == 0.0 && doc.display_range.max == 0.0 {
        let data_range = match first_plane {
            PixelPlane::Gray16(_) | PixelPlane::Gray32(_) => Some(auto_display_range(doc)),
            _ => None,
        };
        if data_range != Some(DisplayRange { min: 0.0, max: 0.0 }) {
            return Err(IoError::UnsupportedFormat(
                "ImageJ TIFF cannot store a 0/0 display range (FileOpener ignores it as \
                 \"not set\"); set a non-zero display range before saving"
                    .into(),
            ));
        }
    }

    let mut sb = String::from("ImageJ=1.54u\n");
    let mut has_metadata = false;
    if stack_size > 1 {
        sb.push_str(&format!("images={stack_size}\n"));
        has_metadata = true;
    }
    if doc.n_channels > 1 {
        sb.push_str(&format!("channels={}\n", doc.n_channels));
        has_metadata = true;
    }
    if doc.n_slices > 1 {
        sb.push_str(&format!("slices={}\n", doc.n_slices));
        has_metadata = true;
    }
    if doc.n_frames > 1 {
        sb.push_str(&format!("frames={}\n", doc.n_frames));
        has_metadata = true;
    }
    if doc.is_hyperstack() {
        sb.push_str("hyperstack=true\n");
        has_metadata = true;
    }

    let cal = &doc.calibration;
    if calibration_is_scaled(doc) {
        validate_scaled_calibration(doc)?;
        // FileSaver writes cal.getUnit(), which Calibration.setUnit has
        // already canonicalized (Calibration.java 128-137).
        append_escaped_line(&mut sb, &format!("unit={}", sanitize_unit(&cal.unit)));
        has_metadata = true;
    }
    if stack_size > 1 && cal.pixel_depth != 1.0 {
        sb.push_str(&format!("spacing={:?}\n", cal.pixel_depth));
        has_metadata = true;
    }
    if stack_size > 1 {
        // FileSaver always writes the loop flag for stacks (FileSaver.java
        // 763-772). Calibration.loop is not part of the Rust model, and
        // ImageJ never animates in batch mode, so the default false is exact.
        sb.push_str("loop=false\n");
        has_metadata = true;
    }

    let write_display_range = match first_plane {
        // `enhancedLut`: 8-bit and palette images only store a custom range.
        PixelPlane::Gray8(_) | PixelPlane::Color256 { .. } => {
            doc.display_range.min != 0.0 || doc.display_range.max != 255.0
        }
        PixelPlane::Gray16(_) | PixelPlane::Gray32(_) => true,
        PixelPlane::ColorRgb(_) => false,
    };
    if write_display_range {
        sb.push_str(&format!(
            "min={}\n",
            format_java_double(doc.display_range.min)
        ));
        sb.push_str(&format!(
            "max={}\n",
            format_java_double(doc.display_range.max)
        ));
        has_metadata = true;
    }
    if cal.invert_y {
        sb.push_str("inverty=true\n");
        has_metadata = true;
    }
    if matches!(first_plane, PixelPlane::Color256 { .. }) {
        // FileSaver marks palette images so FileOpener calls
        // setTypeToColor256 (FileSaver.java 803-804, FileOpener.java 465-466).
        sb.push_str("8bitcolor=true\n");
        has_metadata = true;
    }

    Ok(if has_metadata { Some(sb) } else { None })
}

fn validate_scaled_calibration(doc: &ImageDocument) -> Result<(), IoError> {
    let cal = &doc.calibration;
    for (name, value) in [
        ("pixel width", cal.pixel_width),
        ("pixel height", cal.pixel_height),
        ("pixel depth", cal.pixel_depth),
    ] {
        if !value.is_finite() || value <= 0.0 {
            return Err(IoError::UnsupportedFormat(format!(
                "calibration {name} must be finite and positive to write TIFF resolution tags, got {value}"
            )));
        }
    }
    Ok(())
}

/// Mirrors FileSaver.appendEscapedLine (FileSaver.java 810-823): printable
/// ASCII passes through, everything else becomes a Java Properties `\uXXXX`
/// escape so `Properties.load` decodes the same bytes ImageJ wrote.
fn append_escaped_line(out: &mut String, line: &str) {
    for c in line.chars() {
        let code = c as u32;
        if (0x20..0x7f).contains(&code) && c != '\\' {
            out.push(c);
        } else {
            let mut units = [0u16; 2];
            for unit in c.encode_utf16(&mut units) {
                out.push_str(&format!("\\u{unit:04X}"));
            }
        }
    }
    out.push('\n');
}

struct ParsedIfd {
    width: u32,
    height: u32,
    plane: PixelPlane,
    next_ifd: u32,
    description: Option<String>,
    x_resolution: Option<f64>,
    y_resolution: Option<f64>,
    resolution_unit: Option<u16>,
    data_bytes: usize,
    data_offset: usize,
    orientation: bool,
    compressed: bool,
    format: PlaneFormat,
    metadata: Vec<ImageJMetadata>,
}

fn parse_ifd(
    bytes: &[u8],
    ifd_offset: usize,
    le: bool,
    data_budget: usize,
    decoded_budget: usize,
) -> Result<ParsedIfd, IoError> {
    let num_entries = read_u16(bytes, ifd_offset, le)? as usize;

    let mut width: Option<u32> = None;
    let mut height: Option<u32> = None;
    let mut bits_per_sample: u16 = 8;
    let mut compression: u16 = 1;
    let mut horizontal_predictor = false;
    let mut rows_per_strip: Option<u32> = None;
    let mut samples_per_pixel: u16 = 1;
    let mut sample_format: u16 = 1;
    let mut photometric: Option<u16> = None;
    let mut planar_config: u16 = 1;
    let mut color_map: Option<Box<Lut256>> = None;
    let mut strip_offsets: Vec<u32> = Vec::new();
    let mut strip_byte_counts: Vec<u32> = Vec::new();
    let mut description: Option<String> = None;
    let mut x_resolution: Option<f64> = None;
    let mut y_resolution: Option<f64> = None;
    let mut resolution_unit: Option<u16> = None;
    let mut orientation = false;
    let mut meta_data_counts: Option<Vec<u32>> = None;
    let mut meta_data: Option<(usize, usize)> = None;

    for i in 0..num_entries {
        let entry_pos = ifd_offset
            .checked_add(2)
            .and_then(|base| i.checked_mul(12).and_then(|off| base.checked_add(off)))
            .ok_or_else(|| IoError::Malformed("IFD entry offset overflow".into()))?;
        let entry_end = entry_pos
            .checked_add(12)
            .ok_or_else(|| IoError::Malformed("IFD entry offset overflow".into()))?;
        if entry_end > bytes.len() {
            return Err(IoError::Malformed(
                "IFD entry extends beyond end of file".into(),
            ));
        }
        let tag = read_u16(bytes, entry_pos, le)?;
        let field_type = read_u16(bytes, entry_pos + 2, le)?;
        let count = read_u32(bytes, entry_pos + 4, le)? as usize;
        let val_pos = entry_pos + 8;

        match tag {
            TAG_IMAGE_WIDTH => width = Some(read_tag_u32(bytes, field_type, val_pos, le)?),
            TAG_IMAGE_LENGTH => height = Some(read_tag_u32(bytes, field_type, val_pos, le)?),
            TAG_BITS_PER_SAMPLE => {
                if count == 0 || count > 4 {
                    return Err(IoError::Malformed(format!(
                        "BitsPerSample has {count} components"
                    )));
                }
                if count == 1 {
                    bits_per_sample =
                        read_tag_u16(bytes, field_type, "BitsPerSample", val_pos, le)?;
                } else {
                    let ext_pos = read_u32(bytes, val_pos, le)? as usize;
                    let ext_end = count
                        .checked_mul(2)
                        .and_then(|span| ext_pos.checked_add(span))
                        .ok_or_else(|| {
                            IoError::Malformed("BitsPerSample extent overflow".into())
                        })?;
                    if ext_end > bytes.len() {
                        return Err(IoError::Malformed(
                            "BitsPerSample extends beyond end of file".into(),
                        ));
                    }
                    bits_per_sample = read_u16(bytes, ext_pos, le)?;
                    for component in 1..count {
                        let value = read_u16(bytes, ext_pos + component * 2, le)?;
                        if value != bits_per_sample {
                            return Err(IoError::UnsupportedFormat(
                                "TIFF with differing BitsPerSample components".into(),
                            ));
                        }
                    }
                }
            }
            TAG_COMPRESSION => {
                compression = read_tag_u16(bytes, field_type, "Compression", val_pos, le)?
            }
            TAG_PREDICTOR => {
                let value = read_tag_u16(bytes, field_type, "Predictor", val_pos, le)?;
                // TiffDecoder.java 526-532 enables differencing only after
                // encountering LZW/ZIP, and ignores it for PackBits/none.
                horizontal_predictor = value == 2 && matches!(compression, 5 | 8 | 32946);
            }
            TAG_ROWS_PER_STRIP => {
                rows_per_strip = Some(read_tag_u32(bytes, field_type, val_pos, le)?);
            }
            TAG_PHOTO_INTERP => {
                photometric = Some(read_tag_u16(
                    bytes,
                    field_type,
                    "PhotometricInterpretation",
                    val_pos,
                    le,
                )?)
            }
            TAG_SAMPLES_PER_PIXEL => {
                samples_per_pixel = read_tag_u16(bytes, field_type, "SamplesPerPixel", val_pos, le)?
            }
            TAG_SAMPLE_FORMAT => {
                sample_format = read_tag_u16(bytes, field_type, "SampleFormat", val_pos, le)?
            }
            TAG_PLANAR_CONFIG => {
                planar_config = read_tag_u16(bytes, field_type, "PlanarConfiguration", val_pos, le)?
            }
            TAG_COLOR_MAP => {
                if field_type != 3 || count != 768 {
                    return Err(IoError::UnsupportedFormat(format!(
                        "ColorMap with type {field_type} and {count} entries; only 8-bit 768-entry palettes are supported"
                    )));
                }
                let start = read_u32(bytes, val_pos, le)? as usize;
                color_map = Some(read_color_map(bytes, start, le)?);
            }
            TAG_IMAGE_DESCRIPTION => {
                let start = if count <= 4 {
                    val_pos
                } else {
                    read_u32(bytes, val_pos, le)? as usize
                };
                let end = start
                    .checked_add(count)
                    .ok_or_else(|| IoError::Malformed("ImageDescription extent overflow".into()))?;
                if end > bytes.len() {
                    return Err(IoError::Malformed(
                        "ImageDescription extends beyond end of file".into(),
                    ));
                }
                let mut text = String::from_utf8_lossy(&bytes[start..end]).into_owned();
                while text.ends_with('\0') {
                    text.pop();
                }
                description = Some(text);
            }
            TAG_STRIP_OFFSETS => {
                strip_offsets = read_tag_u32_vec(bytes, field_type, count, val_pos, le)?;
            }
            TAG_STRIP_BYTE_COUNTS => {
                strip_byte_counts = read_tag_u32_vec(bytes, field_type, count, val_pos, le)?;
            }
            TAG_X_RESOLUTION => {
                x_resolution = read_tiff_rational(bytes, field_type, count, val_pos, le)?
            }
            TAG_Y_RESOLUTION => {
                y_resolution = read_tiff_rational(bytes, field_type, count, val_pos, le)?
            }
            TAG_RESOLUTION_UNIT => {
                resolution_unit = Some(read_tag_u16(
                    bytes,
                    field_type,
                    "ResolutionUnit",
                    val_pos,
                    le,
                )?)
            }
            TAG_ORIENTATION => {
                // TiffDecoder sets fi.nImages=0 when it sees Orientation, so
                // an `images=` description must not be trusted (TiffDecoder.java
                // 560-563).
                orientation = true;
            }
            TAG_TILE_WIDTH | TAG_TILE_LENGTH => {
                return Err(IoError::UnsupportedFormat(
                    "tiled TIFFs are not supported (ImageJ reports \"Cannot open tiled TIFFs\")"
                        .into(),
                ));
            }
            TAG_META_DATA_BYTE_COUNTS => {
                meta_data_counts = Some(read_tag_u32_vec(bytes, field_type, count, val_pos, le)?);
            }
            TAG_META_DATA => {
                if count <= 4 {
                    return Err(IoError::Malformed(
                        "ImageJ metadata payload is stored inline".into(),
                    ));
                }
                meta_data = Some((read_u32(bytes, val_pos, le)? as usize, count));
            }
            _ => {}
        }
    }

    let compression = TiffCompression::from_tag(compression)?;
    let compressed = compression.is_compressed();
    if horizontal_predictor && !matches!((bits_per_sample, samples_per_pixel), (8, 1 | 3) | (16, 1))
    {
        return Err(IoError::UnsupportedFormat(
            "TIFF Predictor=2 requires 8-bit grayscale/RGB or 16-bit grayscale".into(),
        ));
    }

    let metadata = match (meta_data_counts, meta_data) {
        (None, None) => Vec::new(),
        (Some(counts), Some((offset, count))) => {
            parse_imagej_metadata(bytes, &counts, offset, count, le)?
        }
        _ => {
            return Err(IoError::UnsupportedFormat(
                "incomplete ImageJ private metadata tags (MetaDataByteCounts/MetaData)".into(),
            ));
        }
    };

    let mut photo_interp = photometric.unwrap_or(1);
    if planar_config != 1 && planar_config != 2 {
        return Err(IoError::UnsupportedFormat(format!(
            "PlanarConfiguration={planar_config}"
        )));
    }
    // Chunky (1) is the only layout this reader decodes. Planar data with a
    // single sample is identical to chunky, so only reject multi-sample data.
    if planar_config == 2 && samples_per_pixel != 1 {
        return Err(IoError::UnsupportedFormat(
            "planar (PlanarConfiguration=2) RGB TIFFs are not supported".into(),
        ));
    }
    if photo_interp == 3 {
        if bits_per_sample != 8 || samples_per_pixel != 1 {
            return Err(IoError::UnsupportedFormat(
                "palette TIFFs are only supported as 8-bit single-channel images".into(),
            ));
        }
        if color_map.is_none() {
            return Err(IoError::UnsupportedFormat(
                "palette TIFF is missing a 768-entry ColorMap".into(),
            ));
        }
    } else if color_map.is_some() {
        // TiffDecoder reads a ColorMap for every PhotometricInterpretation
        // (TiffDecoder.java 536) and FileOpener.createColorModel installs it as
        // the processor's display LUT whenever lutSize > 0
        // (FileOpener.java 509-514).
        let grayscale_8bit = bits_per_sample == 8
            && samples_per_pixel == 1
            && sample_format == 1
            && photo_interp <= 1;
        // TiffDecoder discards the LUT for GRAY16_UNSIGNED files without an
        // ImageDescription (TiffDecoder.java 850-851).
        let ignored_gray16 = bits_per_sample == 16
            && samples_per_pixel == 1
            && sample_format == 1
            && photo_interp <= 1
            && !description_keeps_color_map(description.as_deref());
        if grayscale_8bit {
            // getColorMap promotes GRAY8 to COLOR8 only when the signed byte
            // sum is non-zero (TiffDecoder.java 181-182), but the palette is
            // installed either way, so Color256 preserves the display LUT.
            photo_interp = 3;
        } else if ignored_gray16 {
            color_map = None;
        } else {
            return Err(IoError::UnsupportedFormat(format!(
                "ColorMap display LUT on a {bits_per_sample}-bit PhotometricInterpretation={photo_interp} image cannot be represented; ImageJ applies it as the processor LUT (FileOpener.createColorModel)"
            )));
        }
    }

    let w = width.ok_or_else(|| IoError::UnsupportedFormat("missing ImageWidth".into()))?;
    let h = height.ok_or_else(|| IoError::UnsupportedFormat("missing ImageLength".into()))?;
    if w == 0 || h == 0 {
        return Err(IoError::Malformed(
            "image width and height must be positive".into(),
        ));
    }
    if strip_offsets.is_empty() {
        return Err(IoError::UnsupportedFormat("missing StripOffsets".into()));
    }
    if bits_per_sample == 0 || bits_per_sample % 8 != 0 {
        return Err(IoError::UnsupportedFormat(format!(
            "BitsPerSample={bits_per_sample}"
        )));
    }

    let pixel_count = u64::from(w)
        .checked_mul(u64::from(h))
        .ok_or_else(|| IoError::TooLarge("image dimension product overflow".into()))?;
    let bytes_per_pixel = (u64::from(bits_per_sample) / 8)
        .checked_mul(u64::from(samples_per_pixel))
        .ok_or_else(|| IoError::TooLarge("bytes-per-pixel overflow".into()))?;
    let expected_bytes = pixel_count
        .checked_mul(bytes_per_pixel)
        .ok_or_else(|| IoError::TooLarge("image data size overflow".into()))?;
    let expected_bytes = usize::try_from(expected_bytes)
        .map_err(|_| IoError::TooLarge("image data size exceeds addressable memory".into()))?;
    let stored_bytes = if compressed {
        strip_byte_counts.iter().try_fold(0usize, |sum, &count| {
            sum.checked_add(count as usize)
                .ok_or_else(|| IoError::TooLarge("compressed strip sizes overflow".into()))
        })?
    } else {
        expected_bytes
    };
    if (!compressed && expected_bytes > bytes.len())
        || stored_bytes > data_budget
        || (compressed && expected_bytes > decoded_budget)
    {
        return Err(IoError::TooLarge(format!(
            "image requires {expected_bytes} decoded bytes and {stored_bytes} stored bytes; remaining budgets are {decoded_budget} decoded and {data_budget} stored in the {} byte file",
            bytes.len()
        )));
    }
    let pixel_count = usize::try_from(pixel_count)
        .map_err(|_| IoError::TooLarge("pixel count exceeds addressable memory".into()))?;

    let row_bytes = expected_bytes / h as usize;
    let rows_per_strip = rows_per_strip.unwrap_or(h) as usize;
    if compressed
        && (rows_per_strip == 0
            || strip_offsets.len() != (h as usize).div_ceil(rows_per_strip)
            || strip_byte_counts.len() != strip_offsets.len())
    {
        return Err(IoError::Malformed(
            "compressed TIFF requires matching strip offsets/counts and nonzero RowsPerStrip"
                .into(),
        ));
    }
    let mut raw = Vec::new();
    for (idx, &offset) in strip_offsets.iter().enumerate() {
        if raw.len() >= expected_bytes {
            break;
        }
        let start = offset as usize;
        if start > bytes.len() {
            return Err(IoError::Malformed(format!(
                "StripOffsets[{idx}]={offset} is beyond the end of the file"
            )));
        }
        // Never copy more than the image needs: a malformed StripByteCounts
        // entry must not drive an unbounded allocation. Still reject a declared
        // strip that runs past the file, even if only part of it is needed.
        let remaining = expected_bytes - raw.len();
        let requested = strip_byte_counts
            .get(idx)
            .map_or(remaining, |&c| c as usize);
        let declared_end = start
            .checked_add(requested)
            .ok_or_else(|| IoError::Malformed("strip extent overflow".into()))?;
        if declared_end > bytes.len() {
            return Err(IoError::Malformed(format!(
                "strip {idx} extends beyond the end of the file"
            )));
        }
        if compressed {
            let rows = rows_per_strip.min((h as usize) - raw.len() / row_bytes);
            let mut strip =
                decompress_tiff_strip(&bytes[start..declared_end], compression, rows * row_bytes)?;
            if horizontal_predictor {
                undo_tiff_predictor(
                    &mut strip,
                    w as usize,
                    bits_per_sample,
                    samples_per_pixel,
                    le,
                );
            }
            raw.extend_from_slice(&strip);
        } else {
            let take = requested.min(remaining);
            raw.extend_from_slice(&bytes[start..start + take]);
        }
    }
    if raw.len() < expected_bytes {
        return Err(IoError::Malformed(format!(
            "strip data is short: got {} of {expected_bytes} pixel bytes",
            raw.len()
        )));
    }

    let format = PlaneFormat {
        bits_per_sample,
        samples_per_pixel,
        sample_format,
        photometric: photo_interp,
        palette: color_map,
        pixel_count,
        plane_bytes: expected_bytes,
    };
    let plane = format.decode(&raw, le)?;
    let data_offset = strip_offsets[0] as usize;

    let next_pos = ifd_offset
        .checked_add(2)
        .and_then(|base| {
            num_entries
                .checked_mul(12)
                .and_then(|off| base.checked_add(off))
        })
        .ok_or_else(|| IoError::Malformed("IFD next-pointer overflow".into()))?;
    let next_ifd = read_u32(bytes, next_pos, le)?;
    Ok(ParsedIfd {
        width: w,
        height: h,
        plane,
        next_ifd,
        description,
        x_resolution,
        y_resolution,
        resolution_unit,
        data_bytes: stored_bytes,
        data_offset,
        orientation,
        compressed,
        format,
        metadata,
    })
}

/// Decodes the ImageJ private metadata block (TiffDecoder.getMetaData,
/// TiffDecoder.java 596-700). Every declared length is validated against both
/// the tag extent and the file before copying, so a corrupt byte-count array
/// cannot allocate or read out of bounds.
fn parse_imagej_metadata(
    bytes: &[u8],
    counts: &[u32],
    data_offset: usize,
    data_count: usize,
    le: bool,
) -> Result<Vec<ImageJMetadata>, IoError> {
    let Some(&header_size) = counts.first() else {
        return Ok(Vec::new());
    };
    let header_size = header_size as usize;
    if !(12..=804).contains(&header_size) {
        return Err(IoError::Malformed(format!(
            "ImageJ metadata header size {header_size} is out of range"
        )));
    }
    let data_end = data_offset
        .checked_add(data_count)
        .ok_or_else(|| IoError::Malformed("ImageJ metadata extent overflow".into()))?;
    if data_end > bytes.len() {
        return Err(IoError::Malformed(
            "ImageJ metadata extends beyond end of file".into(),
        ));
    }
    let header_end = data_offset
        .checked_add(header_size)
        .ok_or_else(|| IoError::Malformed("ImageJ metadata header overflow".into()))?;
    if header_end > data_end {
        return Err(IoError::Malformed(
            "ImageJ metadata header is larger than the tag payload".into(),
        ));
    }
    if read_u32(bytes, data_offset, le)? != META_MAGIC {
        return Ok(Vec::new());
    }
    let type_count = (header_size - 4) / 8;
    let mut kinds = Vec::with_capacity(type_count);
    let mut entry_counts = Vec::with_capacity(type_count);
    for i in 0..type_count {
        let pos = data_offset + 4 + i * 8;
        kinds.push(read_u32(bytes, pos, le)?);
        entry_counts.push(read_u32(bytes, pos + 4, le)? as usize);
    }
    let total_entries = entry_counts
        .iter()
        .try_fold(0usize, |total, &count| total.checked_add(count));
    let total_entries = total_entries
        .ok_or_else(|| IoError::TooLarge("ImageJ metadata entry count overflow".into()))?;
    if counts.len() < total_entries + 1 {
        return Err(IoError::Malformed(format!(
            "ImageJ metadata declares {total_entries} entries but has only {} byte counts",
            counts.len() - 1
        )));
    }

    let mut position = header_end;
    let mut count_index = 1usize;
    let mut entries = Vec::with_capacity(type_count);
    for (kind, entry_count) in kinds.into_iter().zip(entry_counts) {
        let mut payloads = Vec::with_capacity(entry_count);
        for _ in 0..entry_count {
            let length = counts[count_index] as usize;
            count_index += 1;
            let end = position
                .checked_add(length)
                .ok_or_else(|| IoError::Malformed("ImageJ metadata entry overflow".into()))?;
            if end > data_end {
                return Err(IoError::Malformed(
                    "ImageJ metadata entry extends beyond the tag payload".into(),
                ));
            }
            payloads.push(bytes[position..end].to_vec());
            position = end;
        }
        entries.push(ImageJMetadata { kind, payloads });
    }
    Ok(entries)
}

/// Reads one TIFF RATIONAL (TiffDecoder.getRational, TiffDecoder.java
/// 352-361). Other field types are ignored, matching ImageJ's last-value-wins
/// style without inventing a value.
fn read_tiff_rational(
    bytes: &[u8],
    field_type: u16,
    count: usize,
    val_pos: usize,
    le: bool,
) -> Result<Option<f64>, IoError> {
    if field_type != 5 || count != 1 {
        return Ok(None);
    }
    let loc = read_u32(bytes, val_pos, le)? as usize;
    let numerator = read_u32(bytes, loc, le)? as f64;
    let denominator = read_u32(bytes, loc + 4, le)? as f64;
    Ok(Some(if denominator != 0.0 {
        numerator / denominator
    } else {
        0.0
    }))
}

/// True when TiffDecoder keeps a ColorMap for a GRAY16_UNSIGNED file. A decoded
/// description of at least seven characters makes `FileInfo.description`
/// non-null (TiffDecoder.saveImageDescription, TiffDecoder.java 201-207), which
/// disables the `lutSize = 0` fallback at TiffDecoder.java 850-851.
fn description_keeps_color_map(description: Option<&str>) -> bool {
    description.is_some_and(|text| text.encode_utf16().count() >= 7)
}

fn read_color_map(bytes: &[u8], start: usize, le: bool) -> Result<Box<Lut256>, IoError> {
    const ENTRIES: usize = 256;
    let end = start
        .checked_add(ENTRIES * 3 * 2)
        .ok_or_else(|| IoError::Malformed("ColorMap offset overflow".into()))?;
    if end > bytes.len() {
        return Err(IoError::Malformed(
            "ColorMap extends beyond end of file".into(),
        ));
    }
    let mut lut = Box::new(Lut256 {
        r: Vec::with_capacity(ENTRIES),
        g: Vec::with_capacity(ENTRIES),
        b: Vec::with_capacity(ENTRIES),
    });
    for i in 0..ENTRIES {
        // ImageJ maps a 16-bit TIFF entry to the high byte
        // (TiffDecoder.getColorMap, TiffDecoder.java lines 159-182).
        lut.r.push((read_u16(bytes, start + i * 2, le)? >> 8) as u8);
        lut.g
            .push((read_u16(bytes, start + 512 + i * 2, le)? >> 8) as u8);
        lut.b
            .push((read_u16(bytes, start + 1024 + i * 2, le)? >> 8) as u8);
    }
    Ok(lut)
}

/// Reads a strip-based TIFF, including PackBits, LZW and zlib Deflate, and
/// restores the metadata that `save_tiff` writes. LZW/Deflate Predictor=2 is
/// supported for 8-bit grayscale/palette/RGB and 16-bit unsigned grayscale.
/// Compressed stacks share a 512 MiB decoded pixel budget.
///
/// An `ImageDescription` starting with `ImageJ` is decoded as Java Properties
/// and supplies `min`/`max` (display range), `unit`, `spacing`, `inverty`,
/// `images`/`channels`/`slices`/`frames`, and `hyperstack`. XResolution and
/// YResolution supply pixel width/height only when a unit is defined, matching
/// `FileOpener.setCalibration` (`FileOpener.java` 390-414). Units are
/// normalized like `Calibration.setUnit` (`um` becomes `µm`, `A` becomes `Å`).
///
/// A single-IFD file whose description declares `images=N` is read as ImageJ
/// does: N contiguous planes starting at the first strip offset. A mismatch
/// between the declared stack size and the IFD/plane layout is an explicit
/// error rather than a truncated image.
///
/// A 768-entry TIFF `ColorMap` is interpreted through `TiffDecoder.getColorMap`
/// and `FileOpener.createColorModel`: an 8-bit grayscale (Photometric 0/1)
/// image becomes `Color256` carrying the LUT, a GRAY16_UNSIGNED image without
/// an ImageDescription ignores it exactly like Java's `lutSize = 0` fallback,
/// and a LUT that ImageJ would apply to a 16-bit (with description) or 32-bit
/// display is rejected because those planes have no LUT field.
///
/// Metadata that `ImageDocument` cannot represent is rejected instead of being
/// dropped: custom channel LUTs (`luts`), non-uniform per-channel display
/// ranges, image Info/plot metadata, overlay/property metadata,
/// and origin/time/yunit/zunit/density calibration keys. A uniform per-channel
/// display range is restored. A single `roi ` record is restored for rectangle,
/// oval, polygon and line selections with exact integral coordinates; other
/// ROI types, fractional subpixel coordinates and presentation/position
/// metadata are rejected explicitly. The composite `mode=` flag is not
/// modelled. A TIFF without stored min/max for GRAY16/GRAY32 opens with
/// ImageJ's image-derived range rather than the constructor default.
pub fn load_tiff(path: impl AsRef<Path>) -> Result<ImageDocument, IoError> {
    let path = path.as_ref();
    if !fs::metadata(path)?.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "TIFF input must be a regular file",
        )
        .into());
    }
    let bytes = fs::read(path)?;
    if bytes.len() < 8 {
        return Err(IoError::InvalidHeader);
    }
    let le = match &bytes[0..2] {
        b"II" => true,
        b"MM" => false,
        _ => return Err(IoError::InvalidHeader),
    };
    if read_u16(&bytes, 2, le)? != 42 {
        return Err(IoError::InvalidHeader);
    }

    let mut ifd_offset = read_u32(&bytes, 4, le)? as usize;
    let mut width: Option<u32> = None;
    let mut height: Option<u32> = None;
    let mut planes: Vec<PixelPlane> = Vec::new();
    let mut data_offsets: Vec<usize> = Vec::new();
    let mut visited: Vec<usize> = Vec::new();
    let mut description: Option<String> = None;
    let mut x_resolution: Option<f64> = None;
    let mut y_resolution: Option<f64> = None;
    let mut resolution_unit: Option<u16> = None;
    let mut first_ifd = true;
    let mut first_orientation = false;
    let mut first_compressed = false;
    let mut first_format: Option<PlaneFormat> = None;
    let mut first_data_offset = 0usize;
    let mut first_metadata: Vec<ImageJMetadata> = Vec::new();
    // Stored bytes must fit in the file across the whole stack. A separate
    // decoded budget bounds expansion and repeated compressed IFD references.
    let mut remaining_data = bytes.len();
    let mut remaining_decoded = MAX_DECODED_TIFF_BYTES;

    while ifd_offset != 0 {
        if visited.contains(&ifd_offset) {
            return Err(IoError::Malformed("cyclic TIFF IFD chain".into()));
        }
        visited.push(ifd_offset);
        let ifd = parse_ifd(&bytes, ifd_offset, le, remaining_data, remaining_decoded)?;
        remaining_data -= ifd.data_bytes;
        if ifd.compressed {
            remaining_decoded -= ifd.format.plane_bytes;
        }
        if first_ifd {
            // TiffDecoder decodes IMAGE_DESCRIPTION only while ifdCount==1
            // (TiffDecoder.java 555-559), so a later IFD's description must
            // not supply display range, C/Z/T or calibration metadata.
            description = ifd.description.clone();
            x_resolution = ifd.x_resolution;
            y_resolution = ifd.y_resolution;
            resolution_unit = ifd.resolution_unit;
            first_orientation = ifd.orientation;
            first_compressed = ifd.compressed;
            first_format = Some(ifd.format.clone());
            first_data_offset = ifd.data_offset;
            first_metadata = ifd.metadata;
            first_ifd = false;
        }
        match width {
            Some(w) if w != ifd.width => {
                return Err(IoError::UnsupportedFormat(
                    "stack slices have different widths".into(),
                ));
            }
            None => width = Some(ifd.width),
            _ => {}
        }
        match height {
            Some(h) if h != ifd.height => {
                return Err(IoError::UnsupportedFormat(
                    "stack slices have different heights".into(),
                ));
            }
            None => height = Some(ifd.height),
            _ => {}
        }
        data_offsets.push(ifd.data_offset);
        planes.push(ifd.plane);
        ifd_offset = ifd.next_ifd as usize;
    }

    let w = width.ok_or_else(|| IoError::UnsupportedFormat("no TIFF IFDs".into()))?;
    let h = height.ok_or_else(|| IoError::UnsupportedFormat("no TIFF IFDs".into()))?;

    // ImageJ builds a stack only when every IFD has the same
    // FileInfo.fileType (Opener.allSameSizeAndType, Opener.java 794-819) and
    // otherwise opens just the first image (Opener.openTiff2, Opener.java
    // 1144-1150). A per-slice mix of palette and grayscale slices is therefore
    // not a stack in ImageJ, so refuse it instead of inventing one.
    let first_is_color256 = matches!(planes.first(), Some(PixelPlane::Color256 { .. }));
    if planes
        .iter()
        .skip(1)
        .any(|plane| matches!(plane, PixelPlane::Color256 { .. }) != first_is_color256)
    {
        return Err(IoError::UnsupportedFormat(
            "stack mixes grayscale and ColorMap palette slices; ImageJ opens such a TIFF as its first image only".into(),
        ));
    }
    // ImageJ builds one ImageStack with the ColorModel from the first FileInfo,
    // so a multi-IFD palette stack is displayed through IFD1's LUT even when a
    // later IFD stores a different ColorMap (Opener.openTiffStack, Opener.java
    // 824-825; FileOpener.openStack, FileOpener.java 246-247). Rust preserves
    // every slice's LUT and save_tiff rejects mixed palettes, so accepting this
    // file would silently keep per-slice LUTs that ImageJ never uses.
    if let Some(PixelPlane::Color256 { lut: first_lut, .. }) = planes.first() {
        for (index, plane) in planes.iter().enumerate().skip(1) {
            if let PixelPlane::Color256 { lut, .. } = plane {
                if lut != first_lut {
                    return Err(IoError::UnsupportedFormat(format!(
                        "stack slice {} has a different ColorMap palette than slice 1; ImageJ applies the first IFD LUT to every slice",
                        index + 1
                    )));
                }
            }
        }
    }

    // Only an ImageJ description is decoded (FileOpener.decodeDescriptionString
    // returns null for anything else, FileOpener.java 605-611).
    let props = description
        .as_deref()
        .filter(|desc| desc.starts_with("ImageJ"))
        .map(parse_java_properties);
    let declared_images = props
        .as_ref()
        .and_then(|props| property_u32(props, "images"))
        .filter(|&images| images > 1);

    // TiffDecoder.saveImageDescription trusts `images=N` only for
    // uncompressed files, and OpenIFD drops that trust when Orientation is
    // present (TiffDecoder.java 205-216, 560-563). FileInfo.nImages then makes
    // ImageJ ignore the rest of the IFD chain and read N contiguous planes
    // from the first strip offset (Opener.java 795-813, FileOpener.openStack).
    if let Some(images) = declared_images.filter(|_| !first_orientation && !first_compressed) {
        let images = images as usize;
        if planes.len() > 1 && planes.len() != images {
            return Err(IoError::UnsupportedFormat(format!(
                "ImageDescription declares images={images} but the TIFF IFD chain contains {} planes",
                planes.len()
            )));
        }
        let format = first_format
            .as_ref()
            .ok_or_else(|| IoError::Malformed("missing first IFD format".into()))?;
        if planes.len() == 1 && images > 1 {
            if format.plane_bytes == 0 {
                return Err(IoError::UnsupportedFormat(
                    "ImageDescription declares a stack but the TIFF plane has zero bytes".into(),
                ));
            }
            let total = images
                .checked_mul(format.plane_bytes)
                .ok_or_else(|| IoError::TooLarge("contiguous stack size overflow".into()))?;
            let end = first_data_offset
                .checked_add(total)
                .ok_or_else(|| IoError::TooLarge("contiguous stack extent overflow".into()))?;
            if end > bytes.len() {
                return Err(IoError::TooLarge(format!(
                    "ImageDescription declares images={images} ({} pixel bytes) but the file only has {} bytes after offset {first_data_offset}",
                    total,
                    bytes.len().saturating_sub(first_data_offset)
                )));
            }
            let mut decoded = Vec::with_capacity(images);
            for index in 0..images {
                let start = first_data_offset + index * format.plane_bytes;
                let end = start + format.plane_bytes;
                decoded.push(format.decode(&bytes[start..end], le)?);
            }
            planes = decoded;
        } else if planes.len() == images {
            // ImageJ reads the stack contiguously even when every IFD carries
            // its own strip offset; refuse a layout ImageJ would silently
            // misread instead of trusting the individual IFD offsets.
            for (index, &offset) in data_offsets.iter().enumerate().skip(1) {
                let expected = first_data_offset
                    .checked_add(index * format.plane_bytes)
                    .ok_or_else(|| IoError::TooLarge("contiguous stack extent overflow".into()))?;
                if offset != expected {
                    return Err(IoError::UnsupportedFormat(format!(
                        "stack plane {index} starts at offset {offset}, but ImageJ reads contiguous planes at offset {expected}"
                    )));
                }
            }
        }
    }

    let title = path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("Untitled.tif")
        .to_string();

    let mut doc = ImageDocument::new_stack(title, w, h, planes)?;
    if let Some(props) = &props {
        validate_description_metadata(props)?;
    }
    apply_calibration(
        &mut doc,
        props.as_ref(),
        x_resolution,
        y_resolution,
        resolution_unit,
    );
    apply_metadata(&mut doc, props.as_ref())?;
    apply_imagej_metadata(&mut doc, &first_metadata, le)?;
    Ok(doc)
}

/// Reconstructs Calibration the way FileOpener.setCalibration +
/// TiffDecoder.OpenIFD do (FileOpener.java 390-483, TiffDecoder.java 471-490).
fn apply_calibration(
    doc: &mut ImageDocument,
    props: Option<&std::collections::BTreeMap<String, String>>,
    x_resolution: Option<f64>,
    y_resolution: Option<f64>,
    resolution_unit: Option<u16>,
) {
    let cal = &mut doc.calibration;
    // FileInfo leaves pixelWidth/pixelHeight at 0 until X/YResolution tags set
    // them (TiffDecoder.java 471-490).
    let mut pixel_width = x_resolution
        .filter(|value| *value != 0.0)
        .map(|value| 1.0 / value)
        .unwrap_or(0.0);
    let mut pixel_height = y_resolution
        .filter(|value| *value != 0.0)
        .map(|value| 1.0 / value)
        .unwrap_or(0.0);

    let mut unit_from_tags: Option<&str> = None;
    match resolution_unit {
        Some(1) => unit_from_tags = Some(" "),
        Some(2) => {
            if pixel_width == 1.0 / 72.0 {
                pixel_width = 1.0;
                pixel_height = 1.0;
            } else {
                unit_from_tags = Some("inch");
            }
        }
        Some(3) => unit_from_tags = Some("cm"),
        _ => {}
    }

    // decodeDescriptionString sets fi.unit from the description even when the
    // key is absent (the default empty string), and Calibration.setUnit turns
    // that into "pixel". Without an ImageJ description only a ResolutionUnit
    // tag defines fi.unit; X/YResolution without one is ignored by
    // FileOpener.setCalibration (FileOpener.java 390-414).
    let mut unit = props
        .map(|props| sanitize_unit(props.get("unit").map(String::as_str).unwrap_or("")))
        .or_else(|| unit_from_tags.map(sanitize_unit));

    // decodeDescriptionString stores |spacing| in fi.pixelDepth before
    // setCalibration runs (FileOpener.java 641-646).
    let mut pixel_depth = props
        .and_then(|props| property_f64(props, "spacing"))
        .map(f64::abs)
        .unwrap_or(1.0);

    // Prefs.convertToMicrons defaults to true (Prefs.java 181). ImageJ converts
    // sub-threshold centimetre pixels to microns, using 0.0001 cm for an
    // ImageJ-written description and 0.001 cm otherwise (FileOpener.java
    // 398-408). fi.pixelDepth follows only when it is not exactly 1.0.
    if unit.as_deref() == Some("cm") && pixel_width > 0.0 {
        let threshold = if props.is_some() { 0.0001 } else { 0.001 };
        if pixel_width <= threshold {
            pixel_width *= 10000.0;
            pixel_height *= 10000.0;
            if pixel_depth != 1.0 {
                pixel_depth *= 10000.0;
            }
            // Calibration.setUnit canonicalizes "um" the same way.
            unit = Some("\u{00b5}m".to_string());
        }
    }

    if let Some(unit) = unit {
        if pixel_width > 0.0 {
            cal.pixel_width = pixel_width;
            cal.pixel_height = pixel_height;
            cal.pixel_depth = pixel_depth;
            cal.unit = unit;
        }
    }
    if let Some(props) = props {
        cal.invert_y = props.get("inverty").is_some_and(|value| value == "true");
    }
}

/// Applies display range and C/Z/T dimensions from the ImageDescription.
fn apply_metadata(
    doc: &mut ImageDocument,
    props: Option<&std::collections::BTreeMap<String, String>>,
) -> Result<(), IoError> {
    if let Some(props) = props {
        if doc.slices.len() > 1 {
            let channels = property_u32(props, "channels").unwrap_or(1);
            let slices = property_u32(props, "slices").unwrap_or(1);
            let frames = property_u32(props, "frames").unwrap_or(1);
            let total = (channels as usize)
                .checked_mul(slices as usize)
                .and_then(|value| value.checked_mul(frames as usize));
            // FileOpener.setCalibration only calls setDimensions when the
            // product matches the actual stack size (FileOpener.java 468-482).
            if total == Some(doc.slices.len()) {
                doc.set_dimensions(channels, slices, frames)?;
            }
        }
    }

    let stored_range = props.and_then(|props| {
        match (property_f64(props, "min"), property_f64(props, "max")) {
            // ImageJ ignores a stored 0/0 range (FileOpener.java 452-463).
            (Some(min), Some(max)) if !(min == 0.0 && max == 0.0) => Some((min, max)),
            _ => None,
        }
    });
    let supported = matches!(
        doc.slices[0],
        PixelPlane::Gray8(_)
            | PixelPlane::Color256 { .. }
            | PixelPlane::Gray16(_)
            | PixelPlane::Gray32(_)
    );
    if supported {
        if let Some((min, max)) = stored_range {
            doc.display_range = DisplayRange { min, max };
        } else if matches!(doc.slices[0], PixelPlane::Gray16(_) | PixelPlane::Gray32(_)) {
            // A TIFF without min/max opens with the processor's lazily
            // computed image range (ShortProcessor.findMinAndMax /
            // FloatProcessor.findMinAndMax), not 0..65535.
            doc.display_range = auto_display_range(doc);
        }
    }
    if doc.slices.len() > 1 && doc.display_range.min == doc.display_range.max {
        // FileOpener.openStack calls setStackDisplayRange when the first
        // processor's min equals its max, including a stored equal range such
        // as min=5/max=5 (FileOpener.java 300-332).
        doc.display_range = stack_display_range(doc);
    }
    Ok(())
}

/// Rejects ImageJ description metadata that `ImageDocument` cannot represent.
/// FileOpener.setCalibration reads these keys (FileOpener.java 436-450); loading
/// the pixels while dropping them would silently change calibration semantics.
fn validate_description_metadata(
    props: &std::collections::BTreeMap<String, String>,
) -> Result<(), IoError> {
    let unsupported = |key: &str, value: &str| {
        IoError::UnsupportedFormat(format!(
            "unsupported calibration metadata `{key}={value}` in ImageJ TIFF description cannot be represented"
        ))
    };
    for key in ["xorigin", "yorigin", "zorigin", "finterval", "fps"] {
        if let Some(value) = props.get(key) {
            if property_f64(props, key).is_some_and(|value| value != 0.0) {
                return Err(unsupported(key, value));
            }
        }
    }
    if let Some(value) = props.get("tunit") {
        if value != "sec" {
            return Err(unsupported("tunit", value));
        }
    }
    let unit = sanitize_unit(props.get("unit").map(String::as_str).unwrap_or(""));
    for key in ["yunit", "zunit"] {
        if let Some(value) = props.get(key) {
            if sanitize_unit(value) != unit {
                return Err(unsupported(key, value));
            }
        }
    }
    // Density calibration (Calibration.setFunction) is not part of
    // ImageDocument.
    for key in ["cf", "c0", "c1", "c2", "c3", "c4", "vunit"] {
        if let Some(value) = props.get(key) {
            return Err(unsupported(key, value));
        }
    }
    if props.get("zeroclip").is_some_and(|value| value == "true") {
        return Err(unsupported("zeroclip", "true"));
    }
    if let Some(value) = props.get("info") {
        if !value.is_empty() {
            return Err(unsupported("info", value));
        }
    }
    if props.get("loop").is_some_and(|value| value == "true") {
        return Err(IoError::UnsupportedFormat(
            "unsupported TIFF metadata `loop=true`: ImageDocument has no animation loop flag"
                .into(),
        ));
    }
    Ok(())
}

/// Reads a big-endian unsigned short from an ROI record. Every access is
/// bounds-checked so malformed metadata never panics.
fn roi_u16(bytes: &[u8], position: usize) -> Result<u16, IoError> {
    let end = position
        .checked_add(2)
        .ok_or_else(|| IoError::Malformed("ROI metadata offset overflow".into()))?;
    let slice = bytes.get(position..end).ok_or_else(|| {
        IoError::Malformed("ROI metadata is shorter than its declared fields".into())
    })?;
    Ok(u16::from_be_bytes([slice[0], slice[1]]))
}

/// RoiDecoder.getShort (RoiDecoder.java 567-574): a signed 16-bit value,
/// except values below -5000 are treated as unsigned coordinates.
fn roi_java_short(bytes: &[u8], position: usize) -> Result<i32, IoError> {
    let raw = roi_u16(bytes, position)?;
    let signed = raw as i16 as i32;
    Ok(if signed < -5000 { raw as i32 } else { signed })
}

fn roi_i32(bytes: &[u8], position: usize) -> Result<i32, IoError> {
    let end = position
        .checked_add(4)
        .ok_or_else(|| IoError::Malformed("ROI metadata offset overflow".into()))?;
    let slice = bytes.get(position..end).ok_or_else(|| {
        IoError::Malformed("ROI metadata is shorter than its declared fields".into())
    })?;
    Ok(i32::from_be_bytes([slice[0], slice[1], slice[2], slice[3]]))
}

fn roi_f32(bytes: &[u8], position: usize) -> Result<f32, IoError> {
    let end = position
        .checked_add(4)
        .ok_or_else(|| IoError::Malformed("ROI metadata offset overflow".into()))?;
    let slice = bytes.get(position..end).ok_or_else(|| {
        IoError::Malformed("ROI metadata is shorter than its declared fields".into())
    })?;
    Ok(f32::from_be_bytes([slice[0], slice[1], slice[2], slice[3]]))
}

fn roi_float_to_i32(value: f32) -> Option<i32> {
    if !value.is_finite() {
        return None;
    }
    let wide = f64::from(value);
    if wide.fract() != 0.0 || wide < f64::from(i32::MIN) || wide > f64::from(i32::MAX) {
        return None;
    }
    Some(wide as i32)
}

fn roi_float_coordinate(value: f32, axis: &str) -> Result<i32, IoError> {
    roi_float_to_i32(value).ok_or_else(|| {
        unsupported_roi(format!(
            "with subpixel coordinate {axis}={value}, which is not an exact integer"
        ))
    })
}

/// End offset of a polygon ROI's coordinate arrays. Header2 must not begin
/// before this offset, or `RoiDecoder` would read vertex bytes as metadata.
fn roi_polygon_coordinate_end(
    bytes: &[u8],
    version: u16,
    subpixel_option: bool,
) -> Result<usize, IoError> {
    let n = roi_u16(bytes, 16)? as usize;
    let subpixel = subpixel_option && version >= 222;
    let per_point = if subpixel { 12 } else { 4 };
    ROI_HEADER_SIZE
        .checked_add(
            n.checked_mul(per_point)
                .ok_or_else(|| IoError::Malformed("ROI polygon extent overflow".into()))?,
        )
        .ok_or_else(|| IoError::Malformed("ROI polygon extent overflow".into()))
}

fn validate_roi_header2(bytes: &[u8], version: u16, minimum: usize) -> Result<(), IoError> {
    let header2 = roi_i32(bytes, 60)?;
    if header2 == 0 {
        return Ok(());
    }
    if header2 < 0 {
        return Err(IoError::Malformed(
            "ROI metadata has a negative Header2 offset".into(),
        ));
    }
    let header2 = header2 as usize;
    if header2 < ROI_HEADER_SIZE {
        return Err(IoError::Malformed(
            "ROI metadata Header2 overlaps the fixed header".into(),
        ));
    }
    if header2 < minimum {
        return Err(IoError::Malformed(
            "ROI metadata Header2 overlaps the polygon coordinate data".into(),
        ));
    }
    let end = header2
        .checked_add(ROI_HEADER2_SIZE)
        .ok_or_else(|| IoError::Malformed("ROI metadata Header2 offset overflow".into()))?;
    if end > bytes.len() {
        return Err(IoError::Malformed(
            "ROI metadata Header2 extends beyond the record".into(),
        ));
    }

    if roi_i32(bytes, header2 + 4)? != 0
        || roi_i32(bytes, header2 + 8)? != 0
        || roi_i32(bytes, header2 + 12)? != 0
    {
        return Err(unsupported_roi(
            "with a C/Z/T selection position that ImageDocument cannot represent",
        ));
    }
    let name_offset = roi_i32(bytes, header2 + 16)?;
    let name_length = roi_i32(bytes, header2 + 20)?;
    if name_offset != 0 || name_length != 0 {
        return Err(unsupported_roi(
            "with a ROI name that ImageDocument cannot represent",
        ));
    }
    if roi_i32(bytes, header2 + 24)? != 0 {
        return Err(unsupported_roi("with an overlay label color"));
    }
    if roi_u16(bytes, header2 + 28)? != 0 {
        return Err(unsupported_roi("with an overlay label font"));
    }
    let group = bytes[header2 + 30];
    let image_opacity = bytes[header2 + 31];
    let image_size = roi_i32(bytes, header2 + 32)?;
    if image_size != 0 {
        return Err(unsupported_roi("with an embedded image payload (ImageRoi)"));
    }
    if image_opacity != 0 {
        return Err(unsupported_roi("with ImageRoi opacity metadata"));
    }
    if group != 0 {
        return Err(unsupported_roi(
            "with a ROI group that ImageDocument cannot represent",
        ));
    }
    if roi_f32(bytes, header2 + 36)? != 0.0 {
        return Err(unsupported_roi(
            "with a stroke width that ImageDocument cannot represent",
        ));
    }
    let props_offset = roi_i32(bytes, header2 + 40)?;
    let props_length = roi_i32(bytes, header2 + 44)?;
    if props_offset != 0 || props_length != 0 {
        return Err(unsupported_roi(
            "with ROI properties that ImageDocument cannot represent",
        ));
    }
    if roi_i32(bytes, header2 + 48)? != 0 {
        return Err(unsupported_roi(
            "with point-counter metadata that ImageDocument cannot represent",
        ));
    }
    if version >= 229 && group == 0 && roi_u16(bytes, header2 + 52)? != 0 {
        return Err(unsupported_roi(
            "with an extended ROI group that ImageDocument cannot represent",
        ));
    }
    Ok(())
}

/// Decodes the ImageJ `roi ` metadata payload written by RoiEncoder. The
/// decoder mirrors RoiDecoder.getRoi for the four ROI kinds `ij-core` models.
/// Metadata that changes the selection but cannot be represented is rejected
/// instead of being dropped.
fn decode_roi_metadata(bytes: &[u8]) -> Result<Roi, IoError> {
    if bytes.len() < ROI_HEADER_SIZE {
        return Err(IoError::Malformed(
            "ImageJ TIFF ROI metadata is shorter than the 64-byte header".into(),
        ));
    }
    if &bytes[..4] != b"Iout" {
        return Err(IoError::UnsupportedFormat(
            "ImageJ TIFF ROI metadata does not start with \"Iout\"".into(),
        ));
    }
    let version = roi_u16(bytes, 4)?;
    if !(218..=ROI_VERSION).contains(&version) {
        return Err(IoError::UnsupportedFormat(format!(
            "unsupported ImageJ TIFF ROI format version {version}; supported versions are 218 through {ROI_VERSION}"
        )));
    }
    let roi_type = bytes[6];
    let top = roi_java_short(bytes, 8)?;
    let left = roi_java_short(bytes, 10)?;
    let bottom = roi_java_short(bytes, 12)?;
    let right = roi_java_short(bytes, 14)?;

    if roi_i32(bytes, 36)? != 0 {
        return Err(unsupported_roi("with a composite ShapeRoi segment array"));
    }
    let subtype = roi_u16(bytes, 48)?;
    if subtype != 0 {
        return Err(unsupported_roi(format!(
            "with subtype {subtype} (text, arrow, ellipse, rotated rectangle or image ROI)"
        )));
    }
    let options = roi_u16(bytes, 50)?;
    let subpixel_option = options & ROI_SUB_PIXEL_RESOLUTION != 0;
    let unsupported_options = options & !ROI_SUB_PIXEL_RESOLUTION;
    if unsupported_options != 0 {
        return Err(unsupported_roi(format!(
            "with unsupported option bits 0x{unsupported_options:04x} (stroke/overlay/label/draw-offset metadata)"
        )));
    }
    if roi_java_short(bytes, 34)? != 0 {
        return Err(unsupported_roi(
            "with a stroke width that ImageDocument cannot represent",
        ));
    }
    if roi_i32(bytes, 40)? != 0 || roi_i32(bytes, 44)? != 0 {
        return Err(unsupported_roi(
            "with stroke or fill colors that ImageDocument cannot represent",
        ));
    }
    if roi_java_short(bytes, 54)? != 0 {
        return Err(unsupported_roi("with a rounded-rectangle corner diameter"));
    }
    if roi_i32(bytes, 56)? != 0 {
        return Err(unsupported_roi(
            "with a selection position that ImageDocument cannot represent",
        ));
    }
    let header2_minimum = if roi_type == ROI_TYPE_POLYGON {
        roi_polygon_coordinate_end(bytes, version, subpixel_option)?
    } else {
        ROI_HEADER_SIZE
    };
    validate_roi_header2(bytes, version, header2_minimum)?;

    match roi_type {
        ROI_TYPE_RECT | ROI_TYPE_OVAL => {
            let (x, y, width, height) = if version >= 223 && subpixel_option {
                (
                    roi_float_coordinate(roi_f32(bytes, 18)?, "x")?,
                    roi_float_coordinate(roi_f32(bytes, 22)?, "y")?,
                    roi_float_coordinate(roi_f32(bytes, 26)?, "width")?,
                    roi_float_coordinate(roi_f32(bytes, 30)?, "height")?,
                )
            } else {
                if subpixel_option {
                    return Err(unsupported_roi(
                        "with a subpixel option that this ROI format version does not apply",
                    ));
                }
                (left, top, right - left, bottom - top)
            };
            if width <= 0 || height <= 0 {
                return Err(unsupported_roi(format!(
                    "with non-positive bounds {width}x{height}"
                )));
            }
            let rect = RectRoi {
                x,
                y,
                width,
                height,
            };
            Ok(if roi_type == ROI_TYPE_RECT {
                Roi::Rect(rect)
            } else {
                Roi::Oval(rect)
            })
        }
        ROI_TYPE_POLYGON => {
            let n = roi_u16(bytes, 16)? as usize;
            if n < 3 {
                return Err(unsupported_roi(format!(
                    "with a polygon of {n} points (at least three are required)"
                )));
            }
            let subpixel = subpixel_option && version >= 222;
            if subpixel_option && version < 222 {
                return Err(unsupported_roi(
                    "with a subpixel option that this ROI format version does not apply",
                ));
            }
            let coordinate_bytes = n
                .checked_mul(4)
                .and_then(|span| {
                    if subpixel {
                        n.checked_mul(8).and_then(|extra| span.checked_add(extra))
                    } else {
                        Some(span)
                    }
                })
                .ok_or_else(|| IoError::Malformed("ROI polygon extent overflow".into()))?;
            let required = ROI_HEADER_SIZE
                .checked_add(coordinate_bytes)
                .ok_or_else(|| IoError::Malformed("ROI polygon extent overflow".into()))?;
            if bytes.len() < required {
                return Err(IoError::Malformed(format!(
                    "ROI polygon declares {n} points but the record is too short"
                )));
            }
            let mut xpoints = Vec::with_capacity(n);
            let mut ypoints = Vec::with_capacity(n);
            if subpixel {
                let x_base = ROI_HEADER_SIZE + n * 4;
                let y_base = x_base + n * 4;
                for index in 0..n {
                    xpoints.push(roi_float_coordinate(
                        roi_f32(bytes, x_base + index * 4)?,
                        "x",
                    )?);
                    ypoints.push(roi_float_coordinate(
                        roi_f32(bytes, y_base + index * 4)?,
                        "y",
                    )?);
                }
            } else {
                for index in 0..n {
                    let rx = roi_java_short(bytes, ROI_HEADER_SIZE + index * 2)?.max(0);
                    let ry = roi_java_short(bytes, ROI_HEADER_SIZE + n * 2 + index * 2)?.max(0);
                    xpoints.push(left.checked_add(rx).ok_or_else(|| {
                        IoError::Malformed("ROI polygon x coordinate overflow".into())
                    })?);
                    ypoints.push(top.checked_add(ry).ok_or_else(|| {
                        IoError::Malformed("ROI polygon y coordinate overflow".into())
                    })?);
                }
            }
            let min_x = *xpoints.iter().min().unwrap_or(&0);
            let max_x = *xpoints.iter().max().unwrap_or(&0);
            let min_y = *ypoints.iter().min().unwrap_or(&0);
            let max_y = *ypoints.iter().max().unwrap_or(&0);
            if min_x == max_x && min_y == max_y {
                // ImagePlus.setRoi deletes an area selection whose width and
                // height are both zero (ImagePlus.java 2073-2076).
                return Err(unsupported_roi(
                    "with zero width and height, which ImageJ drops on open",
                ));
            }
            if subpixel
                && (max_x < 0
                    || (min_x == 0 && max_x == 0)
                    || max_y < 0
                    || (min_y == 0 && max_y == 0))
            {
                // FloatPolygon starts maxX/maxY at Float.MIN_VALUE, a small
                // positive value (FloatPolygon.java 89-105), so these axes
                // get Java bounds that the Rust polygon cannot express.
                return Err(unsupported_roi(
                    "with subpixel coordinates on an axis whose ImageJ FloatPolygon bounds are incorrect",
                ));
            }
            Ok(Roi::Polygon(PolygonRoi { xpoints, ypoints }))
        }
        ROI_TYPE_LINE => {
            // Line endpoints are always stored as floats, even for integer
            // coordinates (RoiEncoder.java 208-213). The subpixel option only
            // carries the draw offset, which the option check above rejects.
            let x1 = roi_float_coordinate(roi_f32(bytes, 18)?, "x1")?;
            let y1 = roi_float_coordinate(roi_f32(bytes, 22)?, "y1")?;
            let x2 = roi_float_coordinate(roi_f32(bytes, 26)?, "x2")?;
            let y2 = roi_float_coordinate(roi_f32(bytes, 30)?, "y2")?;
            Ok(Roi::Line(LineRoi { x1, y1, x2, y2 }))
        }
        other => Err(unsupported_roi(format!(
            "with ROI type {other}; supported types are rectangle (1), oval (2), polygon (0) and line (3)"
        ))),
    }
}

/// Validates/restores ImageJ's private metadata block. Custom channel LUTs
/// cannot be represented and are rejected. A uniform per-channel display range
/// is representable as the document range; differing ranges are not.
fn apply_imagej_metadata(
    doc: &mut ImageDocument,
    entries: &[ImageJMetadata],
    le: bool,
) -> Result<(), IoError> {
    let mut uniform_range: Option<DisplayRange> = None;
    let mut roi: Option<Roi> = None;
    let mut saw_roi = false;
    for entry in entries {
        match entry.kind {
            META_LUTS => {
                if entry.payloads.iter().any(|payload| !payload.is_empty()) {
                    return Err(IoError::UnsupportedFormat(
                        "ImageJ TIFF contains custom channel LUT metadata (`luts`) that cannot be represented; refusing to drop it".into(),
                    ));
                }
            }
            META_RANGES => {
                let Some(payload) = entry.payloads.first() else {
                    continue;
                };
                if payload.is_empty() {
                    continue;
                }
                if payload.len() % 16 != 0 {
                    return Err(IoError::Malformed(
                        "ImageJ display-range metadata is not a list of min/max pairs".into(),
                    ));
                }
                let mut range: Option<(f64, f64)> = None;
                for pair in payload.chunks_exact(16) {
                    let min = read_metadata_f64(pair, le)?;
                    let max = read_metadata_f64(&pair[8..], le)?;
                    match range {
                        None => range = Some((min, max)),
                        Some((first_min, first_max))
                            if first_min == min && first_max == max => {}
                        Some(_) => {
                            return Err(IoError::UnsupportedFormat(
                                "ImageJ TIFF has different per-channel display ranges that ImageDocument cannot represent".into(),
                            ))
                        }
                    }
                }
                if let Some((min, max)) = range {
                    uniform_range = Some(DisplayRange { min, max });
                }
            }
            META_LABELS => {
                if entry.payloads.len() > doc.slices.len() {
                    return Err(IoError::Malformed(
                        "ImageJ TIFF has more slice labels than slices".into(),
                    ));
                }
                for (index, payload) in entry.payloads.iter().enumerate() {
                    if payload.is_empty() {
                        continue;
                    }
                    if payload.len() % 2 != 0 {
                        return Err(IoError::Malformed(
                            "ImageJ TIFF slice label has odd UTF-16 byte length".into(),
                        ));
                    }
                    let units = payload.chunks_exact(2).map(|pair| {
                        if le {
                            u16::from_le_bytes([pair[0], pair[1]])
                        } else {
                            u16::from_be_bytes([pair[0], pair[1]])
                        }
                    });
                    doc.slice_labels[index] =
                        Some(String::from_utf16(&units.collect::<Vec<_>>()).map_err(|_| {
                            IoError::Malformed("ImageJ TIFF slice label is invalid UTF-16".into())
                        })?);
                }
            }
            META_INFO => {
                if entry.payloads.iter().any(|payload| !payload.is_empty()) {
                    return Err(IoError::UnsupportedFormat(
                        "ImageJ TIFF contains an image Info property that ImageDocument cannot represent"
                            .into(),
                    ));
                }
            }
            META_PLOT => {
                if entry.payloads.iter().any(|payload| !payload.is_empty()) {
                    return Err(IoError::UnsupportedFormat(
                        "ImageJ TIFF contains plot metadata that ImageDocument cannot represent"
                            .into(),
                    ));
                }
            }
            // ROI metadata is restored through RoiDecoder semantics. Anything
            // the integer `ij-core` selection model cannot represent is
            // rejected inside decode_roi_metadata rather than dropped.
            META_ROI => {
                if saw_roi {
                    return Err(IoError::UnsupportedFormat(
                        "ImageJ TIFF contains more than one ROI metadata record".into(),
                    ));
                }
                saw_roi = true;
                if entry.payloads.len() != 1 {
                    return Err(IoError::Malformed(
                        "ImageJ TIFF ROI metadata has an unexpected payload count".into(),
                    ));
                }
                roi = Some(decode_roi_metadata(&entry.payloads[0])?);
            }
            // FileSaver writes overlay and image-property records only when
            // those ImagePlus features are present. ImageDocument has no
            // overlay/property model, so loading them would silently discard
            // selectable state.
            META_OVERLAY => {
                if entry.payloads.iter().any(|payload| !payload.is_empty()) {
                    return Err(IoError::UnsupportedFormat(
                        "ImageJ TIFF contains overlay ROI metadata that ImageDocument cannot represent; refusing to drop it".into(),
                    ));
                }
            }
            META_PROPERTIES => {
                if entry.payloads.iter().any(|payload| !payload.is_empty()) {
                    return Err(IoError::UnsupportedFormat(
                        "ImageJ TIFF contains image-property metadata (`prop`) that ImageDocument cannot represent; refusing to drop it".into(),
                    ));
                }
            }
            other => {
                return Err(IoError::UnsupportedFormat(format!(
                    "unknown ImageJ TIFF metadata type 0x{other:08x}"
                )));
            }
        }
    }
    if let Some(range) = uniform_range {
        doc.display_range = range;
    }
    if let Some(roi) = roi {
        doc.roi = Some(roi);
    }
    Ok(())
}

fn read_metadata_f64(bytes: &[u8], le: bool) -> Result<f64, IoError> {
    if bytes.len() < 8 {
        return Err(IoError::Malformed(
            "ImageJ metadata double is truncated".into(),
        ));
    }
    let mut raw = [0u8; 8];
    raw.copy_from_slice(&bytes[..8]);
    Ok(if le {
        f64::from_le_bytes(raw)
    } else {
        f64::from_be_bytes(raw)
    })
}

/// FileOpener.setStackDisplayRange (FileOpener.java 315-332): only a first
/// slice whose min and max are equal triggers the stack-wide range.
fn auto_display_range(doc: &ImageDocument) -> DisplayRange {
    let (mut min, mut max) = slice_range(&doc.slices[0]);
    if min == max {
        for plane in &doc.slices[1..] {
            let (slice_min, slice_max) = slice_range(plane);
            if slice_min < min {
                min = slice_min;
            }
            if slice_max > max {
                max = slice_max;
            }
        }
    }
    DisplayRange { min, max }
}

/// FileOpener.setStackDisplayRange (FileOpener.java 315-332) resets every
/// processor before aggregating, so all slices contribute regardless of the
/// first slice's range.
fn stack_display_range(doc: &ImageDocument) -> DisplayRange {
    let mut min = f64::INFINITY;
    let mut max = f64::NEG_INFINITY;
    for plane in &doc.slices {
        let (slice_min, slice_max) = slice_range(plane);
        if slice_min < min {
            min = slice_min;
        }
        if slice_max > max {
            max = slice_max;
        }
    }
    DisplayRange { min, max }
}

fn slice_range(plane: &PixelPlane) -> (f64, f64) {
    match plane {
        PixelPlane::Gray16(v) => (
            v.iter().map(|&px| px as f64).fold(f64::INFINITY, f64::min),
            v.iter()
                .map(|&px| px as f64)
                .fold(f64::NEG_INFINITY, f64::max),
        ),
        PixelPlane::Gray32(v) => (
            v.iter().map(|&px| px as f64).fold(f64::INFINITY, f64::min),
            v.iter()
                .map(|&px| px as f64)
                .fold(f64::NEG_INFINITY, f64::max),
        ),
        _ => (0.0, 255.0),
    }
}

/// Calibration.sanitizeUnit (Calibration.java 128-137).
fn sanitize_unit(unit: &str) -> String {
    match unit {
        "" => "pixel".to_string(),
        "um" => "\u{00b5}m".to_string(),
        "A" => "\u{00c5}".to_string(),
        other => other.to_string(),
    }
}

/// Parses a Java Properties stream the way `Properties.load` reads the
/// ImageDescription that FileSaver writes, including `\uXXXX` escapes from
/// FileSaver.appendEscapedLine.
fn parse_java_properties(text: &str) -> std::collections::BTreeMap<String, String> {
    let mut props = std::collections::BTreeMap::new();
    for line in text.lines() {
        let line = line.trim_start();
        if line.is_empty() || line.starts_with('#') || line.starts_with('!') {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        // Properties.load skips the whitespace that follows the separator.
        props.insert(
            unescape_java_property(key),
            unescape_java_property(value.trim_start()),
        );
    }
    props
}

fn unescape_java_property(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    let mut chars = value.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('u') => {
                let hex: String = chars.by_ref().take(4).collect();
                if let Ok(code) = u32::from_str_radix(&hex, 16) {
                    if let Some(decoded) = char::from_u32(code) {
                        out.push(decoded);
                    }
                }
            }
            Some('n') => out.push('\n'),
            Some('r') => out.push('\r'),
            Some('t') => out.push('\t'),
            Some('f') => out.push('\u{000c}'),
            Some(other) => out.push(other),
            None => out.push('\\'),
        }
    }
    out
}

fn property_f64(props: &std::collections::BTreeMap<String, String>, key: &str) -> Option<f64> {
    let value = props.get(key)?.trim();
    match value {
        "NaN" => Some(f64::NAN),
        "Infinity" => Some(f64::INFINITY),
        "-Infinity" => Some(f64::NEG_INFINITY),
        _ => value.parse::<f64>().ok(),
    }
}

fn property_u32(props: &std::collections::BTreeMap<String, String>, key: &str) -> Option<u32> {
    let value = property_f64(props, key)?;
    if (0.0..=u32::MAX as f64).contains(&value) {
        Some(value as u32)
    } else {
        None
    }
}

fn write_ifd_entry(out: &mut Vec<u8>, tag: u16, field_type: u16, count: u32, value: u32) {
    out.extend_from_slice(&tag.to_le_bytes());
    out.extend_from_slice(&field_type.to_le_bytes());
    out.extend_from_slice(&count.to_le_bytes());
    if field_type == 3 && count == 1 {
        out.extend_from_slice(&(value as u16).to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
    } else {
        out.extend_from_slice(&value.to_le_bytes());
    }
}

fn read_u16(bytes: &[u8], pos: usize, le: bool) -> Result<u16, IoError> {
    let end = pos
        .checked_add(2)
        .ok_or_else(|| IoError::Malformed("TIFF read offset overflow".into()))?;
    if end > bytes.len() {
        return Err(IoError::InvalidHeader);
    }
    Ok(if le {
        u16::from_le_bytes([bytes[pos], bytes[pos + 1]])
    } else {
        u16::from_be_bytes([bytes[pos], bytes[pos + 1]])
    })
}

fn read_u32(bytes: &[u8], pos: usize, le: bool) -> Result<u32, IoError> {
    let end = pos
        .checked_add(4)
        .ok_or_else(|| IoError::Malformed("TIFF read offset overflow".into()))?;
    if end > bytes.len() {
        return Err(IoError::InvalidHeader);
    }
    Ok(if le {
        u32::from_le_bytes([bytes[pos], bytes[pos + 1], bytes[pos + 2], bytes[pos + 3]])
    } else {
        u32::from_be_bytes([bytes[pos], bytes[pos + 1], bytes[pos + 2], bytes[pos + 3]])
    })
}

fn read_tag_u32(bytes: &[u8], field_type: u16, val_pos: usize, le: bool) -> Result<u32, IoError> {
    match field_type {
        3 => Ok(read_u16(bytes, val_pos, le)? as u32),
        4 => read_u32(bytes, val_pos, le),
        _ => Err(IoError::UnsupportedFormat(format!(
            "unexpected TIFF field type {field_type}"
        ))),
    }
}

/// Reads a scalar tag whose model field is 16 bits. Java compares the full
/// 32-bit `getValue` result against 16-bit constants (TiffDecoder.java
/// 148-157, 382-550), so a LONG-typed value such as Compression 0x00010001 is
/// rejected there as unsupported. Masking it with `as u16` would instead turn
/// it into uncompressed (1) and silently change the pixel interpretation.
fn read_tag_u16(
    bytes: &[u8],
    field_type: u16,
    name: &str,
    val_pos: usize,
    le: bool,
) -> Result<u16, IoError> {
    let value = read_tag_u32(bytes, field_type, val_pos, le)?;
    u16::try_from(value).map_err(|_| {
        IoError::Malformed(format!(
            "{name} value {value} does not fit in 16 bits; \
             truncating it would change the TIFF interpretation"
        ))
    })
}

fn read_tag_u32_vec(
    bytes: &[u8],
    field_type: u16,
    count: usize,
    val_pos: usize,
    le: bool,
) -> Result<Vec<u32>, IoError> {
    let item_size = match field_type {
        3 => 2usize,
        4 => 4usize,
        _ => {
            return Err(IoError::UnsupportedFormat(format!(
                "unexpected TIFF array field type {field_type}"
            )));
        }
    };
    if count == 0 {
        return Ok(Vec::new());
    }
    if count == 1 {
        return Ok(vec![read_tag_u32(bytes, field_type, val_pos, le)?]);
    }
    if field_type == 3 && count == 2 {
        return Ok(vec![
            read_u16(bytes, val_pos, le)? as u32,
            read_u16(bytes, val_pos + 2, le)? as u32,
        ]);
    }
    let ext_pos = read_u32(bytes, val_pos, le)? as usize;
    // Validate the whole array span before reserving, so a huge declared count
    // cannot trigger an allocation larger than the file itself.
    let span = count
        .checked_mul(item_size)
        .ok_or_else(|| IoError::Malformed("TIFF array extent overflow".into()))?;
    let end = ext_pos
        .checked_add(span)
        .ok_or_else(|| IoError::Malformed("TIFF array extent overflow".into()))?;
    if end > bytes.len() {
        return Err(IoError::Malformed(format!(
            "TIFF array of {count} entries extends beyond end of file"
        )));
    }
    let mut out = Vec::with_capacity(count);
    for i in 0..count {
        let value = match field_type {
            3 => read_u16(bytes, ext_pos + i * 2, le)? as u32,
            4 => read_u32(bytes, ext_pos + i * 4, le)?,
            _ => unreachable!("field type validated above"),
        };
        out.push(value);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::env;

    #[test]
    fn compressed_strips_reject_truncation_corruption_and_excess_output() {
        for input in [&[127, 1][..], &[255][..], &[254, 1][..], &[0, 1][..]] {
            assert!(matches!(
                decompress_tiff_strip(input, TiffCompression::PackBits, 2),
                Err(IoError::Malformed(_))
            ));
        }
        for input in [&[][..], &[255, 255][..]] {
            assert!(matches!(
                decompress_tiff_strip(input, TiffCompression::Lzw, 2),
                Err(IoError::Malformed(_))
            ));
        }
        let mut encoder =
            flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
        encoder.write_all(&[7; 1024]).unwrap();
        let valid = encoder.finish().unwrap();
        let mut corrupt = valid.clone();
        *corrupt.last_mut().unwrap() ^= 1;
        for (input, expected) in [
            (&valid[..valid.len() - 1], 1024),
            (corrupt.as_slice(), 1024),
            (valid.as_slice(), 4),
            (valid.as_slice(), 1025),
            (&[][..], 1024),
        ] {
            assert!(matches!(
                decompress_tiff_strip(input, TiffCompression::Deflate, expected),
                Err(IoError::Malformed(_))
            ));
        }
    }

    #[test]
    fn tiff_8bit_roundtrip_preserves_pixels_and_dimensions() {
        let doc = ImageDocument::new_ramp_8bit("ramp.tif", 32, 24).unwrap();
        let tmp = env::temp_dir().join("ij_rs_roundtrip_8bit.tif");
        save_tiff(&tmp, &doc).unwrap();
        let loaded = load_tiff(&tmp).unwrap();
        let _ = fs::remove_file(&tmp);
        assert_eq!(loaded.width, 32);
        assert_eq!(loaded.height, 24);
        assert_eq!(loaded.plane(), doc.plane());
    }

    #[test]
    fn tiff_multipage_roundtrip_preserves_16bit_float_and_rgb_slices() {
        let gray16 = vec![
            PixelPlane::Gray16(vec![1u16, 2, 3, 4, 5, 6]),
            PixelPlane::Gray16(vec![100u16, 200, 300, 400, 500, 600]),
        ];
        let gray32 = vec![
            PixelPlane::Gray32(vec![0.0f32, 0.25, 0.5, 0.75, 1.0, -1.5]),
            PixelPlane::Gray32(vec![2.0f32, 3.0, 4.0, 5.0, 6.0, 7.0]),
        ];
        let rgb = vec![
            PixelPlane::ColorRgb(vec![
                0xff00_0000,
                0xff00_00ff,
                0xff00_ff00,
                0xffff_0000,
                0xff00_ffff,
                0xffff_ffff,
            ]),
            PixelPlane::ColorRgb(vec![
                0xff12_3456,
                0xff65_4321,
                0xffaa_bbcc,
                0xff01_0203,
                0xff04_0506,
                0xff07_0809,
            ]),
        ];

        for (name, slices) in [("gray16", gray16), ("gray32", gray32), ("rgb", rgb)] {
            let doc = ImageDocument::new_stack(name, 3, 2, slices.clone()).unwrap();
            let tmp = env::temp_dir().join(format!("ij_rs_roundtrip_stack_{name}.tif"));
            save_tiff(&tmp, &doc).unwrap();
            let loaded = load_tiff(&tmp).unwrap_or_else(|e| panic!("{name}: {e}"));
            let _ = fs::remove_file(&tmp);
            assert_eq!(loaded.slices, slices, "{name}");
            assert_eq!(loaded.get_dimensions(), [3, 2, 1, 2, 1], "{name}");
        }
    }

    #[test]
    fn tiff_hyperstack_roundtrip_preserves_dimensions_and_slices() {
        let slices = vec![PixelPlane::Gray8(vec![7u8; 6]); 12];
        let doc =
            ImageDocument::new_hyperstack("hyper.tif", 3, 2, slices.clone(), 2, 2, 3).unwrap();
        let tmp = env::temp_dir().join("ij_rs_roundtrip_hyperstack.tif");
        save_tiff(&tmp, &doc).unwrap();
        let loaded = load_tiff(&tmp).unwrap();
        let _ = fs::remove_file(&tmp);
        assert_eq!(loaded.get_dimensions(), [3, 2, 2, 2, 3]);
        assert_eq!(loaded.slices, slices);
    }

    #[test]
    fn tiff_multipage_roundtrip_preserves_all_slices() {
        let mut slices = Vec::new();
        for s in 0..3u8 {
            slices.push(PixelPlane::Gray8(
                (0..12).map(|i| i + s * 20).collect::<Vec<_>>(),
            ));
        }
        let doc = ImageDocument::new_stack("stack.tif", 4, 3, slices.clone()).unwrap();
        let tmp = env::temp_dir().join("ij_rs_roundtrip_stack_8bit.tif");
        save_tiff(&tmp, &doc).unwrap();
        let loaded = load_tiff(&tmp).unwrap();
        let _ = fs::remove_file(&tmp);
        assert_eq!(loaded.slices, slices);
        assert_eq!(loaded.stack_size(), 3);
        assert_eq!(loaded.get_dimensions(), [4, 3, 1, 3, 1]);
    }

    fn tmp_tiff(name: &str) -> std::path::PathBuf {
        env::temp_dir().join(format!("ij_rs_{name}_{}.tif", std::process::id()))
    }

    /// Builds a minimal little-endian, single-IFD TIFF. Each entry is
    /// `(tag, field_type, count, value)`. Image data starts immediately after
    /// the 8-byte header, so `StripOffsets` for the fixtures is 8.
    fn make_tiff(entries: &[(u16, u16, u32, u32)], data: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(b"II");
        out.extend_from_slice(&42u16.to_le_bytes());
        out.extend_from_slice(&0u32.to_le_bytes());
        out.extend_from_slice(data);
        if out.len() % 2 == 1 {
            out.push(0);
        }
        let ifd_offset = out.len() as u32;
        out[4..8].copy_from_slice(&ifd_offset.to_le_bytes());
        out.extend_from_slice(&(entries.len() as u16).to_le_bytes());
        for &(tag, field_type, count, value) in entries {
            out.extend_from_slice(&tag.to_le_bytes());
            out.extend_from_slice(&field_type.to_le_bytes());
            out.extend_from_slice(&count.to_le_bytes());
            out.extend_from_slice(&value.to_le_bytes());
        }
        out.extend_from_slice(&0u32.to_le_bytes());
        out
    }

    /// Builds a big-endian single-IFD TIFF. Each entry is
    /// `(tag, field_type, count, value)`. An inline SHORT value is
    /// left-justified in the four-byte value slot, as TIFF requires and as
    /// TiffDecoder reads it.
    fn make_big_endian_tiff(entries: &[(u16, u16, u32, u32)], data: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(b"MM");
        out.extend_from_slice(&42u16.to_be_bytes());
        out.extend_from_slice(&0u32.to_be_bytes());
        out.extend_from_slice(data);
        if out.len() % 2 == 1 {
            out.push(0);
        }
        let ifd_offset = out.len() as u32;
        out[4..8].copy_from_slice(&ifd_offset.to_be_bytes());
        out.extend_from_slice(&(entries.len() as u16).to_be_bytes());
        for &(tag, field_type, count, value) in entries {
            out.extend_from_slice(&tag.to_be_bytes());
            out.extend_from_slice(&field_type.to_be_bytes());
            out.extend_from_slice(&count.to_be_bytes());
            if field_type == 3 && count == 1 {
                out.extend_from_slice(&(value as u16).to_be_bytes());
                out.extend_from_slice(&0u16.to_be_bytes());
            } else {
                out.extend_from_slice(&value.to_be_bytes());
            }
        }
        out.extend_from_slice(&0u32.to_be_bytes());
        out
    }

    /// Builds a little-endian two-IFD 8-bit stack with contiguous planes from
    /// offset 8. Each IFD may carry its own ImageDescription.
    fn make_two_ifd_tiff(
        width: u32,
        height: u32,
        planes: [&[u8]; 2],
        descriptions: [Option<&str>; 2],
    ) -> Vec<u8> {
        let plane_bytes = (width as usize) * (height as usize);
        assert_eq!(planes[0].len(), plane_bytes);
        assert_eq!(planes[1].len(), plane_bytes);

        let mut out = vec![0u8; 8];
        out[..2].copy_from_slice(b"II");
        out[2..4].copy_from_slice(&42u16.to_le_bytes());
        let mut strip_offsets = Vec::with_capacity(2);
        for plane in planes {
            strip_offsets.push(out.len() as u32);
            out.extend_from_slice(plane);
        }
        if out.len() % 2 == 1 {
            out.push(0);
        }

        let blobs: Vec<Option<Vec<u8>>> = descriptions
            .iter()
            .map(|text| {
                text.map(|text| {
                    let mut bytes = text.as_bytes().to_vec();
                    bytes.push(0);
                    bytes
                })
            })
            .collect();
        let entry_counts = [
            10 + usize::from(blobs[0].is_some()),
            10 + usize::from(blobs[1].is_some()),
        ];
        let ifd1 = out.len();
        let ifd2 = ifd1 + 2 + entry_counts[0] * 12 + 4;
        let ifd2_size = 2 + entry_counts[1] * 12 + 4;
        let mut blob_offsets = Vec::with_capacity(2);
        let mut cursor = ifd2 + ifd2_size;
        for blob in blobs.iter().flatten() {
            if cursor % 2 == 1 {
                cursor += 1;
            }
            blob_offsets.push(cursor as u32);
            cursor += blob.len();
        }
        out[4..8].copy_from_slice(&(ifd1 as u32).to_le_bytes());

        for index in 0..2 {
            let description_entry = blobs[index].as_ref().map(|blob| {
                let placed = blobs[..index]
                    .iter()
                    .filter(|other| other.is_some())
                    .count();
                (blob.len() as u32, blob_offsets[placed])
            });
            let next_ifd = if index == 0 { ifd2 as u32 } else { 0 };
            out.extend_from_slice(&(entry_counts[index] as u16).to_le_bytes());
            out.extend_from_slice(&TAG_IMAGE_WIDTH.to_le_bytes());
            out.extend_from_slice(&4u16.to_le_bytes());
            out.extend_from_slice(&1u32.to_le_bytes());
            out.extend_from_slice(&width.to_le_bytes());
            out.extend_from_slice(&TAG_IMAGE_LENGTH.to_le_bytes());
            out.extend_from_slice(&4u16.to_le_bytes());
            out.extend_from_slice(&1u32.to_le_bytes());
            out.extend_from_slice(&height.to_le_bytes());
            out.extend_from_slice(&TAG_BITS_PER_SAMPLE.to_le_bytes());
            out.extend_from_slice(&3u16.to_le_bytes());
            out.extend_from_slice(&1u32.to_le_bytes());
            out.extend_from_slice(&8u16.to_le_bytes());
            out.extend_from_slice(&0u16.to_le_bytes());
            out.extend_from_slice(&TAG_COMPRESSION.to_le_bytes());
            out.extend_from_slice(&3u16.to_le_bytes());
            out.extend_from_slice(&1u32.to_le_bytes());
            out.extend_from_slice(&1u16.to_le_bytes());
            out.extend_from_slice(&0u16.to_le_bytes());
            out.extend_from_slice(&TAG_PHOTO_INTERP.to_le_bytes());
            out.extend_from_slice(&3u16.to_le_bytes());
            out.extend_from_slice(&1u32.to_le_bytes());
            out.extend_from_slice(&1u16.to_le_bytes());
            out.extend_from_slice(&0u16.to_le_bytes());
            if let Some((count, offset)) = description_entry {
                out.extend_from_slice(&TAG_IMAGE_DESCRIPTION.to_le_bytes());
                out.extend_from_slice(&2u16.to_le_bytes());
                out.extend_from_slice(&count.to_le_bytes());
                out.extend_from_slice(&offset.to_le_bytes());
            }
            out.extend_from_slice(&TAG_STRIP_OFFSETS.to_le_bytes());
            out.extend_from_slice(&4u16.to_le_bytes());
            out.extend_from_slice(&1u32.to_le_bytes());
            out.extend_from_slice(&strip_offsets[index].to_le_bytes());
            out.extend_from_slice(&TAG_SAMPLES_PER_PIXEL.to_le_bytes());
            out.extend_from_slice(&3u16.to_le_bytes());
            out.extend_from_slice(&1u32.to_le_bytes());
            out.extend_from_slice(&1u16.to_le_bytes());
            out.extend_from_slice(&0u16.to_le_bytes());
            out.extend_from_slice(&TAG_ROWS_PER_STRIP.to_le_bytes());
            out.extend_from_slice(&4u16.to_le_bytes());
            out.extend_from_slice(&1u32.to_le_bytes());
            out.extend_from_slice(&height.to_le_bytes());
            out.extend_from_slice(&TAG_STRIP_BYTE_COUNTS.to_le_bytes());
            out.extend_from_slice(&4u16.to_le_bytes());
            out.extend_from_slice(&1u32.to_le_bytes());
            out.extend_from_slice(&(plane_bytes as u32).to_le_bytes());
            out.extend_from_slice(&TAG_SAMPLE_FORMAT.to_le_bytes());
            out.extend_from_slice(&3u16.to_le_bytes());
            out.extend_from_slice(&1u32.to_le_bytes());
            out.extend_from_slice(&1u16.to_le_bytes());
            out.extend_from_slice(&0u16.to_le_bytes());
            out.extend_from_slice(&next_ifd.to_le_bytes());
        }
        assert_eq!(out.len(), ifd2 + ifd2_size);
        for (blob, offset) in blobs.iter().flatten().zip(&blob_offsets) {
            while out.len() < *offset as usize {
                out.push(0);
            }
            out.extend_from_slice(blob);
        }
        out
    }

    /// Builds a single-IFD TIFF containing an ImageJ `roi ` metadata record,
    /// using the same `MetaDataByteCounts`/`MetaData` layout as
    /// TiffEncoder.writeMetaData.
    fn make_roi_tiff(roi: &[u8]) -> Vec<u8> {
        let width = 4u32;
        let height = 4u32;
        let data = vec![0u8; (width * height) as usize];
        let counts = [12u32, roi.len() as u32];
        let counts: Vec<u8> = counts.iter().flat_map(|v| v.to_le_bytes()).collect();
        let mut metadata = Vec::new();
        metadata.extend_from_slice(&META_MAGIC.to_le_bytes());
        metadata.extend_from_slice(&META_ROI.to_le_bytes());
        metadata.extend_from_slice(&1u32.to_le_bytes());
        metadata.extend_from_slice(roi);

        let mut out = Vec::new();
        out.extend_from_slice(b"II");
        out.extend_from_slice(&42u16.to_le_bytes());
        out.extend_from_slice(&0u32.to_le_bytes());
        out.extend_from_slice(&data);
        if out.len() % 2 == 1 {
            out.push(0);
        }
        let ifd_offset = out.len() as u32;
        out[4..8].copy_from_slice(&ifd_offset.to_le_bytes());
        let mut entries = base_entries(width, height, 8, 1, 1, 1, width * height);
        entries.push((TAG_META_DATA_BYTE_COUNTS, 4, 2, 0));
        entries.push((TAG_META_DATA, 1, metadata.len() as u32, 0));
        out.extend_from_slice(&(entries.len() as u16).to_le_bytes());
        let blob_start = out.len() + entries.len() * 12 + 4;
        let counts_offset = blob_start as u32;
        let metadata_offset = (counts_offset as usize + counts.len()) as u32;
        for &(tag, field_type, count, _) in &entries {
            let value = match tag {
                TAG_META_DATA_BYTE_COUNTS => counts_offset,
                TAG_META_DATA => metadata_offset,
                _ => {
                    let base = base_entries(width, height, 8, 1, 1, 1, width * height);
                    base.iter()
                        .find(|entry| entry.0 == tag)
                        .map(|entry| entry.3)
                        .unwrap()
                }
            };
            out.extend_from_slice(&tag.to_le_bytes());
            out.extend_from_slice(&field_type.to_le_bytes());
            out.extend_from_slice(&count.to_le_bytes());
            out.extend_from_slice(&value.to_le_bytes());
        }
        out.extend_from_slice(&0u32.to_le_bytes());
        out.extend_from_slice(&counts);
        out.extend_from_slice(&metadata);
        out
    }

    fn base_entries(
        width: u32,
        height: u32,
        bits: u16,
        spp: u16,
        sample_format: u16,
        photo_interp: u16,
        strip_len: u32,
    ) -> Vec<(u16, u16, u32, u32)> {
        vec![
            (TAG_IMAGE_WIDTH, 4, 1, width),
            (TAG_IMAGE_LENGTH, 4, 1, height),
            (TAG_BITS_PER_SAMPLE, 3, 1, u32::from(bits)),
            (TAG_COMPRESSION, 3, 1, 1),
            (TAG_PHOTO_INTERP, 3, 1, u32::from(photo_interp)),
            (TAG_STRIP_OFFSETS, 4, 1, 8),
            (TAG_SAMPLES_PER_PIXEL, 3, 1, u32::from(spp)),
            (TAG_ROWS_PER_STRIP, 4, 1, height),
            (TAG_STRIP_BYTE_COUNTS, 4, 1, strip_len),
            (TAG_SAMPLE_FORMAT, 3, 1, u32::from(sample_format)),
        ]
    }

    fn load_fixture(name: &str, bytes: &[u8]) -> Result<ImageDocument, IoError> {
        let path = tmp_tiff(name);
        fs::write(&path, bytes).unwrap();
        let result = load_tiff(&path);
        let _ = fs::remove_file(&path);
        result
    }

    #[test]
    fn tiff_rejects_image_description_offset_beyond_file() {
        let mut entries = base_entries(2, 1, 8, 1, 1, 1, 2);
        entries.push((TAG_IMAGE_DESCRIPTION, 2, 32, 0xffff_0000));
        let bytes = make_tiff(&entries, &[0, 1]);
        let err = load_fixture("desc_offset", &bytes).unwrap_err();
        assert!(
            err.to_string().contains("malformed TIFF"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn tiff_rejects_image_description_count_beyond_file() {
        let mut entries = base_entries(2, 1, 8, 1, 1, 1, 2);
        entries.push((TAG_IMAGE_DESCRIPTION, 2, 1000, 8));
        let bytes = make_tiff(&entries, &[0, 1]);
        assert!(load_fixture("desc_count", &bytes).is_err());
    }

    #[test]
    fn tiff_rejects_image_dimensions_larger_than_file() {
        // Pixel count is so large that allocating it would abort the process
        // before the file-bounded check runs.
        let entries = base_entries(u32::MAX, u32::MAX, 8, 1, 1, 1, 1);
        let bytes = make_tiff(&entries, &[0]);
        assert!(load_fixture("huge_dims", &bytes).is_err());
    }

    #[test]
    fn compressed_tiff_rejects_oversized_dimensions_and_invalid_strip_layout() {
        for (index, (width, height, rows, count)) in [
            (u32::MAX, u32::MAX, 1, Some(2)),
            (1, MAX_DECODED_TIFF_BYTES as u32 + 1, 1, Some(2)),
            (2, 1, 0, Some(2)),
            (2, 3, 1, Some(2)),
            (2, 1, 1, None),
            (2, 1, 1, Some(u32::MAX)),
            (0, 1, 1, Some(2)),
        ]
        .into_iter()
        .enumerate()
        {
            let mut entries = base_entries(width, height, 8, 1, 1, 1, 2);
            entries
                .iter_mut()
                .find(|e| e.0 == TAG_COMPRESSION)
                .unwrap()
                .3 = 32773;
            entries
                .iter_mut()
                .find(|e| e.0 == TAG_ROWS_PER_STRIP)
                .unwrap()
                .3 = rows;
            if let Some(count) = count {
                entries
                    .iter_mut()
                    .find(|e| e.0 == TAG_STRIP_BYTE_COUNTS)
                    .unwrap()
                    .3 = count;
            } else {
                entries.retain(|e| e.0 != TAG_STRIP_BYTE_COUNTS);
            }
            assert!(
                load_fixture(
                    &format!("compressed_invalid_{index}"),
                    &make_tiff(&entries, &[255, 7])
                )
                .is_err()
            );
        }
    }

    #[test]
    fn tiff_rejects_overflowing_rgb_dimension_product() {
        // pixel_count * 3 overflows usize on 64-bit; must be checked.
        let entries = base_entries(u32::MAX, u32::MAX, 8, 3, 1, 2, 1);
        let bytes = make_tiff(&entries, &[0]);
        assert!(load_fixture("overflow_rgb_dims", &bytes).is_err());
    }

    #[test]
    fn tiff_rejects_excessive_strip_count() {
        let mut entries = base_entries(2, 1, 8, 1, 1, 1, 2);
        entries.retain(|entry| entry.0 != TAG_STRIP_OFFSETS);
        entries.push((TAG_STRIP_OFFSETS, 4, 1_000_000, 8));
        let bytes = make_tiff(&entries, &[0, 1]);
        assert!(load_fixture("strip_count", &bytes).is_err());
    }

    #[test]
    fn tiff_rejects_strip_byte_count_beyond_file() {
        let entries = base_entries(1, 1, 8, 1, 1, 1, u32::MAX);
        let bytes = make_tiff(&entries, &[0]);
        assert!(load_fixture("strip_len", &bytes).is_err());
    }

    #[test]
    fn tiff_rejects_planar_rgb_instead_of_silent_corruption() {
        let mut entries = base_entries(2, 1, 8, 3, 1, 2, 6);
        entries.push((TAG_PLANAR_CONFIG, 3, 1, 2));
        let bytes = make_tiff(&entries, &[1, 2, 3, 4, 5, 6]);
        assert!(load_fixture("planar_rgb", &bytes).is_err());
    }

    #[test]
    fn tiff_rejects_signed_16bit_samples() {
        let entries = base_entries(1, 1, 16, 1, 2, 1, 2);
        let bytes = make_tiff(&entries, &[0, 0]);
        assert!(load_fixture("signed16", &bytes).is_err());
    }

    #[test]
    fn tiff_rejects_unsupported_photometric_interpretation() {
        let entries = base_entries(1, 1, 8, 3, 1, 6, 3);
        let bytes = make_tiff(&entries, &[1, 2, 3]);
        assert!(load_fixture("ycbr", &bytes).is_err());
    }

    #[test]
    fn tiff_rejects_palette_without_color_map() {
        let entries = base_entries(2, 1, 8, 1, 1, 3, 2);
        let bytes = make_tiff(&entries, &[0, 1]);
        assert!(load_fixture("palette_no_map", &bytes).is_err());
    }

    #[test]
    fn tiff_palette_loads_color256_with_lut() {
        let pixels = [0u8, 1, 2, 3];
        let mut lut = Lut256::default();
        for i in 0..256 {
            lut.r[i] = i as u8;
            lut.g[i] = (255 - i) as u8;
            lut.b[i] = (i as u8).wrapping_mul(3);
        }
        let mut data = Vec::from(pixels);
        let color_map_offset = 8 + data.len() as u32;
        for channel in [&lut.r, &lut.g, &lut.b] {
            for &value in channel {
                // TIFF stores 16-bit channels; ImageJ reads the high byte.
                data.extend_from_slice(&(u16::from(value) * 257).to_le_bytes());
            }
        }
        let mut entries = base_entries(2, 2, 8, 1, 1, 3, pixels.len() as u32);
        entries.push((TAG_COLOR_MAP, 3, 768, color_map_offset));

        let bytes = make_tiff(&entries, &data);
        let doc = load_fixture("palette_load", &bytes).unwrap();
        assert_eq!(
            doc.slices,
            vec![PixelPlane::Color256 {
                pixels: pixels.to_vec(),
                lut: Box::new(lut),
            }]
        );
    }

    #[test]
    fn tiff_gray8_color_map_loads_as_color256_with_lut() {
        // getColorMap also runs for PhotometricInterpretation 0/1
        // (TiffDecoder.java 536) and 8-bit color maps are applied through
        // createColorModel (FileOpener.java 509-514).
        let pixels = [1u8, 8, 15, 22];
        let map = fixture_color_map(3);
        let mut data = Vec::from(pixels);
        data.extend_from_slice(&map);
        let mut entries = base_entries(2, 2, 8, 1, 1, 1, pixels.len() as u32);
        entries.push((TAG_COLOR_MAP, 3, 768, (8 + pixels.len()) as u32));

        let doc = load_fixture("gray8_color_map", &make_tiff(&entries, &data)).unwrap();
        assert_eq!(
            doc.slices,
            vec![PixelPlane::Color256 {
                pixels: pixels.to_vec(),
                lut: Box::new(fixture_lut(3)),
            }]
        );
    }

    #[test]
    fn tiff_gray16_color_map_is_ignored_only_without_description() {
        let values = [5u16, 1239, 2473, 3707];
        let pixel_bytes: Vec<u8> = values
            .iter()
            .flat_map(|value| value.to_le_bytes())
            .collect();
        let map = fixture_color_map(3);
        let mut data = pixel_bytes.clone();
        data.extend_from_slice(&map);
        let mut entries = base_entries(2, 2, 16, 1, 1, 1, pixel_bytes.len() as u32);
        entries.push((TAG_COLOR_MAP, 3, 768, (8 + pixel_bytes.len()) as u32));

        // TiffDecoder discards the LUT for GRAY16_UNSIGNED without an
        // ImageDescription (TiffDecoder.java 850-851).
        let doc = load_fixture("gray16_color_map_none", &make_tiff(&entries, &data)).unwrap();
        assert_eq!(doc.slices, vec![PixelPlane::Gray16(values.to_vec())]);

        // A description of at least 7 characters makes Java keep and apply the
        // LUT, which Gray16 cannot represent.
        let description = b"ImageJ=1.54u\n\0";
        let mut data = pixel_bytes.clone();
        data.extend_from_slice(&map);
        let description_offset = 8 + pixel_bytes.len() + map.len();
        data.extend_from_slice(description);
        let mut entries = base_entries(2, 2, 16, 1, 1, 1, pixel_bytes.len() as u32);
        entries.push((TAG_COLOR_MAP, 3, 768, (8 + pixel_bytes.len()) as u32));
        entries.push((
            TAG_IMAGE_DESCRIPTION,
            2,
            description.len() as u32,
            description_offset as u32,
        ));
        let err = load_fixture("gray16_color_map_desc", &make_tiff(&entries, &data)).unwrap_err();
        assert!(
            err.to_string().contains("16-bit") && err.to_string().contains("LUT"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn tiff_gray32_color_map_is_rejected() {
        let pixel_bytes: Vec<u8> = [0.5f32, 1.5, 2.5, 3.5]
            .iter()
            .flat_map(|value| value.to_le_bytes())
            .collect();
        let map = fixture_color_map(3);
        let mut data = pixel_bytes.clone();
        data.extend_from_slice(&map);
        let mut entries = base_entries(2, 2, 32, 1, 3, 1, pixel_bytes.len() as u32);
        entries.push((TAG_COLOR_MAP, 3, 768, (8 + pixel_bytes.len()) as u32));

        // 32-bit files keep the LUT regardless of the description, and Float32
        // has no LUT field in the Rust model.
        let err = load_fixture("gray32_color_map", &make_tiff(&entries, &data)).unwrap_err();
        assert!(
            err.to_string().contains("32-bit") && err.to_string().contains("LUT"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn tiff_color256_save_roundtrip_preserves_palette() {
        let mut lut = Lut256::default();
        for i in 0..256 {
            lut.r[i] = (255 - i) as u8;
            lut.g[i] = (i / 2) as u8;
            lut.b[i] = (i * 7 % 256) as u8;
        }
        let plane = PixelPlane::Color256 {
            pixels: vec![0, 1, 2, 3, 4, 5],
            lut: Box::new(lut),
        };
        let doc = ImageDocument::new("palette.tif", 3, 2, plane.clone()).unwrap();
        let path = tmp_tiff("palette_roundtrip");
        save_tiff(&path, &doc).unwrap();
        let loaded = load_tiff(&path).unwrap();
        let _ = fs::remove_file(&path);
        assert_eq!(loaded.plane(), &plane);
        assert_eq!(loaded.get_dimensions(), [3, 2, 1, 1, 1]);
    }

    #[test]
    fn tiff_color256_hyperstack_roundtrip_preserves_palette_and_dimensions() {
        let mut lut = Lut256::default();
        for i in 0..256 {
            lut.r[i] = (i % 7) as u8;
            lut.g[i] = (255 - i) as u8;
            lut.b[i] = ((i * 11) % 256) as u8;
        }
        let slices: Vec<PixelPlane> = (0..2u8)
            .map(|offset| PixelPlane::Color256 {
                pixels: (0..6).map(|i| i + offset).collect(),
                lut: Box::new(lut.clone()),
            })
            .collect();
        let doc = ImageDocument::new_hyperstack("palette_hyper.tif", 3, 2, slices.clone(), 2, 1, 1)
            .unwrap();
        let path = tmp_tiff("palette_hyperstack");
        save_tiff(&path, &doc).unwrap();
        let loaded = load_tiff(&path).unwrap();
        let _ = fs::remove_file(&path);
        assert_eq!(loaded.slices, slices);
        assert_eq!(loaded.get_dimensions(), [3, 2, 2, 1, 1]);
    }

    #[test]
    fn tiff_color256_stack_with_mixed_luts_is_rejected() {
        let lut_a = Lut256::default();
        let mut lut_b = Lut256::default();
        lut_b.r[0] = 7;
        let slices = vec![
            PixelPlane::Color256 {
                pixels: vec![0; 4],
                lut: Box::new(lut_a),
            },
            PixelPlane::Color256 {
                pixels: vec![1; 4],
                lut: Box::new(lut_b),
            },
        ];
        let doc = ImageDocument::new_stack("mixed.tif", 2, 2, slices).unwrap();
        let path = tmp_tiff("mixed_luts");
        let err = save_tiff(&path, &doc).unwrap_err();
        let _ = fs::remove_file(&path);
        assert!(
            err.to_string().contains("palette"),
            "unexpected error: {err}"
        );
    }

    fn fixture_color_map(seed: u8) -> Vec<u8> {
        let mut map = Vec::with_capacity(768 * 2);
        for channel in 0..3 {
            for i in 0..256u32 {
                let value = match channel {
                    0 => (i as u8).wrapping_add(seed),
                    1 => (255 - i) as u8,
                    _ => seed,
                };
                map.extend_from_slice(&(u16::from(value) * 257).to_le_bytes());
            }
        }
        map
    }

    fn fixture_lut(seed: u8) -> Lut256 {
        let mut lut = Lut256::default();
        for i in 0..256 {
            lut.r[i] = (i as u8).wrapping_add(seed);
            lut.g[i] = (255 - i) as u8;
            lut.b[i] = seed;
        }
        lut
    }

    fn push_ifd_entry(out: &mut Vec<u8>, tag: u16, field_type: u16, count: u32, value: u32) {
        out.extend_from_slice(&tag.to_le_bytes());
        out.extend_from_slice(&field_type.to_le_bytes());
        out.extend_from_slice(&count.to_le_bytes());
        if field_type == 3 && count == 1 {
            out.extend_from_slice(&(value as u16).to_le_bytes());
            out.extend_from_slice(&0u16.to_le_bytes());
        } else {
            out.extend_from_slice(&value.to_le_bytes());
        }
    }

    /// Builds a little-endian palette TIFF with one IFD per slice and a
    /// separate ColorMap for each IFD.
    fn make_multi_ifd_palette_tiff(width: u32, height: u32, seeds: &[u8]) -> Vec<u8> {
        let plane_bytes = (width * height) as usize;
        let mut out = vec![0u8; 8];
        out[..2].copy_from_slice(b"II");
        out[2..4].copy_from_slice(&42u16.to_le_bytes());
        let mut strip_offsets = Vec::new();
        for seed in seeds {
            strip_offsets.push(out.len() as u32);
            for i in 0..plane_bytes {
                out.push((i as u8).wrapping_mul(7).wrapping_add(*seed));
            }
        }
        if out.len() % 2 == 1 {
            out.push(0);
        }

        let n = seeds.len();
        let num_entries = 11usize;
        let ifd_start = out.len();
        let ifd_size = 2 + num_entries * 12 + 4;
        let mut map_offsets = Vec::new();
        let mut cursor = ifd_start + ifd_size * n;
        for _ in 0..n {
            if cursor % 2 == 1 {
                cursor += 1;
            }
            map_offsets.push(cursor);
            cursor += 768 * 2;
        }
        out[4..8].copy_from_slice(&(ifd_start as u32).to_le_bytes());

        for (index, (offset, map_offset)) in strip_offsets.iter().zip(&map_offsets).enumerate() {
            let next_ifd = if index + 1 < n {
                ifd_start + (index + 1) * ifd_size
            } else {
                0
            };
            out.extend_from_slice(&(num_entries as u16).to_le_bytes());
            push_ifd_entry(&mut out, TAG_IMAGE_WIDTH, 4, 1, width);
            push_ifd_entry(&mut out, TAG_IMAGE_LENGTH, 4, 1, height);
            push_ifd_entry(&mut out, TAG_BITS_PER_SAMPLE, 3, 1, 8);
            push_ifd_entry(&mut out, TAG_COMPRESSION, 3, 1, 1);
            push_ifd_entry(&mut out, TAG_PHOTO_INTERP, 3, 1, 3);
            push_ifd_entry(&mut out, TAG_STRIP_OFFSETS, 4, 1, *offset);
            push_ifd_entry(&mut out, TAG_SAMPLES_PER_PIXEL, 3, 1, 1);
            push_ifd_entry(&mut out, TAG_ROWS_PER_STRIP, 4, 1, height);
            push_ifd_entry(&mut out, TAG_STRIP_BYTE_COUNTS, 4, 1, plane_bytes as u32);
            push_ifd_entry(&mut out, TAG_COLOR_MAP, 3, 768, *map_offset as u32);
            push_ifd_entry(&mut out, TAG_SAMPLE_FORMAT, 3, 1, 1);
            out.extend_from_slice(&(next_ifd as u32).to_le_bytes());
        }

        for (seed, map_offset) in seeds.iter().zip(&map_offsets) {
            while out.len() < *map_offset {
                out.push(0);
            }
            out.extend_from_slice(&fixture_color_map(*seed));
        }
        out
    }

    /// Builds a two-IFD 8-bit stack; each slice optionally carries a ColorMap.
    fn make_two_slice_ifd_tiff(palettes: [Option<u8>; 2]) -> Vec<u8> {
        let plane_bytes = 4usize;
        let mut out = vec![0u8; 8];
        out[..2].copy_from_slice(b"II");
        out[2..4].copy_from_slice(&42u16.to_le_bytes());
        let mut strip_offsets = Vec::new();
        for seed in 0..2u8 {
            strip_offsets.push(out.len() as u32);
            out.extend((0..plane_bytes).map(|i| i as u8 + seed * 7 + 1));
        }
        if out.len() % 2 == 1 {
            out.push(0);
        }

        let entry_counts: Vec<usize> = palettes
            .iter()
            .map(|palette| 10 + usize::from(palette.is_some()))
            .collect();
        let first_ifd = out.len();
        let second_ifd = first_ifd + 2 + entry_counts[0] * 12 + 4;
        let mut map_offset = second_ifd + 2 + entry_counts[1] * 12 + 4;
        out[4..8].copy_from_slice(&(first_ifd as u32).to_le_bytes());

        let mut map_offsets = Vec::new();
        for palette in &palettes {
            if palette.is_some() {
                if map_offset % 2 == 1 {
                    map_offset += 1;
                }
                map_offsets.push(Some(map_offset));
                map_offset += 768 * 2;
            } else {
                map_offsets.push(None);
            }
        }

        for index in 0..palettes.len() {
            let next_ifd = if index == 0 { second_ifd } else { 0 };
            out.extend_from_slice(&(entry_counts[index] as u16).to_le_bytes());
            push_ifd_entry(&mut out, TAG_IMAGE_WIDTH, 4, 1, 2);
            push_ifd_entry(&mut out, TAG_IMAGE_LENGTH, 4, 1, 2);
            push_ifd_entry(&mut out, TAG_BITS_PER_SAMPLE, 3, 1, 8);
            push_ifd_entry(&mut out, TAG_COMPRESSION, 3, 1, 1);
            push_ifd_entry(&mut out, TAG_PHOTO_INTERP, 3, 1, 1);
            push_ifd_entry(&mut out, TAG_STRIP_OFFSETS, 4, 1, strip_offsets[index]);
            push_ifd_entry(&mut out, TAG_SAMPLES_PER_PIXEL, 3, 1, 1);
            push_ifd_entry(&mut out, TAG_ROWS_PER_STRIP, 4, 1, 2);
            push_ifd_entry(&mut out, TAG_STRIP_BYTE_COUNTS, 4, 1, plane_bytes as u32);
            if let Some(map_offset) = map_offsets[index] {
                push_ifd_entry(&mut out, TAG_COLOR_MAP, 3, 768, map_offset as u32);
            }
            push_ifd_entry(&mut out, TAG_SAMPLE_FORMAT, 3, 1, 1);
            out.extend_from_slice(&(next_ifd as u32).to_le_bytes());
        }

        for (palette, map_offset) in palettes.iter().zip(&map_offsets) {
            if let (Some(seed), Some(map_offset)) = (palette, map_offset) {
                while out.len() < *map_offset {
                    out.push(0);
                }
                out.extend_from_slice(&fixture_color_map(*seed));
            }
        }
        out
    }

    #[test]
    fn tiff_load_rejects_stack_that_mixes_grayscale_and_palette_slices() {
        // Opener.openTiffStack only builds a stack when every IFD has the same
        // FileInfo.fileType; mixed GRAY8/COLOR8 IFDs open as the first image
        // (Opener.java 794-819, 1144-1150).
        for palettes in [[None, Some(3)], [Some(3), None]] {
            let bytes = make_two_slice_ifd_tiff(palettes);
            let err = load_fixture("gray_palette_stack", &bytes).unwrap_err();
            assert!(
                err.to_string().contains("palette"),
                "unexpected error: {err}"
            );
        }
    }

    #[test]
    fn tiff_load_rejects_multi_ifd_palette_stack_with_different_luts() {
        // ImageJ applies IFD1's palette to every slice of this file
        // (FileOpener.openStack), but the Rust model preserves each slice's
        // LUT and save_tiff rejects mixed palettes. Loading must therefore
        // fail with an explicit palette error instead of silently keeping
        // per-slice LUTs that ImageJ would not use.
        let bytes = make_multi_ifd_palette_tiff(2, 2, &[0, 9]);
        let err = load_fixture("multi_ifd_mixed_palette", &bytes).unwrap_err();
        assert!(
            err.to_string().contains("palette"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn tiff_load_accepts_multi_ifd_palette_stack_with_identical_luts() {
        let bytes = make_multi_ifd_palette_tiff(2, 2, &[0, 0]);
        let doc = load_fixture("multi_ifd_same_palette", &bytes).unwrap();
        assert_eq!(doc.stack_size(), 2);
        let expected_lut = Box::new(Lut256 {
            r: (0..=255u8).collect(),
            g: (0..=255u8).rev().collect(),
            b: vec![0u8; 256],
        });
        for (index, plane) in doc.slices.iter().enumerate() {
            let PixelPlane::Color256 { pixels, lut } = plane else {
                panic!("slice {index} is not Color256: {plane:?}");
            };
            assert_eq!(**lut, *expected_lut);
            assert_eq!(
                pixels,
                &(0..4u8).map(|i| i.wrapping_mul(7)).collect::<Vec<_>>()
            );
        }
    }

    #[test]
    fn tiff_load_converts_tiny_cm_pixel_size_to_microns_without_imagej_description() {
        // 2000 pixels/cm means 0.0005 cm per pixel. Without an ImageJ
        // description FileOpener.setCalibration uses the 0.001 threshold and
        // converts to 5 um (FileOpener.java 398-408, Prefs.java 181).
        let data = vec![0u8; 16];
        let mut bytes = data.clone();
        bytes.extend_from_slice(&2000u32.to_le_bytes());
        bytes.extend_from_slice(&1u32.to_le_bytes());
        bytes.extend_from_slice(&2000u32.to_le_bytes());
        bytes.extend_from_slice(&1u32.to_le_bytes());
        let rational_offset = 8 + data.len() as u32;
        let mut entries = base_entries(4, 4, 8, 1, 1, 1, data.len() as u32);
        entries.push((TAG_X_RESOLUTION, 5, 1, rational_offset));
        entries.push((TAG_Y_RESOLUTION, 5, 1, rational_offset + 8));
        entries.push((TAG_RESOLUTION_UNIT, 3, 1, 3));

        let doc = load_fixture("cm_microns", &make_tiff(&entries, &bytes)).unwrap();
        assert_eq!(doc.calibration.unit, "\u{00b5}m");
        assert!((doc.calibration.pixel_width - 5.0).abs() < 1e-12);
        assert!((doc.calibration.pixel_height - 5.0).abs() < 1e-12);
    }

    #[test]
    fn tiff_load_applies_imagej_description_micron_threshold_and_depth_conversion() {
        let slices = vec![PixelPlane::Gray8(vec![0u8; 4]); 2];
        let mut doc = ImageDocument::new_stack("cm_stack.tif", 2, 2, slices).unwrap();
        doc.calibration.pixel_width = 0.00005;
        doc.calibration.pixel_height = 0.00005;
        doc.calibration.pixel_depth = 0.0002;
        doc.calibration.unit = "cm".to_string();

        let path = tmp_tiff("cm_imagej_threshold");
        save_tiff(&path, &doc).unwrap();
        let loaded = load_tiff(&path).unwrap();
        let _ = fs::remove_file(&path);
        assert_eq!(loaded.calibration.unit, "\u{00b5}m");
        assert!((loaded.calibration.pixel_width - 0.5).abs() < 1e-12);
        assert!((loaded.calibration.pixel_height - 0.5).abs() < 1e-12);
        assert!((loaded.calibration.pixel_depth - 2.0).abs() < 1e-12);

        // An ImageJ description raises the threshold to 0.0001 cm, so a
        // 0.0005 cm pixel stays in centimetres.
        doc.calibration.pixel_width = 0.0005;
        doc.calibration.pixel_height = 0.0005;
        doc.calibration.pixel_depth = 1.0;
        let path = tmp_tiff("cm_imagej_above_threshold");
        save_tiff(&path, &doc).unwrap();
        let loaded = load_tiff(&path).unwrap();
        let _ = fs::remove_file(&path);
        assert_eq!(loaded.calibration.unit, "cm");
        assert!((loaded.calibration.pixel_width - 0.0005).abs() < 1e-15);
        assert!((loaded.calibration.pixel_height - 0.0005).abs() < 1e-15);
        assert_eq!(loaded.calibration.pixel_depth, 1.0);
    }

    #[test]
    fn tiff_roundtrip_preserves_display_range_scale_spacing_and_unit() {
        let slices = vec![PixelPlane::Gray8(vec![1u8; 6]); 3];
        let mut doc = ImageDocument::new_stack("metadata.tif", 3, 2, slices).unwrap();
        doc.display_range = DisplayRange {
            min: 10.0,
            max: 200.0,
        };
        doc.calibration.pixel_width = 0.5;
        doc.calibration.pixel_height = 0.25;
        doc.calibration.pixel_depth = 2.0;
        doc.calibration.unit = "um".to_string();
        doc.calibration.invert_y = true;
        let path = tmp_tiff("metadata_roundtrip");
        save_tiff(&path, &doc).unwrap();
        let loaded = load_tiff(&path).unwrap();
        let _ = fs::remove_file(&path);

        assert_eq!(loaded.display_range, doc.display_range);
        assert_eq!(loaded.calibration.pixel_width, 0.5);
        assert_eq!(loaded.calibration.pixel_height, 0.25);
        assert_eq!(loaded.calibration.pixel_depth, 2.0);
        // Calibration.setUnit canonicalizes "um" to the micro sign.
        assert_eq!(loaded.calibration.unit, "\u{00b5}m");
        assert!(loaded.calibration.invert_y);
    }

    #[test]
    fn tiff_roundtrip_preserves_16bit_display_range() {
        let doc = ImageDocument::new(
            "range16.tif",
            2,
            2,
            PixelPlane::Gray16(vec![100, 200, 300, 400]),
        )
        .unwrap();
        let mut doc = doc;
        doc.display_range = DisplayRange {
            min: 100.0,
            max: 400.0,
        };
        let path = tmp_tiff("range16_roundtrip");
        save_tiff(&path, &doc).unwrap();
        let loaded = load_tiff(&path).unwrap();
        let _ = fs::remove_file(&path);
        assert_eq!(loaded.display_range, doc.display_range);
    }

    /// Builds a version-229 ROI record with an optional float rectangle.
    fn roi_record(
        roi_type: u8,
        bounds: [i16; 4],
        options: u16,
        float_rect: Option<[f32; 4]>,
    ) -> Vec<u8> {
        let mut record = vec![0u8; 128];
        record[..4].copy_from_slice(b"Iout");
        record[4..6].copy_from_slice(&229u16.to_be_bytes());
        record[6] = roi_type;
        record[8..10].copy_from_slice(&bounds[0].to_be_bytes());
        record[10..12].copy_from_slice(&bounds[1].to_be_bytes());
        record[12..14].copy_from_slice(&bounds[2].to_be_bytes());
        record[14..16].copy_from_slice(&bounds[3].to_be_bytes());
        record[16..18].copy_from_slice(&0u16.to_be_bytes());
        if let Some(values) = float_rect {
            for (index, value) in values.iter().enumerate() {
                let offset = 18 + index * 4;
                record[offset..offset + 4].copy_from_slice(&value.to_be_bytes());
            }
        }
        record[50..52].copy_from_slice(&options.to_be_bytes());
        record[60..64].copy_from_slice(&64i32.to_be_bytes());
        record
    }

    fn load_roi_record(name: &str, record: &[u8]) -> Result<ImageDocument, IoError> {
        load_fixture(name, &make_roi_tiff(record))
    }

    /// Builds a version-229 polygon ROI record with an explicit Header2 offset.
    fn polygon_roi_record(
        bounds: [i32; 4],
        options: u16,
        coordinates: &[(i16, i16)],
        float_coordinates: Option<&[(f32, f32)]>,
        header2_offset: usize,
    ) -> Vec<u8> {
        assert!(float_coordinates.is_none_or(|points| points.len() == coordinates.len()));
        let coordinate_end =
            64 + coordinates.len() * 4 + float_coordinates.map_or(0, |p| p.len() * 8);
        let length = coordinate_end
            .max(header2_offset.saturating_add(64))
            .max(64);
        let mut record = vec![0u8; length];
        record[..4].copy_from_slice(b"Iout");
        record[4..6].copy_from_slice(&229u16.to_be_bytes());
        record[6] = 0;
        for (index, value) in bounds.iter().enumerate() {
            let offset = 8 + index * 2;
            record[offset..offset + 2].copy_from_slice(&(*value as u16).to_be_bytes());
        }
        record[16..18].copy_from_slice(&(coordinates.len() as u16).to_be_bytes());
        record[50..52].copy_from_slice(&options.to_be_bytes());
        record[60..64].copy_from_slice(&(header2_offset as i32).to_be_bytes());
        for (index, &(x, y)) in coordinates.iter().enumerate() {
            let x_offset = 64 + index * 2;
            let y_offset = 64 + coordinates.len() * 2 + index * 2;
            record[x_offset..x_offset + 2].copy_from_slice(&x.to_be_bytes());
            record[y_offset..y_offset + 2].copy_from_slice(&y.to_be_bytes());
        }
        if let Some(float_coordinates) = float_coordinates {
            let x_base = 64 + coordinates.len() * 4;
            let y_base = x_base + coordinates.len() * 4;
            for (index, &(x, y)) in float_coordinates.iter().enumerate() {
                let x_offset = x_base + index * 4;
                let y_offset = y_base + index * 4;
                record[x_offset..x_offset + 4].copy_from_slice(&x.to_be_bytes());
                record[y_offset..y_offset + 4].copy_from_slice(&y.to_be_bytes());
            }
        }
        record
    }

    #[test]
    fn tiff_load_rejects_header2_overlapping_polygon_coordinates() {
        // Header2 starts at 68, inside the y-coordinate shorts that occupy
        // 64..76 for three points. The bytes happen to read as all-zero
        // metadata, so without an overlap check the record is accepted.
        let record = polygon_roi_record([0, 0, 10, 9], 0, &[(0, 9), (10, 0), (5, 0)], None, 68);
        let error = load_roi_record("roi_header2_overlap", &record).unwrap_err();
        let message = error.to_string();
        assert!(
            message.contains("ROI")
                && message.contains("Header2")
                && message.contains("coordinate"),
            "expected an explicit Header2 overlap error, got: {message}"
        );
    }

    #[test]
    fn tiff_roi_metadata_rejects_zero_size_polygon() {
        let record = polygon_roi_record([5, 5, 5, 5], 0, &[(0, 0), (0, 0), (0, 0)], None, 0);
        let error = load_roi_record("roi_zero_size_polygon", &record).unwrap_err();
        let message = error.to_string();
        assert!(
            message.contains("ROI") && message.contains("zero width and height"),
            "expected an explicit zero-size polygon error, got: {message}"
        );

        let mut doc = ImageDocument::new_ramp_8bit("roi_zero_size_polygon.tif", 16, 16).unwrap();
        doc.roi = Some(ij_core::Roi::Polygon(ij_core::PolygonRoi {
            xpoints: vec![5, 5, 5],
            ypoints: vec![5, 5, 5],
        }));
        let path = tmp_tiff("roi_zero_size_polygon");
        let error = save_tiff(&path, &doc).unwrap_err().to_string();
        assert!(
            error.contains("zero width and height"),
            "expected the encoder to reject a zero-size polygon, got: {error}"
        );
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn tiff_roi_metadata_rejects_zero_axis_subpixel_polygon() {
        let record = polygon_roi_record(
            [0, 70000, 0, 70050],
            128,
            &[(0, 0), (0, 10), (0, 50)],
            Some(&[(0.0, 70000.0), (0.0, 70010.0), (0.0, 70050.0)]),
            100,
        );
        let error = load_roi_record("roi_zero_axis_subpixel", &record).unwrap_err();
        let message = error.to_string();
        assert!(
            message.contains("ROI") && message.contains("subpixel"),
            "expected an explicit zero-axis subpixel error, got: {message}"
        );

        let mut doc = ImageDocument::new_ramp_8bit("roi_zero_axis.tif", 16, 16).unwrap();
        doc.roi = Some(ij_core::Roi::Polygon(ij_core::PolygonRoi {
            xpoints: vec![0, 0, 0],
            ypoints: vec![70000, 70010, 70050],
        }));
        let path = tmp_tiff("roi_zero_axis_subpixel");
        let error = save_tiff(&path, &doc).unwrap_err().to_string();
        assert!(
            error.contains("subpixel"),
            "expected the encoder to reject a zero-axis subpixel polygon, got: {error}"
        );
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn tiff_load_rejects_fractional_subpixel_rect_roi() {
        let record = roi_record(1, [3, 2, 9, 10], 128, Some([2.5, 3.0, 8.0, 6.0]));
        let error = load_roi_record("roi_subpixel_fraction", &record).unwrap_err();
        let message = error.to_string();
        assert!(
            message.contains("ROI") && message.contains("subpixel"),
            "expected an explicit subpixel ROI error, got: {message}"
        );
    }

    #[test]
    fn tiff_load_rejects_unsupported_roi_variants() {
        let record = roi_record(7, [3, 2, 9, 10], 0, None);
        let error = load_roi_record("roi_freehand", &record).unwrap_err();
        let message = error.to_string();
        assert!(
            message.contains("ROI") && message.contains("type"),
            "expected an explicit unsupported ROI type error, got: {message}"
        );
    }

    #[test]
    fn tiff_load_rejects_composite_roi_metadata() {
        let mut record = roi_record(1, [3, 2, 9, 10], 0, None);
        record[36..40].copy_from_slice(&4i32.to_be_bytes());
        let error = load_roi_record("roi_composite", &record).unwrap_err();
        let message = error.to_string();
        assert!(
            message.contains("ROI") && message.contains("composite"),
            "expected an explicit composite ROI error, got: {message}"
        );
    }

    #[test]
    fn tiff_load_rejects_roi_metadata_it_cannot_represent() {
        let mut positioned = roi_record(1, [3, 2, 9, 10], 0, None);
        positioned[56..60].copy_from_slice(&5i32.to_be_bytes());
        let error = load_roi_record("roi_position", &positioned).unwrap_err();
        let message = error.to_string();
        assert!(
            message.contains("ROI") && message.contains("position"),
            "expected an explicit ROI position error, got: {message}"
        );

        let mut stroked = roi_record(1, [3, 2, 9, 10], 0, None);
        stroked[34..36].copy_from_slice(&2u16.to_be_bytes());
        let error = load_roi_record("roi_stroke", &stroked).unwrap_err();
        let message = error.to_string();
        assert!(
            message.contains("ROI") && message.contains("stroke"),
            "expected an explicit ROI stroke error, got: {message}"
        );
    }

    fn roi_roundtrip(name: &str, roi: ij_core::Roi) {
        let mut doc = ImageDocument::new_ramp_8bit("roi.tif", 16, 16).unwrap();
        doc.roi = Some(roi.clone());
        let path = tmp_tiff(name);
        save_tiff(&path, &doc).unwrap();
        let loaded = load_tiff(&path).unwrap();
        let _ = fs::remove_file(&path);
        assert_eq!(loaded.roi, Some(roi), "{name}");
    }

    #[test]
    fn tiff_roi_metadata_roundtrips_rect_oval_polygon_and_line() {
        use ij_core::{LineRoi, PolygonRoi, RectRoi};

        roi_roundtrip(
            "roi_rect",
            ij_core::Roi::Rect(RectRoi {
                x: 2,
                y: 3,
                width: 8,
                height: 6,
            }),
        );
        roi_roundtrip(
            "roi_oval",
            ij_core::Roi::Oval(RectRoi {
                x: 2,
                y: 3,
                width: 8,
                height: 6,
            }),
        );
        roi_roundtrip(
            "roi_polygon",
            ij_core::Roi::Polygon(PolygonRoi {
                xpoints: vec![2, 12, 9, 3],
                ypoints: vec![2, 3, 11, 8],
            }),
        );
        roi_roundtrip(
            "roi_line",
            ij_core::Roi::Line(LineRoi {
                x1: 2,
                y1: 3,
                x2: 10,
                y2: 7,
            }),
        );
    }

    #[test]
    fn tiff_roi_metadata_roundtrips_off_image_and_negative_coordinates() {
        use ij_core::{LineRoi, PolygonRoi, RectRoi};

        roi_roundtrip(
            "roi_rect_off_image",
            ij_core::Roi::Rect(RectRoi {
                x: -7,
                y: -9,
                width: 10,
                height: 12,
            }),
        );
        roi_roundtrip(
            "roi_oval_off_image",
            ij_core::Roi::Oval(RectRoi {
                x: -3,
                y: -2,
                width: 8,
                height: 7,
            }),
        );
        roi_roundtrip(
            "roi_polygon_off_image",
            ij_core::Roi::Polygon(PolygonRoi {
                xpoints: vec![-2, 8, 5, 1],
                ypoints: vec![-1, 2, 9, 6],
            }),
        );
        roi_roundtrip(
            "roi_line_off_image",
            ij_core::Roi::Line(LineRoi {
                x1: -3,
                y1: 2,
                x2: 7,
                y2: -4,
            }),
        );
    }

    #[test]
    fn tiff_roi_metadata_roundtrips_coordinates_that_need_subpixel_encoding() {
        use ij_core::{LineRoi, PolygonRoi, RectRoi};

        // ImageJ's signed-short ROI header cannot represent a negative
        // coordinate below -5000 losslessly (RoiDecoder.getShort falls back to
        // an unsigned value), so the writer must use the float encoding.
        roi_roundtrip(
            "roi_rect_large_negative",
            ij_core::Roi::Rect(RectRoi {
                x: -6000,
                y: 40_000,
                width: 100,
                height: 80,
            }),
        );
        roi_roundtrip(
            "roi_rect_large_positive",
            ij_core::Roi::Rect(RectRoi {
                x: 0,
                y: 0,
                width: 65_000,
                height: 70,
            }),
        );
        roi_roundtrip(
            "roi_polygon_large_negative",
            ij_core::Roi::Polygon(PolygonRoi {
                xpoints: vec![-6000, -5900, 100],
                ypoints: vec![0, 50, 20],
            }),
        );
        roi_roundtrip(
            "roi_line_large_negative",
            ij_core::Roi::Line(LineRoi {
                x1: -6000,
                y1: 2,
                x2: 7,
                y2: -4,
            }),
        );
    }

    #[test]
    fn tiff_save_rejects_zero_display_range_that_imagej_treats_as_unset() {
        let mut doc = ImageDocument::new(
            "zero_range.tif",
            2,
            2,
            PixelPlane::Gray16(vec![100, 200, 300, 400]),
        )
        .unwrap();
        doc.display_range = DisplayRange { min: 0.0, max: 0.0 };
        let path = tmp_tiff("zero_range");
        let err = save_tiff(&path, &doc).unwrap_err();
        let _ = fs::remove_file(&path);
        assert!(
            err.to_string().contains("0/0 display range"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn tiff_save_allows_zero_display_range_when_the_data_range_is_zero() {
        // An all-zero 32-bit image reopens with ImageJ's own 0/0 range, so the
        // 0/0 written to the file is lossless.
        let doc =
            ImageDocument::new("all_zero.tif", 2, 2, PixelPlane::Gray32(vec![0.0; 4])).unwrap();
        assert_eq!(doc.display_range, DisplayRange { min: 0.0, max: 0.0 });
        let path = tmp_tiff("all_zero_range");
        save_tiff(&path, &doc).unwrap();
        let loaded = load_tiff(&path).unwrap();
        let _ = fs::remove_file(&path);
        assert_eq!(loaded.display_range, DisplayRange { min: 0.0, max: 0.0 });
    }

    #[test]
    fn tiff_save_rejects_pixel_depth_it_cannot_persist_for_a_single_image() {
        // ImageJ only writes `spacing=` when nImages>1 (FileSaver.java
        // 763-772), so a single-plane z-spacing would silently disappear.
        let mut doc = ImageDocument::new_ramp_8bit("depth.tif", 2, 2).unwrap();
        doc.calibration.pixel_depth = 2.0;
        let path = tmp_tiff("single_pixel_depth");
        let err = save_tiff(&path, &doc).unwrap_err();
        let _ = fs::remove_file(&path);
        assert!(
            err.to_string().contains("pixel depth"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn tiff_save_rejects_rgb_display_range_imagej_cannot_persist() {
        // FileSaver.getDescriptionString only writes min/max for GRAY8,
        // COLOR_256, GRAY16 and GRAY32 (FileSaver.java 775-784), and
        // FileOpener.setCalibration only restores those types.
        let mut doc =
            ImageDocument::new("rgb.tif", 2, 2, PixelPlane::ColorRgb(vec![0xff00_0000; 4]))
                .unwrap();
        doc.display_range = DisplayRange {
            min: 10.0,
            max: 200.0,
        };
        let path = tmp_tiff("rgb_display_range");
        let err = save_tiff(&path, &doc).unwrap_err();
        let _ = fs::remove_file(&path);
        assert!(
            err.to_string().contains("display range"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn tiff_color256_save_rejects_malformed_lut() {
        let lut = Lut256 {
            r: vec![0; 255],
            g: vec![0; 256],
            b: vec![0; 256],
        };
        let plane = PixelPlane::Color256 {
            pixels: vec![0; 4],
            lut: Box::new(lut),
        };
        let doc = ImageDocument::new("bad_lut.tif", 2, 2, plane).unwrap();
        let path = tmp_tiff("bad_lut");
        let err = save_tiff(&path, &doc).unwrap_err();
        let _ = fs::remove_file(&path);
        assert!(err.to_string().contains("256"), "unexpected error: {err}");
    }

    fn atomic_test_dir(name: &str) -> PathBuf {
        let dir = env::temp_dir().join(format!(
            "ij_rs_atomic_{name}_{}_{}",
            std::process::id(),
            TEMP_FILE_SEQ.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn dir_entries(dir: &Path) -> Vec<String> {
        let mut entries: Vec<String> = fs::read_dir(dir)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        entries.sort();
        entries
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum Fault {
        Create,
        PartialWrite,
        SetPermissions,
        Sync,
        Rename,
    }

    /// Delegates to the real filesystem but fails at one stage, so the writer's
    /// rollback behavior can be observed without relying on OS quirks.
    struct FaultyFs {
        fault: Fault,
        attempted_rename_payload: RefCell<Option<Vec<u8>>>,
    }

    impl FaultyFs {
        fn new(fault: Fault) -> Self {
            Self {
                fault,
                attempted_rename_payload: RefCell::new(None),
            }
        }

        fn injected(&self) -> io::Error {
            io::Error::other(format!("injected {:?} failure", self.fault))
        }
    }

    impl AtomicFileSystem for FaultyFs {
        fn symlink_metadata(&self, path: &Path) -> io::Result<fs::Metadata> {
            RealFs.symlink_metadata(path)
        }

        fn metadata(&self, path: &Path) -> io::Result<fs::Metadata> {
            RealFs.metadata(path)
        }

        fn read_link(&self, path: &Path) -> io::Result<PathBuf> {
            RealFs.read_link(path)
        }

        fn create_new(&self, path: &Path) -> io::Result<fs::File> {
            if self.fault == Fault::Create {
                return Err(self.injected());
            }
            RealFs.create_new(path)
        }

        fn write_all(&self, file: &mut fs::File, bytes: &[u8]) -> io::Result<()> {
            if self.fault == Fault::PartialWrite {
                // Deliberately leave a short file behind, the same way a torn
                // write would.
                RealFs.write_all(file, &bytes[..bytes.len() / 2])?;
                return Err(self.injected());
            }
            RealFs.write_all(file, bytes)
        }

        fn sync_all(&self, file: &fs::File) -> io::Result<()> {
            if self.fault == Fault::Sync {
                return Err(self.injected());
            }
            RealFs.sync_all(file)
        }

        fn set_permissions(&self, path: &Path, permissions: fs::Permissions) -> io::Result<()> {
            if self.fault == Fault::SetPermissions {
                return Err(self.injected());
            }
            RealFs.set_permissions(path, permissions)
        }

        fn rename(&self, from: &Path, to: &Path) -> io::Result<()> {
            if self.fault == Fault::Rename {
                // Capture what the writer was about to publish before the
                // failed rename, proving the temp file was complete.
                *self.attempted_rename_payload.borrow_mut() = Some(fs::read(from)?);
                return Err(self.injected());
            }
            RealFs.rename(from, to)
        }

        fn remove_file(&self, path: &Path) -> io::Result<()> {
            RealFs.remove_file(path)
        }
    }

    #[test]
    fn atomic_save_replaces_existing_file_and_round_trips() {
        let dir = atomic_test_dir("replace");
        let path = dir.join("image.tif");
        fs::write(&path, b"OLD SENTINEL").unwrap();
        let doc = ImageDocument::new_ramp_8bit("image.tif", 8, 6).unwrap();

        save_tiff(&path, &doc).unwrap();

        let loaded = load_tiff(&path).unwrap();
        assert_eq!(loaded.plane(), doc.plane());
        assert_ne!(fs::read(&path).unwrap(), b"OLD SENTINEL");
        assert_eq!(dir_entries(&dir), vec!["image.tif".to_string()]);
        let _ = fs::remove_dir_all(&dir);
    }

    #[cfg(unix)]
    #[test]
    fn atomic_save_preserves_existing_permission_bits() {
        use std::os::unix::fs::PermissionsExt;

        let dir = atomic_test_dir("permissions");
        let path = dir.join("image.tif");
        fs::write(&path, b"OLD").unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o640)).unwrap();
        let doc = ImageDocument::new_ramp_8bit("image.tif", 4, 4).unwrap();

        save_tiff(&path, &doc).unwrap();

        assert_eq!(
            fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o640
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[cfg(unix)]
    #[test]
    fn atomic_save_follows_destination_symlink() {
        use std::os::unix::fs::symlink;

        let dir = atomic_test_dir("symlink");
        let target = dir.join("target.tif");
        let link = dir.join("link.tif");
        fs::write(&target, b"OLD TARGET").unwrap();
        symlink("target.tif", &link).unwrap();
        let doc = ImageDocument::new_ramp_8bit("image.tif", 4, 4).unwrap();

        save_tiff(&link, &doc).unwrap();

        assert!(
            fs::symlink_metadata(&link)
                .unwrap()
                .file_type()
                .is_symlink(),
            "the symlink itself must not be replaced"
        );
        assert_eq!(load_tiff(&target).unwrap().plane(), doc.plane());
        assert_eq!(
            dir_entries(&dir),
            vec!["link.tif".to_string(), "target.tif".to_string()]
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn atomic_save_failures_preserve_destination_and_remove_temp_file() {
        let reference_doc = ImageDocument::new_ramp_8bit("fault.tif", 8, 6).unwrap();
        let reference_dir = atomic_test_dir("fault_reference");
        let reference_path = reference_dir.join("reference.tif");
        save_tiff(&reference_path, &reference_doc).unwrap();
        let payload = fs::read(&reference_path).unwrap();
        let _ = fs::remove_dir_all(&reference_dir);
        assert!(payload.len() > 8);

        for fault in [
            Fault::Create,
            Fault::PartialWrite,
            Fault::SetPermissions,
            Fault::Sync,
            Fault::Rename,
        ] {
            let dir = atomic_test_dir(&format!("fault_{fault:?}"));
            let destination = dir.join("dest.tif");
            let sentinel = b"existing destination sentinel".to_vec();
            fs::write(&destination, &sentinel).unwrap();

            let faulty = FaultyFs::new(fault);
            let error = write_atomic_with(&faulty, &destination, &payload).unwrap_err();

            assert!(
                error.to_string().contains("injected"),
                "{fault:?}: unexpected error: {error}"
            );
            assert_eq!(
                fs::read(&destination).unwrap(),
                sentinel,
                "{fault:?} must not truncate or partially overwrite the destination"
            );
            assert_eq!(
                dir_entries(&dir),
                vec!["dest.tif".to_string()],
                "{fault:?} must remove its temporary file"
            );

            if fault == Fault::Rename {
                let captured = faulty
                    .attempted_rename_payload
                    .borrow()
                    .clone()
                    .expect("rename must be attempted only after the payload is fully written");
                let captured_path = dir.join("captured.tif");
                fs::write(&captured_path, captured).unwrap();
                let loaded = load_tiff(&captured_path).unwrap();
                assert_eq!(loaded.plane(), reference_doc.plane());
            }

            let _ = fs::remove_dir_all(&dir);
        }
    }

    #[cfg(unix)]
    #[test]
    fn atomic_save_rejects_fifo_without_replacing_it() {
        use std::os::unix::fs::FileTypeExt;
        let dir = std::env::temp_dir().join(format!("ij-fifo-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("image.tif");
        let status = std::process::Command::new("mkfifo")
            .arg(&path)
            .status()
            .unwrap();
        assert!(status.success());
        assert!(write_atomic(&path, b"data").is_err());
        assert!(std::fs::symlink_metadata(&path)
            .unwrap()
            .file_type()
            .is_fifo());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn atomic_save_rejects_directory_destination_without_side_effects() {
        let dir = atomic_test_dir("directory_destination");
        let destination = dir.join("as_directory");
        fs::create_dir(&destination).unwrap();
        let doc = ImageDocument::new_ramp_8bit("image.tif", 4, 4).unwrap();

        let error = save_tiff(&destination, &doc).unwrap_err();

        assert!(error.to_string().contains("directory"), "{error}");
        assert!(destination.is_dir());
        assert_eq!(dir_entries(&dir), vec!["as_directory".to_string()]);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn unsupported_document_state_preserves_existing_destination() {
        let dir = atomic_test_dir("unsupported_state");
        let destination = dir.join("rgb.tif");
        let sentinel = b"existing destination sentinel".to_vec();
        fs::write(&destination, &sentinel).unwrap();
        let mut doc =
            ImageDocument::new("rgb.tif", 2, 2, PixelPlane::ColorRgb(vec![0xff00_0000; 4]))
                .unwrap();
        doc.display_range = DisplayRange {
            min: 10.0,
            max: 200.0,
        };

        let error = save_tiff(&destination, &doc).unwrap_err();

        assert!(error.to_string().contains("display range"), "{error}");
        assert_eq!(fs::read(&destination).unwrap(), sentinel);
        assert_eq!(dir_entries(&dir), vec!["rgb.tif".to_string()]);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn saved_ifd_tags_are_strictly_ascending_for_palette_and_calibrated_images() {
        let palette = ImageDocument::new(
            "palette.tif",
            2,
            2,
            PixelPlane::Color256 {
                pixels: vec![0; 4],
                lut: Box::new(Lut256::default()),
            },
        )
        .unwrap();
        let mut calibrated =
            ImageDocument::new("calibrated.tif", 2, 2, PixelPlane::Gray16(vec![7; 4])).unwrap();
        calibrated.calibration.pixel_width = 0.5;
        calibrated.calibration.pixel_height = 0.25;
        calibrated.calibration.unit = "cm".into();
        calibrated.set_rect_roi(0, 0, 1, 1).unwrap();
        let rgb = ImageDocument::new("rgb.tif", 2, 2, PixelPlane::ColorRgb(vec![0xff00_0000; 4]))
            .unwrap();

        for (name, doc) in [
            ("palette", palette),
            ("calibrated", calibrated),
            ("rgb", rgb),
        ] {
            let path = tmp_tiff(&format!("sorted_{name}"));
            save_tiff(&path, &doc).unwrap();
            let bytes = fs::read(&path).unwrap();
            let _ = fs::remove_file(&path);
            let offset = u32::from_le_bytes(bytes[4..8].try_into().unwrap()) as usize;
            let count = u16::from_le_bytes(bytes[offset..offset + 2].try_into().unwrap()) as usize;
            let tags: Vec<u16> = (0..count)
                .map(|i| {
                    let start = offset + 2 + i * 12;
                    u16::from_le_bytes(bytes[start..start + 2].try_into().unwrap())
                })
                .collect();
            assert!(
                tags.windows(2).all(|pair| pair[0] < pair[1]),
                "{name}: {tags:?}"
            );
        }
    }

    #[test]
    fn tiff_reads_big_endian_inline_short_scalar_tags() {
        let data = [9u8, 8, 7, 6, 5, 4];
        let entries = [
            (TAG_IMAGE_WIDTH, 4, 1, 3u32),
            (TAG_IMAGE_LENGTH, 4, 1, 2u32),
            (TAG_BITS_PER_SAMPLE, 3, 1, 8),
            (TAG_COMPRESSION, 3, 1, 1),
            (TAG_PHOTO_INTERP, 3, 1, 1),
            (TAG_STRIP_OFFSETS, 4, 1, 8),
            (TAG_SAMPLES_PER_PIXEL, 3, 1, 1),
            (TAG_ROWS_PER_STRIP, 4, 1, 2),
            (TAG_STRIP_BYTE_COUNTS, 4, 1, 6),
            (TAG_SAMPLE_FORMAT, 3, 1, 1),
        ];
        let doc = load_fixture(
            "big_endian_inline_short",
            &make_big_endian_tiff(&entries, &data),
        )
        .unwrap();
        assert_eq!(doc.get_dimensions(), [3, 2, 1, 1, 1]);
        assert_eq!(doc.plane(), &PixelPlane::Gray8(data.to_vec()));
    }

    #[test]
    fn tiff_accepts_scalar_tags_whose_u16_value_is_encoded_as_long() {
        let data = [1u8, 2];
        let entries = [
            (TAG_IMAGE_WIDTH, 4, 1, 2u32),
            (TAG_IMAGE_LENGTH, 4, 1, 1u32),
            (TAG_BITS_PER_SAMPLE, 4, 1, 8),
            (TAG_COMPRESSION, 4, 1, 1),
            (TAG_PHOTO_INTERP, 4, 1, 1),
            (TAG_STRIP_OFFSETS, 4, 1, 8),
            (TAG_SAMPLES_PER_PIXEL, 4, 1, 1),
            (TAG_ROWS_PER_STRIP, 4, 1, 1),
            (TAG_STRIP_BYTE_COUNTS, 4, 1, 2),
            (TAG_PLANAR_CONFIG, 4, 1, 1),
            (TAG_SAMPLE_FORMAT, 4, 1, 1),
            (TAG_RESOLUTION_UNIT, 4, 1, 1),
        ];
        let doc = load_fixture("long_encoded_scalars", &make_tiff(&entries, &data)).unwrap();
        assert_eq!(doc.plane(), &PixelPlane::Gray8(data.to_vec()));
    }

    #[test]
    fn tiff_rejects_scalar_tag_values_that_do_not_fit_16_bits() {
        let data = [1u8, 2];
        for (tag, name) in [
            (TAG_BITS_PER_SAMPLE, "BitsPerSample"),
            (TAG_COMPRESSION, "Compression"),
            (TAG_PHOTO_INTERP, "PhotometricInterpretation"),
            (TAG_SAMPLES_PER_PIXEL, "SamplesPerPixel"),
            (TAG_PLANAR_CONFIG, "PlanarConfiguration"),
            (TAG_RESOLUTION_UNIT, "ResolutionUnit"),
            (TAG_SAMPLE_FORMAT, "SampleFormat"),
        ] {
            let mut entries = base_entries(2, 1, 8, 1, 1, 1, 2);
            entries.retain(|entry| entry.0 != tag);
            entries.push((tag, 4, 1, 0x0001_0001));
            let error = load_fixture(&format!("scalar_{tag}"), &make_tiff(&entries, &data))
                .unwrap_err()
                .to_string();
            assert!(
                error.contains(name) && error.contains("65537"),
                "{name} must report the unrepresentable value, got: {error}"
            );
        }
    }

    /// TiffDecoder only decodes IMAGE_DESCRIPTION while ifdCount==1
    /// (TiffDecoder.java 555-559); a later IFD's description must not leak
    /// into the image metadata.
    #[test]
    fn tiff_uses_image_description_from_the_first_ifd_only() {
        let first = [1u8, 2, 3, 4];
        let second = [5u8, 6, 7, 8];
        let later_only = make_two_ifd_tiff(
            2,
            2,
            [&first, &second],
            [None, Some("ImageJ=1.54u\nmin=10\nmax=200\n")],
        );
        let loaded = load_fixture("description_second_ifd", &later_only).unwrap();
        assert_eq!(loaded.get_dimensions(), [2, 2, 1, 2, 1]);
        assert_eq!(
            loaded.display_range,
            DisplayRange {
                min: 0.0,
                max: 255.0
            }
        );

        let both = make_two_ifd_tiff(
            2,
            2,
            [&first, &second],
            [
                Some("ImageJ=1.54u\nmin=10\nmax=200\n"),
                Some("ImageJ=1.54u\nmin=30\nmax=90\n"),
            ],
        );
        let loaded = load_fixture("description_first_ifd", &both).unwrap();
        assert_eq!(
            loaded.display_range,
            DisplayRange {
                min: 10.0,
                max: 200.0
            }
        );
    }

    #[cfg(unix)]
    #[test]
    fn tiff_load_rejects_fifo_without_blocking_and_follows_regular_file_symlink() {
        let dir = atomic_test_dir("reader_file_type");
        let fifo = dir.join("image.fifo");
        let status = std::process::Command::new("mkfifo")
            .arg(&fifo)
            .status()
            .unwrap();
        assert!(status.success());
        let error = load_tiff(&fifo).unwrap_err();
        assert!(error.to_string().contains("regular file"), "{error}");

        let target = dir.join("image.tif");
        let link = dir.join("linked.tif");
        let doc = ImageDocument::new_ramp_8bit("linked", 2, 2).unwrap();
        save_tiff(&target, &doc).unwrap();
        std::os::unix::fs::symlink(&target, &link).unwrap();
        assert_eq!(load_tiff(&link).unwrap().plane(), doc.plane());
        fs::remove_dir_all(dir).unwrap();
    }
}
