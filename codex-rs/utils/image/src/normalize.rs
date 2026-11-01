use std::io::Cursor;
use std::path::Path;

use image::DynamicImage;
use image::ImageDecoder;
use image::ImageFormat;
use image::ImageReader;
use image::Limits;
use image::codecs::webp::WebPDecoder;

use crate::EncodedImage;
use crate::ImageMetadata;
use crate::ImageProcessingError;
use crate::encode_image;

/// Decodes a PNG default image or still JPEG/WebP into the PNG representation
/// required by generated-image history, retaining supported rendering metadata.
/// PNG passthrough validates the container and checksums, but does not decode
/// additional animation frames. Its original bytes are preserved.
/// The local byte budget bounds source bytes, decoded RGBA pixels, and output, and
/// is passed to the codec allocation limiter. Animated WebP is rejected.
pub fn normalize_to_png(
    path: &Path,
    bytes: Vec<u8>,
    max_bytes: usize,
) -> Result<EncodedImage, ImageProcessingError> {
    check_size("encoded input", bytes.len(), max_bytes)?;
    let format = image::guess_format(&bytes)
        .map_err(|source| ImageProcessingError::decode_error(path, source))?;
    if !matches!(
        format,
        ImageFormat::Png | ImageFormat::Jpeg | ImageFormat::WebP
    ) {
        return Err(ImageProcessingError::UnsupportedImageFormat {
            mime: format.to_mime_type().to_string(),
        });
    }
    if format == ImageFormat::Png {
        // The image adapter stops after pixels. Use the existing PNG codec to
        // verify checksums and the complete container before retaining bytes.
        let mut options = png::DecodeOptions::default();
        options.set_ignore_checksums(false);
        options.set_skip_ancillary_crc_failures(false);
        let mut decoder = png::Decoder::new_with_options(Cursor::new(&bytes), options);
        decoder.set_limits(png::Limits { bytes: max_bytes });
        decoder.set_transformations(png::Transformations::EXPAND);
        let png_error = |source| ImageProcessingError::Decode {
            path: path.to_path_buf(),
            source: image::ImageError::Decoding(image::error::DecodingError::new(
                ImageFormat::Png.into(),
                source,
            )),
        };
        let mut reader = decoder.read_info().map_err(png_error)?;
        let (width, height) = (reader.info().width, reader.info().height);
        let decoded_bytes = u64::from(width)
            .saturating_mul(u64::from(height))
            .saturating_mul(4);
        let buffer_size = reader.output_buffer_size().unwrap_or(usize::MAX);
        check_size(
            "decoded pixels",
            usize::try_from(decoded_bytes)
                .unwrap_or(usize::MAX)
                .max(buffer_size),
            max_bytes,
        )?;
        let mut pixels = vec![0; buffer_size];
        reader.next_frame(&mut pixels).map_err(png_error)?;
        reader.finish().map_err(png_error)?;
        drop(reader);
        return Ok(EncodedImage {
            bytes: bytes.into(),
            mime: "image/png".to_string(),
            source_width: width,
            source_height: height,
            width,
            height,
        });
    }
    if format == ImageFormat::WebP
        && WebPDecoder::new(Cursor::new(&bytes))
            .map_err(|source| ImageProcessingError::decode_error(path, source))?
            .has_animation()
    {
        return Err(ImageProcessingError::UnsupportedImageFormat {
            mime: "animated image/webp".to_string(),
        });
    }
    let mut reader = ImageReader::with_format(Cursor::new(&bytes), format);
    let mut limits = Limits::default();
    limits.max_alloc = Some(max_bytes as u64);
    reader.limits(limits);
    let mut decoder = reader
        .into_decoder()
        .map_err(|source| ImageProcessingError::decode_error(path, source))?;
    let (width, height) = decoder.dimensions();
    let decoded_bytes = u64::from(width)
        .saturating_mul(u64::from(height))
        .saturating_mul(u64::from(decoder.color_type().bytes_per_pixel()).max(4));
    check_size(
        "decoded pixels",
        usize::try_from(decoded_bytes).unwrap_or(usize::MAX),
        max_bytes,
    )?;
    let metadata = ImageMetadata::from_decoder(&mut decoder);
    let decoded = DynamicImage::from_decoder(decoder)
        .map_err(|source| ImageProcessingError::decode_error(path, source))?;
    let bytes = encode_image(&decoded, ImageFormat::Png, metadata)?.0;
    check_size("encoded output", bytes.len(), max_bytes)?;
    Ok(EncodedImage {
        bytes: bytes.into(),
        mime: "image/png".to_string(),
        source_width: width,
        source_height: height,
        width: decoded.width(),
        height: decoded.height(),
    })
}

fn check_size(
    representation: &'static str,
    size: usize,
    max: usize,
) -> Result<(), ImageProcessingError> {
    if size > max {
        return Err(ImageProcessingError::ImageTooLarge {
            representation,
            size,
            max,
        });
    }
    Ok(())
}
