use std::io::Cursor;

use image::imageops::{self, FilterType};
use image::{ImageFormat, RgbaImage};
use wayland_client::protocol::wl_output::Transform;

use crate::screen::{self, Image, Region};

const OPAQUE: u8 = 255;

const CLEAR: image::Rgba<u8> = image::Rgba([0, 0, 0, 0]);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Channels {
    Argb8888,
    Xrgb8888,
    Abgr8888,
    Xbgr8888,
}

pub(crate) struct Buffer<'a> {
    pub(crate) bytes: &'a [u8],
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) stride: u32,
    pub(crate) channels: Channels,
}

pub(crate) fn to_rgba(buffer: &Buffer<'_>) -> RgbaImage {
    let row = buffer.width as usize * 4;
    let mut rgba = Vec::with_capacity(row * buffer.height as usize);
    for line in buffer
        .bytes
        .chunks(buffer.stride as usize)
        .take(buffer.height as usize)
    {
        for [first, green, third, alpha] in line[..row].as_chunks::<4>().0.iter().copied() {
            let pixel = match buffer.channels {
                Channels::Argb8888 => [third, green, first, alpha],
                Channels::Xrgb8888 => [third, green, first, OPAQUE],
                Channels::Abgr8888 => [first, green, third, alpha],
                Channels::Xbgr8888 => [first, green, third, OPAQUE],
            };
            rgba.extend_from_slice(&pixel);
        }
    }
    RgbaImage::from_raw(buffer.width, buffer.height, rgba)
        .expect("each row holds width pixels of four bytes")
}

