use cleanpixel::formats::detect_format;
use cleanpixel::formats::jpeg::strip_jpeg;
use cleanpixel::formats::png::strip_png;
use cleanpixel::formats::ImageFormat;
use cleanpixel::inspector::inspect_bytes;

#[test]
fn test_jpeg_lossless_metadata_stripping() {
    let mut fake_jpeg = Vec::new();

    // 1. SOI
    fake_jpeg.extend_from_slice(&[0xFF, 0xD8]);

    // 2. APP1 Exif with GPS (Length: 22 bytes = 2 len + 4 Exif + 16 data)
    fake_jpeg.extend_from_slice(&[0xFF, 0xE1, 0x00, 0x16]);
    fake_jpeg.extend_from_slice(b"Exif\0\0GPS_LAT_55_LON_37");

    // 3. COM (Length: 15 bytes = 2 len + 13 text)
    fake_jpeg.extend_from_slice(&[0xFF, 0xFE, 0x00, 0x0F]);
    fake_jpeg.extend_from_slice(b"SecretComment");

    // 4. DQT (Quantization Table - MUST BE PRESERVED)
    fake_jpeg.extend_from_slice(&[0xFF, 0xDB, 0x00, 0x08, 0x00, 0x01, 0x02, 0x03, 0x04, 0x05]);

    // 5. SOS (Start of Scan with compressed pixel data)
    fake_jpeg.extend_from_slice(&[0xFF, 0xDA, 0x00, 0x04, 0x01, 0x00]);
    fake_jpeg.extend_from_slice(&[0x12, 0x34, 0x56, 0x78]); // Compressed entropy pixels

    // 6. EOI
    fake_jpeg.extend_from_slice(&[0xFF, 0xD9]);

    assert_eq!(detect_format(&fake_jpeg), ImageFormat::Jpeg);

    // Verify inspector sees GPS and COM
    let inspect_before = inspect_bytes(&fake_jpeg).expect("inspect failed");
    assert!(inspect_before.has_gps, "Expected GPS detected before cleaning");
    assert_eq!(inspect_before.detected_blocks.len(), 2);

    // Strip JPEG
    let (cleaned, rep) = strip_jpeg(&fake_jpeg).expect("strip_jpeg failed");

    // Assert stripped segments
    assert_eq!(rep.stripped_segments.len(), 2);
    assert!(rep.cleaned_size < rep.original_size);

    // Verify inspector sees ZERO metadata in cleaned file
    let inspect_after = inspect_bytes(&cleaned).expect("inspect failed");
    assert!(!inspect_after.has_gps, "GPS should be completely stripped");
    assert_eq!(inspect_after.detected_blocks.len(), 0);

    // Verify DQT and pixel data preserved
    assert!(cleaned.windows(2).any(|w| w == [0xFF, 0xDB]));
    assert!(cleaned.windows(2).any(|w| w == [0xFF, 0xDA]));
    assert_eq!(&cleaned[cleaned.len() - 2..], &[0xFF, 0xD9]);
}

#[test]
fn test_png_lossless_metadata_stripping() {
    let mut fake_png = Vec::new();

    // 1. 8-byte PNG Signature
    fake_png.extend_from_slice(&[137, 80, 78, 71, 13, 10, 26, 10]);

    // Helper to append a PNG chunk (length + type + data + crc)
    let mut append_chunk = |chunk_type: &[u8; 4], data: &[u8]| {
        let len = data.len() as u32;
        fake_png.extend_from_slice(&len.to_be_bytes());
        fake_png.extend_from_slice(chunk_type);
        fake_png.extend_from_slice(data);
        fake_png.extend_from_slice(&[0x00, 0x00, 0x00, 0x00]); // Dummy CRC32
    };

    // 2. IHDR (Critical chunk - KEEP)
    append_chunk(b"IHDR", &[0, 0, 0, 10, 0, 0, 0, 10, 8, 2, 0, 0, 0]);

    // 3. tEXt (Author / Software metadata - STRIP)
    append_chunk(b"tEXt", b"Software\0Google Photos Camera");

    // 4. eXIf (EXIF GPS - STRIP)
    append_chunk(b"eXIf", b"GPS_LOCATION_LAT_LON");

    // 5. IDAT (Pixel data - KEEP)
    append_chunk(b"IDAT", &[0x78, 0x9C, 0x63, 0x00, 0x00, 0x00, 0x02, 0x00, 0x01]);

    // 6. IEND (Trailer - KEEP)
    append_chunk(b"IEND", &[]);

    assert_eq!(detect_format(&fake_png), ImageFormat::Png);

    // Inspect before cleaning
    let inspect_before = inspect_bytes(&fake_png).expect("inspect failed");
    assert!(inspect_before.has_gps);
    assert_eq!(inspect_before.detected_blocks.len(), 2);

    // Strip PNG
    let (cleaned, rep) = strip_png(&fake_png).expect("strip_png failed");
    assert_eq!(rep.stripped_chunks.len(), 2);

    // Inspect after cleaning
    let inspect_after = inspect_bytes(&cleaned).expect("inspect failed");
    assert!(!inspect_after.has_gps);
    assert_eq!(inspect_after.detected_blocks.len(), 0);

    // Verify IHDR, IDAT, IEND preserved
    assert!(cleaned.windows(4).any(|w| w == b"IHDR"));
    assert!(cleaned.windows(4).any(|w| w == b"IDAT"));
    assert!(cleaned.windows(4).any(|w| w == b"IEND"));
}
