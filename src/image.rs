//! decoded raster images: loaded once, immutable forever after. the
//! immutability is the design -- a draw command snapshots an image by
//! cloning an `Rc`, so the pixels a frame paints are exactly the pixels
//! that existed when the command was recorded. lite-xl's canvas attempts
//! (#1438, #2198) needed a ref-splitting scheme and a gc anchor to get
//! the same guarantee; an image that cannot change needs nothing.

use std::path::Path;

use crate::renderer::Color;

pub struct Image {
    width: i32,
    height: i32,
    pixels: Vec<Color>,
    content_hash: u32,
}

impl Image {
    pub fn load(path: &Path) -> Result<Image, String> {
        let decoded = image::ImageReader::open(path)
            .map_err(|err| err.to_string())?
            // trust the bytes, not the file extension
            .with_guessed_format()
            .map_err(|err| err.to_string())?
            .decode()
            .map_err(|err| err.to_string())?
            .to_rgba8();
        let (width, height) = decoded.dimensions();
        Image::from_rgba(width, height, decoded.as_raw())
    }

    /// builds an image from straight-alpha rgba bytes, row-major
    pub fn from_rgba(width: u32, height: u32, data: &[u8]) -> Result<Image, String> {
        let (Ok(width), Ok(height)) = (i32::try_from(width), i32::try_from(height)) else {
            return Err("image is too large".to_owned());
        };
        if width == 0 || height == 0 {
            return Err("image is empty".to_owned());
        }
        if data.len() != width as usize * height as usize * 4 {
            return Err("pixel data does not match the image size".to_owned());
        }
        // fnv-1a over the dimensions and pixels, fixed once here: the
        // rencache hashes this instead of the rc pointer, so an image
        // reallocated at a freed image's address can never be mistaken
        // for it -- and the dimensions matter, or a 2x8 and a 4x4 with
        // identical bytes would collide
        let mut h: u32 = 2166136261;
        let mut write = |bytes: &[u8]| {
            for &b in bytes {
                h = (h ^ b as u32).wrapping_mul(16777619);
            }
        };
        write(&width.to_le_bytes());
        write(&height.to_le_bytes());
        write(data);
        let pixels = data
            .chunks_exact(4)
            .map(|px| Color {
                r: px[0],
                g: px[1],
                b: px[2],
                a: px[3],
            })
            .collect();
        Ok(Image {
            width,
            height,
            pixels,
            content_hash: h,
        })
    }

    pub fn width(&self) -> i32 {
        self.width
    }

    pub fn height(&self) -> i32 {
        self.height
    }

    pub fn content_hash(&self) -> u32 {
        self.content_hash
    }

    #[inline]
    pub fn pixel(&self, x: i32, y: i32) -> Color {
        // usize math: callers derive x and y from the image's own
        // dimensions, and usize cannot wrap where i32 theoretically could
        self.pixels[y as usize * self.width as usize + x as usize]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("wisp-image-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn a_png_roundtrips_through_load() {
        let dir = scratch("roundtrip");
        let path = dir.join("fixture.png");
        let encoded = image::RgbaImage::from_fn(3, 2, |x, y| {
            image::Rgba([x as u8 * 10, y as u8 * 10, 200, 255])
        });
        encoded.save(&path).unwrap();

        let img = Image::load(&path).unwrap();
        assert_eq!((img.width(), img.height()), (3, 2));
        for y in 0..2 {
            for x in 0..3 {
                let c = img.pixel(x, y);
                assert_eq!((c.r, c.g, c.b, c.a), (x as u8 * 10, y as u8 * 10, 200, 255));
            }
        }
    }

    #[test]
    fn a_misnamed_file_still_decodes_by_content() {
        let dir = scratch("misnamed");
        let path = dir.join("actually-a-png.jpg");
        image::RgbaImage::from_pixel(1, 1, image::Rgba([1, 2, 3, 4]))
            .save_with_format(&path, image::ImageFormat::Png)
            .unwrap();
        let img = Image::load(&path).unwrap();
        assert_eq!(
            img.pixel(0, 0),
            Color {
                r: 1,
                g: 2,
                b: 3,
                a: 4
            }
        );
    }

    #[test]
    fn junk_and_missing_files_fail_without_panicking() {
        let dir = scratch("junk");
        let path = dir.join("junk.png");
        std::fs::write(&path, b"not an image at all").unwrap();
        assert!(Image::load(&path).is_err());
        assert!(Image::load(&dir.join("missing.png")).is_err());
    }

    #[test]
    fn mismatched_pixel_data_is_rejected() {
        assert!(Image::from_rgba(2, 2, &[0; 15]).is_err());
        assert!(Image::from_rgba(2, 2, &[0; 17]).is_err());
        assert!(Image::from_rgba(0, 2, &[]).is_err());
    }

    #[test]
    fn the_content_hash_sees_pixels_and_shape() {
        let a = Image::from_rgba(2, 2, &[7; 16]).unwrap();
        let b = Image::from_rgba(2, 2, &[9; 16]).unwrap();
        // same bytes, different shape: these must not collide either
        let wide = Image::from_rgba(4, 1, &[7; 16]).unwrap();
        assert_ne!(a.content_hash(), b.content_hash());
        assert_ne!(a.content_hash(), wide.content_hash());
        let again = Image::from_rgba(2, 2, &[7; 16]).unwrap();
        assert_eq!(a.content_hash(), again.content_hash());
    }
}
