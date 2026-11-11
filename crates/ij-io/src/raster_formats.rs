use super::{load_tiff, write_atomic, IoError};
use ij_core::{ImageDocument, PixelPlane};
use image::{DynamicImage, ImageBuffer, ImageFormat, Luma, Rgb};
use std::fs;
use std::io::{Cursor, Read};
use std::path::Path;

const MAX_RASTER_BYTES: usize = 512 * 1024 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ImageFileFormat {
    Tiff,
    Png,
    Jpeg,
}

pub fn detect_image_format(path: impl AsRef<Path>) -> Result<ImageFileFormat, IoError> {
    let path = path.as_ref();
    let mut header = Vec::with_capacity(8);
    fs::File::open(path)?.take(8).read_to_end(&mut header)?;
    if header.starts_with(b"II\x2a\0") || header.starts_with(b"MM\0\x2a") {
        Ok(ImageFileFormat::Tiff)
    } else if header.starts_with(b"\x89PNG\r\n\x1a\n") {
        Ok(ImageFileFormat::Png)
    } else if header.starts_with(&[0xff, 0xd8, 0xff]) {
        Ok(ImageFileFormat::Jpeg)
    } else {
        Err(IoError::InvalidImage(format!(
            "unrecognized image header in {}",
            path.display()
        )))
    }
}

pub fn load_image(path: impl AsRef<Path>) -> Result<ImageDocument, IoError> {
    let path = path.as_ref();
    let format = match detect_image_format(path)? {
        ImageFileFormat::Tiff => return load_tiff(path),
        ImageFileFormat::Png => ImageFormat::Png,
        ImageFileFormat::Jpeg => ImageFormat::Jpeg,
    };
    let bytes = fs::read(path)?;
    let image = image::load_from_memory_with_format(&bytes, format)?;
    let width = image.width();
    let height = image.height();
    let pixel_count = (width as usize)
        .checked_mul(height as usize)
        .ok_or_else(|| IoError::InvalidImage("image dimensions overflow".into()))?;
    if pixel_count > MAX_RASTER_BYTES / 4 {
        return Err(IoError::InvalidImage(
            "decoded image exceeds 512 MiB".into(),
        ));
    }
    let indexed_gray_png = format == ImageFormat::Png && bytes.get(25) == Some(&3);
    let plane = decoded_plane(image, indexed_gray_png);
    let title = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| "image".into());
    Ok(ImageDocument::new(title, width, height, plane)?)
}

fn decoded_plane(image: DynamicImage, indexed_gray_png: bool) -> PixelPlane {
    match image {
        DynamicImage::ImageLuma8(buffer) => PixelPlane::Gray8(buffer.into_raw()),
        DynamicImage::ImageLuma16(buffer) => PixelPlane::Gray16(buffer.into_raw()),
        DynamicImage::ImageRgb8(buffer) => {
            let raw = buffer.into_raw();
            if indexed_gray_png
                && raw
                    .chunks_exact(3)
                    .all(|rgb| rgb[0] == rgb[1] && rgb[1] == rgb[2])
            {
                PixelPlane::Gray8(raw.chunks_exact(3).map(|rgb| rgb[0]).collect())
            } else {
                PixelPlane::ColorRgb(rgb_bytes_to_pixels(&raw))
            }
        }
        other => {
            // Opener.openUsingImageIO composites alpha onto white before ImagePlus sees it.
            let raw = other.to_rgba8().into_raw();
            PixelPlane::ColorRgb(
                raw.chunks_exact(4)
                    .map(|rgba| {
                        let alpha = u32::from(rgba[3]);
                        let channel = |v: u8| {
                            ((u32::from(v) * alpha + 255 * (255 - alpha) + 127) / 255) as u8
                        };
                        0xff00_0000
                            | (u32::from(channel(rgba[0])) << 16)
                            | (u32::from(channel(rgba[1])) << 8)
                            | u32::from(channel(rgba[2]))
                    })
                    .collect(),
            )
        }
    }
}

fn rgb_bytes_to_pixels(raw: &[u8]) -> Vec<u32> {
    raw.chunks_exact(3)
        .map(|rgb| {
            0xff00_0000 | (u32::from(rgb[0]) << 16) | (u32::from(rgb[1]) << 8) | u32::from(rgb[2])
        })
        .collect()
}

fn current_plane(doc: &ImageDocument) -> Result<&PixelPlane, IoError> {
    let count = (doc.width as usize)
        .checked_mul(doc.height as usize)
        .ok_or_else(|| IoError::InvalidImage("image dimensions overflow".into()))?;
    if count == 0 || count > MAX_RASTER_BYTES / 4 {
        return Err(IoError::InvalidImage(
            "invalid or excessive image dimensions".into(),
        ));
    }
    let plane = doc
        .slices
        .get(doc.current_slice.saturating_sub(1) as usize)
        .ok_or_else(|| IoError::InvalidImage("missing current image slice".into()))?;
    if plane.len() != count {
        return Err(IoError::InvalidImage(
            "pixel count does not match dimensions".into(),
        ));
    }
    Ok(plane)
}

