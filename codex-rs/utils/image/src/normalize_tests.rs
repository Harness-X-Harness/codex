use super::*;

const IMAGE_BUDGET: usize = 1024 * 1024;

#[test]
fn generated_image_normalization_preserves_decoded_pixels_dimensions_and_metadata() {
    for format in [ImageFormat::Png, ImageFormat::Jpeg, ImageFormat::WebP] {
        let pixels = ImageBuffer::from_fn(/*width*/ 3, /*height*/ 2, |x, y| {
            Rgba([(x * 70) as u8, (y * 100) as u8, 50, 180])
        });
        let original = image_bytes_with_metadata(&pixels, format, TEST_RGB_ICC_PROFILE);
        let expected = image::load_from_memory(&original)
            .expect("decode source")
            .to_rgba8();
        let result = normalize_to_png(Path::new("generated-image"), original.clone(), IMAGE_BUDGET)
            .expect("normalize supported still image");
        assert_eq!(
            (result.mime.as_str(), result.width, result.height),
            ("image/png", 3, 2)
        );
        assert_eq!(
            image::load_from_memory(&result.bytes)
                .expect("decode PNG")
                .to_rgba8(),
            expected
        );
        let mut decoder = ImageReader::with_format(Cursor::new(&result.bytes), ImageFormat::Png)
            .into_decoder()
            .expect("PNG decoder");
        assert_eq!(
            (
                decoder.orientation().expect("orientation"),
                decoder.icc_profile().expect("ICC"),
                decoder.exif_metadata().expect("EXIF")
            ),
            (
                Orientation::Rotate90,
                Some(TEST_RGB_ICC_PROFILE.to_vec()),
                Some(ROTATE_90_EXIF.to_vec())
            )
        );
        if format == ImageFormat::Png {
            assert_eq!(result.bytes.as_ref(), original);
        }
    }
}

#[test]
fn generated_image_normalization_preserves_alpha_and_omits_incompatible_icc() {
    let pixels =
        ImageBuffer::from_pixel(/*width*/ 2, /*height*/ 3, Rgba([15, 30, 60, 128]));
    for format in [ImageFormat::Png, ImageFormat::WebP] {
        let source = image_bytes(&pixels, format);
        let result =
            normalize_to_png(Path::new("image"), source, IMAGE_BUDGET).expect("normalize alpha");
        assert_eq!(
            image::load_from_memory(&result.bytes)
                .expect("decode alpha")
                .to_rgba8(),
            pixels
        );
    }
    let source = image_bytes_with_metadata(&pixels, ImageFormat::Jpeg, TEST_CMYK_ICC_PROFILE);
    let result =
        normalize_to_png(Path::new("image"), source, IMAGE_BUDGET).expect("normalize JPEG");
    let mut decoder = ImageReader::with_format(Cursor::new(&result.bytes), ImageFormat::Png)
        .into_decoder()
        .expect("PNG decoder");
    assert_eq!(decoder.icc_profile().expect("ICC"), None);
    assert_eq!(
        decoder.exif_metadata().expect("EXIF"),
        Some(ROTATE_90_EXIF.to_vec())
    );
}

#[test]
fn generated_image_normalization_rejects_encoded_decoded_and_output_overflow() {
    let pixels = ImageBuffer::from_pixel(
        /*width*/ 64,
        /*height*/ 64,
        Rgba([50, 60, 70, 255]),
    );
    let png = image_bytes(&pixels, ImageFormat::Png);
    let input_limit = png.len() - 1;
    assert!(matches!(
        normalize_to_png(Path::new("image"), png.clone(), input_limit),
        Err(ImageProcessingError::ImageTooLarge {
            representation: "encoded input",
            ..
        })
    ));
    let pixel_limit = png.len().max(1024);
    assert!(normalize_to_png(Path::new("image"), png, pixel_limit).is_err());
    let small =
        ImageBuffer::from_pixel(/*width*/ 1, /*height*/ 1, Rgba([50, 60, 70, 255]));
    let webp = image_bytes(&small, ImageFormat::WebP);
    let normalized =
        normalize_to_png(Path::new("image"), webp.clone(), IMAGE_BUDGET).expect("small PNG");
    let output_limit = normalized.bytes.len() - 1;
    assert!(output_limit > webp.len());
    assert!(matches!(
        normalize_to_png(Path::new("image"), webp, output_limit),
        Err(ImageProcessingError::ImageTooLarge {
            representation: "encoded output",
            ..
        })
    ));
}

