//! `File > New > Image...` pixel construction, mirroring
//! `original_imagej/ij/gui/NewImage.java` (`createByteImage`,
//! `createShortImage`, `createFloatImage`, `createRGBImage`, `createStack`)
//! and the fill defaults chosen in `IJ.createImage`
//! (original_imagej/ij/IJ.java 2293-2318).

use crate::{CoreError, DisplayRange, ImageDocument, PixelPlane};

/// Bit depth choices in `NewImage.types` order (NewImage.java 34).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NewImageType {
    Gray8,
    Gray16,
    Gray32,
    Rgb,
}

/// `Fill with:` choices in `NewImage.fill` order (NewImage.java 35). The
/// dialog choice index doubles as the `options` fill code, where index 0 is
/// the legacy `OLD_FILL_WHITE` that maps to White in `getFill`
/// (NewImage.java 106-113).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NewImageFill {
    White,
    Black,
    Ramp,
    Noise,
}

/// Deterministic Gaussian source for `FILL_NOISE`. Java draws from an
/// unseeded `java.util.Random`, so no pixel-exact parity is possible; the
/// distribution (Gaussian, with rejection sampling clamped to the pixel
/// range for 8/16-bit) is what must match. A caller-supplied seed keeps
/// `ij-core` free of entropy sources.
struct NoiseRng {
    state: u64,
    spare: Option<f64>,
}

impl NoiseRng {
    fn new(seed: u64) -> Self {
        Self {
            state: seed | 1,
            spare: None,
        }
    }

    fn next_u64(&mut self) -> u64 {
        // xorshift64*
        let mut x = self.state;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.state = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    fn uniform(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 * (1.0 / 9007199254740992.0)
    }

    fn gaussian(&mut self) -> f64 {
        if let Some(spare) = self.spare.take() {
            return spare;
        }
        let u = self.uniform().max(f64::MIN_POSITIVE);
        let v = self.uniform();
        let r = (-2.0 * u.ln()).sqrt();
        let theta = 2.0 * std::f64::consts::PI * v;
        self.spare = Some(r * theta.sin());
        r * theta.cos()
    }

    /// ByteProcessor.noise / ShortProcessor.noise rejection sampling: the
    /// rounded Gaussian draw is retried until the result lands in range
    /// (ByteProcessor.java 872-891, ShortProcessor.java 1178-1199).
    fn noise_added(&mut self, base: i64, sd: f64, lo: i64, hi: i64) -> i64 {
        loop {
            let ran = (self.gaussian() * sd).round() as i64;
            let v = base + ran;
            if v >= lo && v <= hi {
                return v;
            }
        }
    }
}

fn new_image_error(width: u32, height: u32) -> CoreError {
    CoreError::InvalidDimensions { width, height }
}

impl ImageDocument {
    /// `NewImage.createImage` for one or more slices. `slices` below 1 is
    /// clamped like `showDialog` (NewImage.java 429). Width or height below 1
    /// is rejected with the dialog's message; `width*height` above 2
    /// gigapixels is rejected like `getSize` (NewImage.java 357-364).
    ///
    /// Slice 1 follows the chosen fill; slices 2+ follow `createStack` —
    /// White/Ramp arraycopy slice 1, Black allocates zero-filled arrays
    /// (leaving RGB slices at `0x00000000`, not `0xff000000`), and Noise
    /// draws fresh noise per slice.
    ///
    /// The 16/32-bit White fill's `invertLut()` has no representable LUT on
    /// `Gray16`/`Gray32` planes, so only the pixel (zero) side is modeled.
    pub fn new_image(
        title: impl Into<String>,
        image_type: NewImageType,
        fill: NewImageFill,
        width: u32,
        height: u32,
        slices: u32,
        noise_seed: u64,
    ) -> Result<Self, CoreError> {
        if width < 1 || height < 1 {
            return Err(new_image_error(width, height));
        }
        if (width as u64) * (height as u64) > i32::MAX as u64 {
            // NewImage.getSize / IJ.error("Image is too large...").
            return Err(new_image_error(width, height));
        }
        let slices = slices.max(1);
        let n = (width * height) as usize;
        let mut rng = NoiseRng::new(noise_seed);
        let first = Self::new_image_plane(image_type, fill, width, height, &mut rng);
        let mut planes = Vec::with_capacity(slices as usize);
        planes.push(first);
        for _ in 1..slices {
            planes.push(match fill {
                // createStack arraycopies for White/Ramp only (NewImage.java
                // 82-83); Black allocates zero arrays — for RGB that is the
                // transparent 0x00000000, not opaque 0xff000000.
                NewImageFill::White | NewImageFill::Ramp => planes[0].clone(),
                // createStack allocates bare zero arrays for Black; for RGB
                // that is the transparent 0x00000000, not the opaque
                // 0xff000000 produced by createRGBImage (NewImage.java 76-88).
                NewImageFill::Black => match image_type {
                    NewImageType::Gray8 => PixelPlane::Gray8(vec![0; n]),
                    NewImageType::Gray16 => PixelPlane::Gray16(vec![0; n]),
                    NewImageType::Gray32 => PixelPlane::Gray32(vec![0.0; n]),
                    NewImageType::Rgb => PixelPlane::ColorRgb(vec![0; n]),
                },
                NewImageFill::Noise => {
                    Self::new_image_plane(image_type, NewImageFill::Noise, width, height, &mut rng)
                }
            });
        }
        let mut doc = Self::new_stack(title, width, height, planes)?;
        match image_type {
            // NewImage.java 304: the 16-bit default display range is always
            // 0..65535, even for noise.
            NewImageType::Gray16 => {
                doc.display_range = DisplayRange {
                    min: 0.0,
                    max: 65535.0,
                };
            }
            // NewImage.java 345-347: non-noise 32-bit gets 0..1; noise runs
            // FloatProcessor.noise -> resetMinAndMax on slice 1's data.
            NewImageType::Gray32 if fill != NewImageFill::Noise => {
                doc.display_range = DisplayRange { min: 0.0, max: 1.0 };
            }
            NewImageType::Gray32 => {
                if let PixelPlane::Gray32(pixels) = &doc.slices[0] {
                    let (min, max) = pixels.iter().fold(
                        (f32::INFINITY, f32::NEG_INFINITY),
                        |(lo, hi), v| (lo.min(*v), hi.max(*v)),
                    );
                    doc.display_range = DisplayRange {
                        min: min as f64,
                        max: max as f64,
                    };
                }
            }
            _ => {}
        }
        Ok(doc)
    }

