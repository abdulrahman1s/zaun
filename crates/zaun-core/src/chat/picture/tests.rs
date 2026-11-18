use super::*;

/// A noisy photo-like picture (noise is the hardest case for JPEG).
fn photo(w: u32, h: u32) -> image::RgbImage {
    let mut seed = 0x2545_f491_u32;
    image::RgbImage::from_fn(w, h, |x, y| {
        seed ^= seed << 13;
        seed ^= seed >> 17;
        seed ^= seed << 5;
        let n = (seed & 0x3f) as u8;
        let base = |v: u32, d: u32| u8::try_from(v * 180 / d.max(1)).unwrap_or(0);
        image::Rgb([
            base(x, w).saturating_add(n),
            base(y, h).saturating_add(n),
            100u8.saturating_add(n),
        ])
    })
}

fn jpeg_file(img: &image::RgbImage) -> Vec<u8> {
    let mut out = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, 92)
        .encode_image(img)
        .expect("encode");
    out
}

fn png_file(img: &DynamicImage) -> Vec<u8> {
    let mut out = Vec::new();
    img.write_to(&mut Cursor::new(&mut out), image::ImageFormat::Png)
        .expect("encode");
    out
}

fn decoded(p: &Picture) -> DynamicImage {
    image::load_from_memory(p.bytes()).expect("a picture")
}

fn code(r: Result<Picture>) -> crate::error::ErrorCode {
    r.expect_err("refused").code
}

#[test]
fn a_large_photo_becomes_a_square_jpeg_under_the_engines_limit() {
    let p = make(&jpeg_file(&photo(3000, 2000)), None).expect("made");
    assert!(p.bytes().starts_with(JPEG_MAGIC));
    assert!(data_uri(&p).len() <= MAX_URI, "{}", data_uri(&p).len());
    assert!(data_uri(&p).starts_with("data:image/jpg;base64,"));
    let img = decoded(&p);
    assert_eq!(img.width(), img.height());
    assert!(img.width() >= 96 && img.width() <= 512, "{}", img.width());
}

#[test]
fn a_tall_picture_keeps_its_centre_square() {
    // Red top and bottom, green middle: the centre square is green.
    let img = image::RgbImage::from_fn(300, 1500, |_, y| {
        if (600..900).contains(&y) {
            image::Rgb([0, 200, 0])
        } else {
            image::Rgb([200, 0, 0])
        }
    });
    let p = make(&jpeg_file(&img), None).expect("made");
    let out = decoded(&p).to_rgb8();
    assert_eq!(out.width(), 300);
    assert_eq!(out.height(), 300);
    let [r, g, _] = out.get_pixel(150, 150).0;
    assert!(g > 150 && r < 60, "{r} {g}");
    assert!(data_uri(&p).len() <= MAX_URI);
}

#[test]
fn the_crop_chooses_the_square_and_is_clamped_to_the_picture() {
    // Left half blue, right half yellow.
    let img = image::RgbImage::from_fn(400, 200, |x, _| {
        if x < 200 {
            image::Rgb([0, 0, 220])
        } else {
            image::Rgb([230, 230, 0])
        }
    });
    let file = jpeg_file(&img);
    let right = make(
        &file,
        Some(PictureCrop {
            x: 220,
            y: 10,
            side: 150,
        }),
    )
    .expect("made");
    let [r, _, b] = decoded(&right).to_rgb8().get_pixel(75, 75).0;
    assert!(r > 180 && b < 80, "{r} {b}");
    // Too large and too far right: the largest square, against the right edge.
    let clamped = make(
        &file,
        Some(PictureCrop {
            x: 9999,
            y: 9999,
            side: 9999,
        }),
    )
    .expect("made");
    let out = decoded(&clamped).to_rgb8();
    assert_eq!((out.width(), out.height()), (200, 200));
    assert!(out.get_pixel(100, 100).0[0] > 180);
}

#[test]
fn a_picture_with_transparency_stays_a_png_under_the_limit() {
    let img = image::RgbaImage::from_fn(800, 800, |x, y| {
        let inside = (x as i32 - 400).pow(2) + (y as i32 - 400).pow(2) < 300 * 300;
        if inside {
            image::Rgba([u8::try_from(x % 256).unwrap_or(0), 90, 200, 255])
        } else {
            image::Rgba([0, 0, 0, 0])
        }
    });
    let p = make(&png_file(&DynamicImage::ImageRgba8(img)), None).expect("made");
    assert!(p.bytes().starts_with(PNG_MAGIC));
    assert!(data_uri(&p).starts_with("data:image/png;base64,"));
    assert!(data_uri(&p).len() <= MAX_URI, "{}", data_uri(&p).len());
    let out = decoded(&p).to_rgba8();
    assert_eq!(out.get_pixel(0, 0).0[3], 0, "the corner stays transparent");
}