#[test]
fn generated_image_normalization_rejects_truncated_unsupported_and_animated_data() {
    for format in [ImageFormat::Png, ImageFormat::Jpeg, ImageFormat::WebP] {
        let pixels =
            ImageBuffer::from_pixel(/*width*/ 3, /*height*/ 2, Rgba([50, 60, 70, 255]));
        let mut bytes = image_bytes(&pixels, format);
        bytes.truncate(bytes.len() / 2);
        assert!(normalize_to_png(Path::new("image"), bytes, IMAGE_BUDGET).is_err());
    }
    let gif = image_bytes(
        &ImageBuffer::from_pixel(/*width*/ 1, /*height*/ 1, Rgba([0, 0, 0, 255])),
        ImageFormat::Gif,
    );
    assert!(matches!(
        normalize_to_png(Path::new("image"), gif, IMAGE_BUDGET),
        Err(ImageProcessingError::UnsupportedImageFormat { .. })
    ));
    let animated = BASE64_STANDARD.decode("UklGRsQAAABXRUJQVlA4WAoAAAACAAAAAQAAAAAAQU5JTQYAAAAAAAAAAABBTk1GSgAAAAAAAAAAAAEAAAAAAGQAAAJWUDggMgAAADABAJ0BKgIAAQABQCYloAADcAD+8ut///mwP/bz/wR6Af//0uD//pcH//S4P/SkAAAAQU5NRkYAAAAAAAAAAAABAAAAAABkAAAAVlA4IC4AAAA0AQCdASoCAAEAAAAmJaAAA3AA/vtV4///S4P/+lwf/9Lg/9Lg//rV5Vesq6AA").expect("animated WebP fixture");
    assert!(matches!(
        normalize_to_png(Path::new("image"), animated, IMAGE_BUDGET),
        Err(ImageProcessingError::UnsupportedImageFormat { .. })
    ));
}

#[test]
fn generated_png_requires_complete_tail_after_decoded_pixels() {
    let pixels =
        ImageBuffer::from_pixel(/*width*/ 2, /*height*/ 1, Rgba([10, 20, 30, 128]));
    let complete = image_bytes(&pixels, ImageFormat::Png);
    // IEND follows a complete IDAT. These corruptions leave pixel data intact.
    for removed in [1, 5, 12] {
        let truncated = complete[..complete.len() - removed].to_vec();
        assert!(normalize_to_png(Path::new("image"), truncated, IMAGE_BUDGET).is_err());
    }
    let mut corrupt = complete;
    *corrupt.last_mut().expect("IEND checksum") ^= 1;
    assert!(normalize_to_png(Path::new("image"), corrupt, IMAGE_BUDGET).is_err());
}

#[test]
fn generated_png_validates_post_image_chunks_and_zlib_checksum() {
    let mut with_text = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut with_text, /*width*/ 1, /*height*/ 1);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().expect("PNG header");
        writer
            .write_image_data(&[255, 0, 0, 255])
            .expect("PNG pixels");
        writer
            .write_chunk(png::chunk::tEXt, b"Description\0after pixels")
            .expect("trailing text");
        writer.finish().expect("PNG end");
    }
    assert!(normalize_to_png(Path::new("image"), with_text.clone(), IMAGE_BUDGET).is_ok());
    let mut corrupt_text = with_text.clone();
    let text_checksum_end = corrupt_text.len() - 12;
    corrupt_text[text_checksum_end - 1] ^= 1;
    assert!(normalize_to_png(Path::new("image"), corrupt_text, IMAGE_BUDGET).is_err());
    with_text.truncate(with_text.len() - 15);
    assert!(normalize_to_png(Path::new("image"), with_text, IMAGE_BUDGET).is_err());

    let mut bad_adler = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut bad_adler, /*width*/ 1, /*height*/ 1);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().expect("PNG header");
        // Valid one-red-pixel deflate data with a deliberately incorrect Adler32.
        // The codec writes the correct PNG chunk CRC around that invalid stream.
        writer
            .write_chunk(
                png::chunk::IDAT,
                &[120, 156, 99, 248, 207, 192, 240, 31, 0, 5, 0, 1, 254],
            )
            .expect("IDAT fixture");
        writer.finish().expect("PNG end");
    }
    assert!(normalize_to_png(Path::new("image"), bad_adler, IMAGE_BUDGET).is_err());
}