    fn new_image_plane(
        image_type: NewImageType,
        fill: NewImageFill,
        width: u32,
        height: u32,
        rng: &mut NoiseRng,
    ) -> PixelPlane {
        let n = (width * height) as usize;
        match image_type {
            NewImageType::Gray8 => {
                let pixels = match fill {
                    NewImageFill::White => vec![255u8; n],
                    NewImageFill::Black => vec![0u8; n],
                    // NewImage.java 129-140: (byte)((i*256.0)/width)
                    NewImageFill::Ramp => {
                        let ramp: Vec<u8> = (0..width)
                            .map(|x| ((x as f64 * 256.0) / width as f64) as u8)
                            .collect();
                        let mut pixels = vec![0u8; n];
                        for y in 0..height as usize {
                            let row = y * width as usize;
                            pixels[row..row + width as usize].copy_from_slice(&ramp);
                        }
                        pixels
                    }
                    // fillNoiseByte: add(127) then noise(31)
                    NewImageFill::Noise => (0..n)
                        .map(|_| rng.noise_added(127, 31.0, 0, 255) as u8)
                        .collect(),
                };
                PixelPlane::Gray8(pixels)
            }
            NewImageType::Gray16 => {
                let pixels = match fill {
                    // White is also zeros; the whiteness comes from
                    // invertLut() (NewImage.java 295-296, 316-317).
                    NewImageFill::White | NewImageFill::Black => vec![0u16; n],
                    // (short)(((i*65536.0)/width)+0.5)
                    NewImageFill::Ramp => {
                        let ramp: Vec<u16> = (0..width)
                            .map(|x| (((x as f64 * 65536.0) / width as f64) + 0.5) as u16)
                            .collect();
                        let mut pixels = vec![0u16; n];
                        for y in 0..height as usize {
                            let row = y * width as usize;
                            pixels[row..row + width as usize].copy_from_slice(&ramp);
                        }
                        pixels
                    }
                    // fillNoiseShort: add(32767) then noise(7940)
                    NewImageFill::Noise => (0..n)
                        .map(|_| rng.noise_added(32767, 7940.0, 0, 65535) as u16)
                        .collect(),
                };
                PixelPlane::Gray16(pixels)
            }
            NewImageType::Gray32 => {
                let pixels = match fill {
                    NewImageFill::White | NewImageFill::Black => vec![0.0f32; n],
                    // (float)((i*1.0)/width)
                    NewImageFill::Ramp => {
                        let ramp: Vec<f32> =
                            (0..width).map(|x| x as f32 / width as f32).collect();
                        let mut pixels = vec![0.0f32; n];
                        for y in 0..height as usize {
                            let row = y * width as usize;
                            pixels[row..row + width as usize].copy_from_slice(&ramp);
                        }
                        pixels
                    }
                    // fillNoiseFloat: noise(1), unclamped
                    NewImageFill::Noise => (0..n).map(|_| rng.gaussian() as f32).collect(),
                };
                PixelPlane::Gray32(pixels)
            }
            NewImageType::Rgb => {
                let pixels = match fill {
                    // -1 == 0xffffffff opaque white
                    NewImageFill::White => vec![0xffff_ffffu32; n],
                    NewImageFill::Black => vec![0xff00_0000u32; n],
                    // 0xff000000 | gray<<16 | gray<<8 | gray
                    NewImageFill::Ramp => {
                        let ramp: Vec<u32> = (0..width)
                            .map(|x| {
                                let g = ((x as f64 * 256.0) / width as f64) as u32;
                                0xff00_0000 | (g << 16) | (g << 8) | g
                            })
                            .collect();
                        let mut pixels = vec![0u32; n];
                        for y in 0..height as usize {
                            let row = y * width as usize;
                            pixels[row..row + width as usize].copy_from_slice(&ramp);
                        }
                        pixels
                    }
                    // fillNoiseRGB: three ByteProcessors each add(127) +
                    // noise(31), then setChannel (NewImage.java 219-236).
                    NewImageFill::Noise => {
                        let r: Vec<u32> = (0..n)
                            .map(|_| rng.noise_added(127, 31.0, 0, 255) as u32)
                            .collect();
                        let g: Vec<u32> = (0..n)
                            .map(|_| rng.noise_added(127, 31.0, 0, 255) as u32)
                            .collect();
                        let b: Vec<u32> = (0..n)
                            .map(|_| rng.noise_added(127, 31.0, 0, 255) as u32)
                            .collect();
                        (0..n)
                            .map(|i| 0xff00_0000 | (r[i] << 16) | (g[i] << 8) | b[i])
                            .collect()
                    }
                };
                PixelPlane::ColorRgb(pixels)
            }
        }
    }
}