fn rgb_from_plane(plane: &PixelPlane) -> Result<Vec<u8>, IoError> {
    match plane {
        PixelPlane::ColorRgb(pixels) => Ok(pixels
            .iter()
            .flat_map(|value| {
                [
                    ((value >> 16) & 255) as u8,
                    ((value >> 8) & 255) as u8,
                    (value & 255) as u8,
                ]
            })
            .collect()),
        PixelPlane::Color256 { pixels, lut } => {
            if lut.r.len() != 256 || lut.g.len() != 256 || lut.b.len() != 256 {
                return Err(IoError::InvalidImage(
                    "invalid 256-color lookup table".into(),
                ));
            }
            Ok(pixels
                .iter()
                .flat_map(|&index| {
                    let i = usize::from(index);
                    [lut.r[i], lut.g[i], lut.b[i]]
                })
                .collect())
        }
        _ => Err(IoError::InvalidImage(
            "plane is not RGB or indexed color".into(),
        )),
    }
}

fn display_gray8(doc: &ImageDocument, plane: &PixelPlane) -> Result<Vec<u8>, IoError> {
    match plane {
        PixelPlane::Gray8(values) => Ok(values.clone()),
        PixelPlane::Gray16(values) => {
            let min = doc.display_range.min as i32;
            let max = doc.display_range.max as i32;
            let scale = 256.0 / f64::from(max - min + 1);
            Ok(values
                .iter()
                .map(|&value| {
                    (((f64::from(i32::from(value) - min).max(0.0) * scale) + 0.5) as i32)
                        .clamp(0, 255) as u8
                })
                .collect())
        }
        PixelPlane::Gray32(values) => {
            let min = doc.display_range.min;
            let scale = 255.0 / (doc.display_range.max - min);
            Ok(values
                .iter()
                .map(|&value| {
                    (((f64::from(value) - min).max(0.0) * scale + 0.5) as i32).clamp(0, 255) as u8
                })
                .collect())
        }
        _ => Err(IoError::InvalidImage("plane is not grayscale".into())),
    }
}

fn dynamic_for_png(doc: &ImageDocument, plane: &PixelPlane) -> Result<DynamicImage, IoError> {
    let (w, h) = (doc.width, doc.height);
    let image = match plane {
        PixelPlane::Gray8(values) => DynamicImage::ImageLuma8(
            ImageBuffer::<Luma<u8>, _>::from_raw(w, h, values.clone()).unwrap(),
        ),
        PixelPlane::Gray16(values) => DynamicImage::ImageLuma16(
            ImageBuffer::<Luma<u16>, _>::from_raw(w, h, values.clone()).unwrap(),
        ),
        PixelPlane::Gray32(_) => DynamicImage::ImageLuma8(
            ImageBuffer::<Luma<u8>, _>::from_raw(w, h, display_gray8(doc, plane)?).unwrap(),
        ),
        PixelPlane::ColorRgb(_) | PixelPlane::Color256 { .. } => DynamicImage::ImageRgb8(
            ImageBuffer::<Rgb<u8>, _>::from_raw(w, h, rgb_from_plane(plane)?).unwrap(),
        ),
    };
    Ok(image)
}

pub fn save_png(path: impl AsRef<Path>, doc: &ImageDocument) -> Result<(), IoError> {
    let plane = current_plane(doc)?;
    let image = dynamic_for_png(doc, plane)?;
    let mut cursor = Cursor::new(Vec::new());
    image.write_to(&mut cursor, ImageFormat::Png)?;
    write_atomic(path.as_ref(), &cursor.into_inner())?;
    Ok(())
}

pub fn save_jpeg(path: impl AsRef<Path>, doc: &ImageDocument, quality: u8) -> Result<(), IoError> {
    let plane = current_plane(doc)?;
    let image = match plane {
        PixelPlane::ColorRgb(_) | PixelPlane::Color256 { .. } => DynamicImage::ImageRgb8(
            ImageBuffer::<Rgb<u8>, _>::from_raw(doc.width, doc.height, rgb_from_plane(plane)?)
                .unwrap(),
        ),
        _ => DynamicImage::ImageLuma8(
            ImageBuffer::<Luma<u8>, _>::from_raw(doc.width, doc.height, display_gray8(doc, plane)?)
                .unwrap(),
        ),
    };
    let mut bytes = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut bytes, quality.clamp(1, 100))
        .encode_image(&image)?;
    write_atomic(path.as_ref(), &bytes)?;
    Ok(())
}