pub(crate) fn untransform(buffer: RgbaImage, transform: Transform) -> RgbaImage {
    let quarter_turned = matches!(
        transform,
        Transform::_90 | Transform::_270 | Transform::Flipped90 | Transform::Flipped270
    );
    let (width, height) = if quarter_turned {
        (buffer.height(), buffer.width())
    } else {
        buffer.dimensions()
    };
    let (right, bottom) = (width - 1, height - 1);
    RgbaImage::from_fn(width, height, |x, y| {
        let (from_x, from_y) = match transform {
            Transform::_90 => (y, right - x),
            Transform::_180 => (right - x, bottom - y),
            Transform::_270 => (bottom - y, x),
            Transform::Flipped => (right - x, y),
            Transform::Flipped90 => (y, x),
            Transform::Flipped180 => (x, bottom - y),
            Transform::Flipped270 => (bottom - y, right - x),
            _ => (x, y),
        };
        *buffer.get_pixel(from_x, from_y)
    })
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Placement {
    pub(crate) x: i32,
    pub(crate) y: i32,
    pub(crate) scale: f64,
}

pub(crate) fn crop(shown: &RgbaImage, monitor: Placement, region: Region) -> RgbaImage {
    let pixels = |logical: f64| (logical * monitor.scale).round() as i64;
    let left = pixels(f64::from(region.x - monitor.x));
    let top = pixels(f64::from(region.y - monitor.y));
    let width = pixels(f64::from(region.width)).max(1) as u32;
    let height = pixels(f64::from(region.height)).max(1) as u32;
    RgbaImage::from_fn(width, height, |x, y| {
        let from_x = u32::try_from(left + i64::from(x)).ok();
        let from_y = u32::try_from(top + i64::from(y)).ok();
        from_x
            .zip(from_y)
            .and_then(|(x, y)| shown.get_pixel_checked(x, y))
            .copied()
            .unwrap_or(CLEAR)
    })
}

pub(crate) struct Look {
    pub(crate) monitor: Placement,
    pub(crate) region: Region,
    pub(crate) scale: f64,
}

pub(crate) fn picture(
    buffer: &Buffer<'_>,
    transform: Transform,
    look: &Look,
) -> Result<Image, String> {
    let shown = untransform(to_rgba(buffer), transform);
    let cropped = crop(&shown, look.monitor, look.region);
    let (width, height) = screen::scaled(look.region, look.scale);
    let resized = imageops::resize(&cropped, width.max(1), height.max(1), FilterType::Triangle);
    let mut bytes = Vec::new();
    resized
        .write_to(&mut Cursor::new(&mut bytes), ImageFormat::Png)
        .map_err(|e| format!("can't encode the screen capture: {e}"))?;
    Ok(Image {
        bytes,
        mime_type: "image/png".into(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const PADDING: u8 = 0xEE;

    fn buffer(rows: &[&[[u8; 4]]]) -> (Vec<u8>, u32, u32, u32) {
        let width = rows[0].len() as u32;
        let stride = width * 4 + 8;
        let mut bytes = Vec::new();
        for row in rows {
            for pixel in *row {
                bytes.extend_from_slice(pixel);
            }
            bytes.extend_from_slice(&[PADDING; 8]);
        }
        (bytes, width, rows.len() as u32, stride)
    }

    fn converted(channels: Channels, rows: &[&[[u8; 4]]]) -> Vec<[u8; 4]> {
        let (bytes, width, height, stride) = buffer(rows);
        let image = to_rgba(&Buffer {
            bytes: &bytes,
            width,
            height,
            stride,
            channels,
        });
        image.pixels().map(|p| p.0).collect()
    }

    #[test]
    fn argb8888_is_stored_blue_green_red_alpha() {
        let rows: &[&[[u8; 4]]] = &[
            &[[1, 2, 3, 4], [5, 6, 7, 8]],
            &[[9, 10, 11, 12], [13, 14, 15, 16]],
        ];

        assert_eq!(
            converted(Channels::Argb8888, rows),
            [
                [3, 2, 1, 4],
                [7, 6, 5, 8],
                [11, 10, 9, 12],
                [15, 14, 13, 16]
            ]
        );
    }

    #[test]
    fn xrgb8888_is_stored_blue_green_red_and_opaque() {
        let rows: &[&[[u8; 4]]] = &[&[[1, 2, 3, 4]], &[[5, 6, 7, 8]]];

        assert_eq!(
            converted(Channels::Xrgb8888, rows),
            [[3, 2, 1, 255], [7, 6, 5, 255]]
        );
    }

    #[test]
    fn abgr8888_is_stored_red_green_blue_alpha() {
        let rows: &[&[[u8; 4]]] = &[&[[1, 2, 3, 4], [5, 6, 7, 8]]];

        assert_eq!(
            converted(Channels::Abgr8888, rows),
            [[1, 2, 3, 4], [5, 6, 7, 8]]
        );
    }

    #[test]
    fn xbgr8888_is_stored_red_green_blue_and_opaque() {
        let rows: &[&[[u8; 4]]] = &[&[[1, 2, 3, 4]], &[[5, 6, 7, 8]]];

        assert_eq!(
            converted(Channels::Xbgr8888, rows),
            [[1, 2, 3, 255], [5, 6, 7, 255]]
        );
    }

    fn image(rows: &[&str]) -> RgbaImage {
        let width = rows[0].len() as u32;
        let bytes = rows
            .iter()
            .flat_map(|row| row.bytes().map(|label| [label, 0, 0, 255]))
            .flatten()
            .collect();
        RgbaImage::from_raw(width, rows.len() as u32, bytes).unwrap()
    }

    #[test]
    fn each_transform_is_undone_back_to_the_screen_as_shown() {
        let shown = image(&["abc", "def"]);
        let buffers = [
            (Transform::Normal, image(&["abc", "def"])),
            (Transform::_90, image(&["cf", "be", "ad"])),
            (Transform::_180, image(&["fed", "cba"])),
            (Transform::_270, image(&["da", "eb", "fc"])),
            (Transform::Flipped, image(&["cba", "fed"])),
            (Transform::Flipped90, image(&["ad", "be", "cf"])),
            (Transform::Flipped180, image(&["def", "abc"])),
            (Transform::Flipped270, image(&["fc", "eb", "da"])),
        ];

        for (transform, buffer) in buffers {
            assert_eq!(untransform(buffer, transform), shown, "{transform:?}");
        }
    }

    const CLIPPED: [u8; 4] = [0, 0, 0, 0];

    fn positions(width: u32, height: u32) -> RgbaImage {
        RgbaImage::from_fn(width, height, |x, y| {
            image::Rgba([x as u8, y as u8, 0, 255])
        })
    }

    fn rows_of(image: &RgbaImage) -> Vec<Vec<[u8; 4]>> {
        image
            .rows()
            .map(|row| row.map(|pixel| pixel.0).collect())
            .collect()
    }

    fn at(x: u8, y: u8) -> [u8; 4] {
        [x, y, 0, 255]
    }

    #[test]
    fn a_region_maps_to_buffer_pixels_at_an_integer_scale() {
        let monitor = Placement {
            x: 100,
            y: 0,
            scale: 2.0,
        };
        let region = Region {
            x: 101,
            y: 1,
            width: 2,
            height: 1,
        };

        let cropped = crop(&positions(8, 6), monitor, region);

        assert_eq!(
            rows_of(&cropped),
            [
                [at(2, 2), at(3, 2), at(4, 2), at(5, 2)],
                [at(2, 3), at(3, 3), at(4, 3), at(5, 3)],
            ]
        );
    }

    #[test]
    fn a_region_maps_to_buffer_pixels_at_a_fractional_scale() {
        let monitor = Placement {
            x: 0,
            y: -10,
            scale: 1.5,
        };
        let region = Region {
            x: 2,
            y: -8,
            width: 2,
            height: 2,
        };

        let cropped = crop(&positions(6, 6), monitor, region);

        assert_eq!(
            rows_of(&cropped),
            [
                [at(3, 3), at(4, 3), at(5, 3)],
                [at(3, 4), at(4, 4), at(5, 4)],
                [at(3, 5), at(4, 5), at(5, 5)],
            ]
        );
    }

    #[test]
    fn the_part_of_a_region_off_the_monitor_is_left_clear() {
        let monitor = Placement {
            x: 0,
            y: 0,
            scale: 1.0,
        };
        let region = Region {
            x: -1,
            y: 1,
            width: 5,
            height: 2,
        };

        let cropped = crop(&positions(3, 2), monitor, region);

        assert_eq!(
            rows_of(&cropped),
            [
                [CLIPPED, at(0, 1), at(1, 1), at(2, 1), CLIPPED],
                [CLIPPED; 5],
            ]
        );
    }

    #[test]
    fn a_picture_is_a_png_of_the_region_at_the_look_s_scale() {
        let blue = [255, 0, 0, 0];
        let bytes: Vec<u8> = std::iter::repeat_n(blue, 40 * 20).flatten().collect();
        let buffer = Buffer {
            bytes: &bytes,
            width: 40,
            height: 20,
            stride: 160,
            channels: Channels::Xrgb8888,
        };
        let look = Look {
            monitor: Placement {
                x: 0,
                y: 0,
                scale: 2.0,
            },
            region: Region {
                x: 3,
                y: 1,
                width: 15,
                height: 7,
            },
            scale: 0.5,
        };

        let picture = picture(&buffer, Transform::Normal, &look).unwrap();

        assert_eq!(picture.mime_type, "image/png");
        let decoded = image::load_from_memory_with_format(&picture.bytes, image::ImageFormat::Png)
            .unwrap()
            .into_rgba8();
        assert_eq!(decoded.dimensions(), (8, 4));
        assert!(decoded.pixels().all(|pixel| pixel.0 == [0, 0, 255, 255]));
    }
}