#[test]
fn transparent_noise_that_no_png_fits_goes_on_white_as_a_jpeg() {
    // 128 px of noise: no PNG of it fits, and it is not shrunk below 128 px as a PNG.
    let noise = photo(128, 128);
    let img = image::RgbaImage::from_fn(128, 128, |x, y| {
        let [r, g, b] = noise.get_pixel(x, y).0;
        image::Rgba([r, g, b, if x < 64 { 0 } else { 255 }])
    });
    let p = make(&png_file(&DynamicImage::ImageRgba8(img)), None).expect("made");
    assert!(p.bytes().starts_with(JPEG_MAGIC), "a JPEG");
    assert!(data_uri(&p).len() <= MAX_URI);
    let [r, g, b] = decoded(&p).to_rgb8().get_pixel(5, 50).0;
    assert!(r > 230 && g > 230 && b > 230, "on white: {r} {g} {b}");
}

#[test]
fn a_file_that_is_not_a_picture_is_refused() {
    let e = code(make(b"%PDF-1.7 not a picture at all", None));
    assert_eq!(e, crate::error::ErrorCode::ChtNotAPicture);
    let mut broken = jpeg_file(&photo(64, 64));
    broken.truncate(40);
    assert_eq!(
        code(make(&broken, None)),
        crate::error::ErrorCode::ChtNotAPicture
    );
}

#[test]
fn a_picture_larger_than_the_source_limit_is_refused_before_decoding() {
    // A tiny PNG file that claims 20 000 × 8 pixels.
    let img = DynamicImage::ImageLuma8(image::GrayImage::new(MAX_SOURCE_SIDE + 1, 8));
    let e = code(make(&png_file(&img), None));
    assert_eq!(e, crate::error::ErrorCode::ChtPictureTooLarge);
}

#[test]
fn exif_orientation_is_applied_before_cropping() {
    // A 200 × 100 JPEG whose EXIF says "rotate 90° clockwise" is shown 100 × 200.
    let jpeg = jpeg_file(&photo(200, 100));
    let tiff: &[u8] = &[
        b'I', b'I', 42, 0, 8, 0, 0, 0, // little-endian TIFF, first IFD at 8
        1, 0, // one entry
        0x12, 0x01, 3, 0, 1, 0, 0, 0, 6, 0, 0, 0, // Orientation (SHORT) = 6
        0, 0, 0, 0, // no next IFD
    ];
    let mut app1 = b"Exif\0\0".to_vec();
    app1.extend_from_slice(tiff);
    let len = u16::try_from(app1.len() + 2).expect("short");
    let mut file = jpeg[..2].to_vec();
    file.extend_from_slice(&[0xff, 0xe1]);
    file.extend_from_slice(&len.to_be_bytes());
    file.extend_from_slice(&app1);
    file.extend_from_slice(&jpeg[2..]);
    let img = decode_upright(&file).expect("decoded");
    assert_eq!((img.width(), img.height()), (100, 200));
}

#[test]
fn a_friends_picture_is_read_from_its_data_uri() {
    let made = make(&jpeg_file(&photo(300, 300)), None).expect("made");
    let back = from_data_uri(&data_uri(&made)).expect("read");
    assert_eq!(back, made);
    assert_eq!(back.key(), made.key());
    // The `jpeg` spelling and a PNG are read too.
    let jpeg = data_uri(&made).replace("image/jpg", "image/jpeg");
    assert!(from_data_uri(&jpeg).is_some());
    let png = png_file(&DynamicImage::ImageRgb8(photo(20, 20)));
    let png_uri = format!(
        "{PNG_PREFIX}{}",
        base64::engine::general_purpose::STANDARD.encode(&png)
    );
    assert_eq!(
        from_data_uri(&png_uri).map(|p| p.bytes().len()),
        Some(png.len())
    );
}

#[test]
fn a_bad_picture_from_a_friend_is_dropped() {
    let b64 = |b: &[u8]| base64::engine::general_purpose::STANDARD.encode(b);
    // Longer than the cap: dropped before decoding.
    let long = format!("{JPEG_PREFIX}{}", "A".repeat(MAX_URI_IN));
    assert_eq!(from_data_uri(&long), None);
    // Not a picture, behind a picture's prefix.
    assert_eq!(
        from_data_uri(&format!("{JPEG_PREFIX}{}", b64(b"hello"))),
        None
    );
    // Not base64.
    assert_eq!(from_data_uri(&format!("{JPEG_PREFIX}!!!")), None);
    // Another type, or no data URI at all.
    let gif = format!("data:image/gif;base64,{}", b64(b"GIF89a"));
    assert_eq!(from_data_uri(&gif), None);
    assert_eq!(from_data_uri("https://example.invalid/a.jpg"), None);
    // Larger than MAX_SIDE (a small file claiming 2000 × 4 pixels).
    let wide = png_file(&DynamicImage::ImageLuma8(image::GrayImage::new(2000, 4)));
    assert_eq!(from_data_uri(&format!("{PNG_PREFIX}{}", b64(&wide))), None);
}

#[test]
fn a_pictures_key_follows_its_bytes_and_debug_hides_them() {
    let a = Picture::new(vec![1, 2, 3]);
    assert_eq!(a.key(), Picture::new(vec![1, 2, 3]).key());
    assert_ne!(a.key(), Picture::new(vec![1, 2, 4]).key());
    assert_eq!(
        format!("{a:?}"),
        format!("Picture(3 bytes, {:016x})", a.key())
    );
}
