use std::fs;
use std::path::PathBuf;

use cleanpixel::clean_image_file;
use cleanpixel::formats::detect_format;
use cleanpixel::formats::jpeg::strip_jpeg;
use cleanpixel::formats::png::{crc32, strip_png};
use cleanpixel::formats::ImageFormat;
use cleanpixel::inspector::inspect_bytes;

fn make_valid_tiff_exif_with_gps() -> Vec<u8> {
    let mut exif = Vec::new();
    // Exif\0\0 prefix (6 bytes)
    exif.extend_from_slice(b"Exif\0\0");

    // TIFF Header: Little Endian ('II'), 42 (0x002A), offset to IFD0 = 8
    exif.extend_from_slice(b"II");
    exif.extend_from_slice(&42u16.to_le_bytes());
    exif.extend_from_slice(&8u32.to_le_bytes());

    // IFD0: 1 entry
    exif.extend_from_slice(&1u16.to_le_bytes());

    // Entry 1: Tag 0x8825 (GPSInfo IFD pointer), type LONG (4), count 1, value offset 26
    exif.extend_from_slice(&0x8825u16.to_le_bytes());
    exif.extend_from_slice(&4u16.to_le_bytes());
    exif.extend_from_slice(&1u32.to_le_bytes());
    exif.extend_from_slice(&26u32.to_le_bytes());

    // Next IFD offset: 0
    exif.extend_from_slice(&0u32.to_le_bytes());

    // GPS IFD at offset 26: 1 entry
    exif.extend_from_slice(&1u16.to_le_bytes());
    // GPS Tag 0x0002 (Latitude)
    exif.extend_from_slice(&2u16.to_le_bytes());
    exif.extend_from_slice(&5u16.to_le_bytes()); // RATIONAL
    exif.extend_from_slice(&3u32.to_le_bytes());
    exif.extend_from_slice(&40u32.to_le_bytes());
    exif.extend_from_slice(&0u32.to_le_bytes()); // Next IFD = 0

    exif
}

fn append_png_chunk(out: &mut Vec<u8>, chunk_type: &[u8; 4], data: &[u8]) {
    let len = data.len() as u32;
    out.extend_from_slice(&len.to_be_bytes());
    out.extend_from_slice(chunk_type);
    out.extend_from_slice(data);

    let mut crc_input = Vec::with_capacity(4 + data.len());
    crc_input.extend_from_slice(chunk_type);
    crc_input.extend_from_slice(data);
    let checksum = crc32(&crc_input);
    out.extend_from_slice(&checksum.to_be_bytes());
}

#[test]
fn test_jpeg_lossless_metadata_stripping() {
    let mut fake_jpeg = Vec::new();

    // 1. SOI
    fake_jpeg.extend_from_slice(&[0xFF, 0xD8]);

    // 2. APP1 Exif with structured TIFF and GPS tag (0x8825)
    let tiff_bytes = make_valid_tiff_exif_with_gps();
    let app1_len = (tiff_bytes.len() + 2) as u16;
    fake_jpeg.extend_from_slice(&[0xFF, 0xE1]);
    fake_jpeg.extend_from_slice(&app1_len.to_be_bytes());
    fake_jpeg.extend_from_slice(&tiff_bytes);

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

    // Verify inspector detects GPS in structured TIFF
    let inspect_before = inspect_bytes(&fake_jpeg).expect("inspect failed");
    assert!(inspect_before.has_gps, "Expected GPS detected before cleaning");

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
fn test_progressive_jpeg_multi_scan_preservation() {
    let mut prog_jpeg = Vec::new();

    // 1. SOI
    prog_jpeg.extend_from_slice(&[0xFF, 0xD8]);

    // 2. SOF2 (Progressive DCT, len 11)
    prog_jpeg.extend_from_slice(&[0xFF, 0xC2, 0x00, 0x0B, 0x08, 0x00, 0x40, 0x00, 0x40, 0x03, 0x01]);

    // 3. APP1 XMP (Metadata to be stripped)
    prog_jpeg.extend_from_slice(&[0xFF, 0xE1, 0x00, 0x24]);
    prog_jpeg.extend_from_slice(b"http://ns.adobe.com/xap/1.0/\0<xmpMM:DocumentID>uuid:12345</xmpMM:DocumentID>");

    // 4. DQT
    prog_jpeg.extend_from_slice(&[0xFF, 0xDB, 0x00, 0x06, 0x00, 0x10, 0x10, 0x10]);

    // 5. DHT Scan 1 (Huffman table 1)
    prog_jpeg.extend_from_slice(&[0xFF, 0xC4, 0x00, 0x06, 0x00, 0x01, 0x02, 0x03]);

    // 6. Scan 1: SOS 1 with byte stuffing (0xFF 0x00) and restart marker (0xFF 0xD0)
    prog_jpeg.extend_from_slice(&[0xFF, 0xDA, 0x00, 0x04, 0x01, 0x00]);
    prog_jpeg.extend_from_slice(&[0xAA, 0xFF, 0x00, 0xBB, 0xFF, 0xD0, 0xCC]);

    // 7. DHT Scan 2 (Huffman table 2 between scans!)
    prog_jpeg.extend_from_slice(&[0xFF, 0xC4, 0x00, 0x06, 0x01, 0x04, 0x05, 0x06]);

    // 8. Scan 2: SOS 2
    prog_jpeg.extend_from_slice(&[0xFF, 0xDA, 0x00, 0x04, 0x02, 0x00]);
    prog_jpeg.extend_from_slice(&[0xDD, 0xEE, 0xFF, 0x00, 0x11]);

    // 9. EOI
    prog_jpeg.extend_from_slice(&[0xFF, 0xD9]);

    let (cleaned, rep) = strip_jpeg(&prog_jpeg).expect("strip progressive jpeg failed");

    // XMP was stripped
    assert_eq!(rep.stripped_segments.len(), 1);

    // Verify BOTH scans and intermediate DHT are preserved!
    let sos_count = cleaned.windows(2).filter(|w| *w == [0xFF, 0xDA]).count();
    assert_eq!(sos_count, 2, "Both progressive scans must be preserved");

    let dht_count = cleaned.windows(2).filter(|w| *w == [0xFF, 0xC4]).count();
    assert_eq!(dht_count, 2, "Both Huffman tables must be preserved");

    // Restart marker (0xFF 0xD0) preserved
    assert!(cleaned.windows(2).any(|w| w == [0xFF, 0xD0]));

    // Byte stuffing preserved
    assert!(cleaned.windows(2).any(|w| w == [0xFF, 0x00]));

    // Ends with EOI
    assert_eq!(&cleaned[cleaned.len() - 2..], &[0xFF, 0xD9]);
}

#[test]
fn test_jpeg_icc_and_adobe_preservation() {
    let mut jpeg = Vec::new();

    // 1. SOI
    jpeg.extend_from_slice(&[0xFF, 0xD8]);

    // 2. APP2 ICC Profile (MUST BE PRESERVED)
    let icc_content = b"ICC_PROFILE\0\x01\x01\0SampleColorSpaceProfileData";
    let icc_len = (icc_content.len() + 2) as u16;
    jpeg.extend_from_slice(&[0xFF, 0xE2]);
    jpeg.extend_from_slice(&icc_len.to_be_bytes());
    jpeg.extend_from_slice(icc_content);

    // 3. APP14 Adobe Color Transform (MUST BE PRESERVED for CMYK/YCCK)
    let adobe_content = b"Adobe\0\x64\x00\x00\x00\x00\x02";
    let adobe_len = (adobe_content.len() + 2) as u16;
    jpeg.extend_from_slice(&[0xFF, 0xEE]);
    jpeg.extend_from_slice(&adobe_len.to_be_bytes());
    jpeg.extend_from_slice(adobe_content);

    // 4. APP2 Proprietary Vendor (MUST BE STRIPPED)
    let app2_trash = b"FlashPixVendorNonIccData";
    let app2_trash_len = (app2_trash.len() + 2) as u16;
    jpeg.extend_from_slice(&[0xFF, 0xE2]);
    jpeg.extend_from_slice(&app2_trash_len.to_be_bytes());
    jpeg.extend_from_slice(app2_trash);

    // 5. APP1 Exif (MUST BE STRIPPED)
    jpeg.extend_from_slice(&[0xFF, 0xE1, 0x00, 0x0A, 0x45, 0x78, 0x69, 0x66, 0x00, 0x00]);

    // 6. SOS + Pixel data + EOI
    jpeg.extend_from_slice(&[0xFF, 0xDA, 0x00, 0x04, 0x01, 0x00, 0x99, 0x88, 0xFF, 0xD9]);

    let (cleaned, rep) = strip_jpeg(&jpeg).expect("strip failed");

    // APP2-trash and APP1 were stripped (2 items)
    assert_eq!(rep.stripped_segments.len(), 2);

    // ICC Profile (APP2) preserved!
    assert!(cleaned.windows(11).any(|w| w == b"ICC_PROFILE"));

    // Adobe APP14 preserved!
    assert!(cleaned.windows(5).any(|w| w == b"Adobe"));
}

#[test]
fn test_png_lossless_metadata_stripping_and_crc() {
    let mut fake_png = Vec::new();

    // 1. 8-byte PNG Signature
    fake_png.extend_from_slice(&[137, 80, 78, 71, 13, 10, 26, 10]);

    // 2. IHDR (Critical chunk - KEEP)
    append_png_chunk(&mut fake_png, b"IHDR", &[0, 0, 0, 10, 0, 0, 0, 10, 8, 2, 0, 0, 0]);

    // 3. iCCP (Color profile - KEEP)
    append_png_chunk(&mut fake_png, b"iCCP", b"sRGB\0\0compressedProfileBytes");

    // 4. tEXt (Author / Software metadata - STRIP)
    append_png_chunk(&mut fake_png, b"tEXt", b"Software\0Google Photos Camera");

    // 5. eXIf (EXIF - STRIP)
    append_png_chunk(&mut fake_png, b"eXIf", &make_valid_tiff_exif_with_gps());

    // 6. IDAT (Pixel data - KEEP)
    append_png_chunk(&mut fake_png, b"IDAT", &[0x78, 0x9C, 0x63, 0x00, 0x00, 0x00, 0x02, 0x00, 0x01]);

    // 7. IEND (Trailer - KEEP)
    append_png_chunk(&mut fake_png, b"IEND", &[]);

    assert_eq!(detect_format(&fake_png), ImageFormat::Png);

    // Inspect before cleaning
    let inspect_before = inspect_bytes(&fake_png).expect("inspect failed");
    assert!(inspect_before.has_gps);

    // Strip PNG
    let (cleaned, rep) = strip_png(&fake_png).expect("strip_png failed");
    assert_eq!(rep.stripped_chunks.len(), 2); // tEXt and eXIf stripped

    // Inspect after cleaning
    let inspect_after = inspect_bytes(&cleaned).expect("inspect failed");
    assert!(!inspect_after.has_gps);
    assert_eq!(inspect_after.detected_blocks.len(), 0);

    // Verify IHDR, iCCP, IDAT, IEND preserved
    assert!(cleaned.windows(4).any(|w| w == b"IHDR"));
    assert!(cleaned.windows(4).any(|w| w == b"iCCP"));
    assert!(cleaned.windows(4).any(|w| w == b"IDAT"));
    assert!(cleaned.windows(4).any(|w| w == b"IEND"));
}

#[test]
fn test_png_corrupted_crc_rejected() {
    let mut corrupted_png = Vec::new();
    corrupted_png.extend_from_slice(&[137, 80, 78, 71, 13, 10, 26, 10]);

    // Append IHDR with deliberately corrupted CRC
    let ihdr_data = [0, 0, 0, 10, 0, 0, 0, 10, 8, 2, 0, 0, 0];
    corrupted_png.extend_from_slice(&(ihdr_data.len() as u32).to_be_bytes());
    corrupted_png.extend_from_slice(b"IHDR");
    corrupted_png.extend_from_slice(&ihdr_data);
    corrupted_png.extend_from_slice(&[0xDE, 0xAD, 0xBE, 0xEF]); // Bad CRC

    append_png_chunk(&mut corrupted_png, b"IEND", &[]);

    let res = strip_png(&corrupted_png);
    assert!(res.is_err(), "Must reject PNG with corrupted CRC");
    let err_msg = res.unwrap_err().to_string();
    assert!(err_msg.contains("CRC32"), "Error message should mention CRC32: {}", err_msg);
}

#[test]
fn test_png_structure_validation_ihdr_and_iend() {
    // Missing IHDR (starts with tEXt)
    let mut no_ihdr = Vec::new();
    no_ihdr.extend_from_slice(&[137, 80, 78, 71, 13, 10, 26, 10]);
    append_png_chunk(&mut no_ihdr, b"tEXt", b"Title\0Test");
    append_png_chunk(&mut no_ihdr, b"IEND", &[]);

    let res1 = strip_png(&no_ihdr);
    assert!(res1.is_err());
    assert!(res1.unwrap_err().to_string().contains("IHDR"));

    // Missing IEND
    let mut no_iend = Vec::new();
    no_iend.extend_from_slice(&[137, 80, 78, 71, 13, 10, 26, 10]);
    append_png_chunk(&mut no_iend, b"IHDR", &[0, 0, 0, 10, 0, 0, 0, 10, 8, 2, 0, 0, 0]);

    let res2 = strip_png(&no_iend);
    assert!(res2.is_err());
    assert!(res2.unwrap_err().to_string().contains("IEND"));
}

#[test]
fn test_clean_image_file_does_not_overwrite_original_when_none() {
    let temp_dir = std::env::temp_dir().join(format!("cleanpixel_test_{}", std::process::id()));
    let _ = fs::create_dir_all(&temp_dir);

    let test_file = temp_dir.join("sample_original.jpg");
    let mut sample_jpeg = Vec::new();
    sample_jpeg.extend_from_slice(&[0xFF, 0xD8]);
    sample_jpeg.extend_from_slice(&[0xFF, 0xFE, 0x00, 0x0A, 0x54, 0x65, 0x73, 0x74, 0x21, 0x21]); // COM
    sample_jpeg.extend_from_slice(&[0xFF, 0xDA, 0x00, 0x04, 0x01, 0x00, 0x12, 0x34, 0xFF, 0xD9]);

    fs::write(&test_file, &sample_jpeg).expect("write original failed");
    let orig_len = fs::metadata(&test_file).unwrap().len();

    // Call clean_image_file with output_path = None
    let rep = clean_image_file(&test_file, None).expect("clean_image_file failed");
    assert!(rep.success);

    // Verify ORIGINAL is untouched!
    let after_len = fs::metadata(&test_file).unwrap().len();
    assert_eq!(orig_len, after_len, "Original file must NOT be modified when output is None!");
    let after_bytes = fs::read(&test_file).unwrap();
    assert_eq!(after_bytes, sample_jpeg, "Original bytes must match 100%");

    // Verify cleaned copy exists next to it
    let cleaned_file = temp_dir.join("sample_original.cleaned.jpg");
    assert!(cleaned_file.exists(), "Cleaned copy must be created");
    let cleaned_bytes = fs::read(&cleaned_file).unwrap();
    assert!(!cleaned_bytes.windows(2).any(|w| w == [0xFF, 0xFE]), "Cleaned copy must have COM stripped");

    // Clean up
    let _ = fs::remove_file(&test_file);
    let _ = fs::remove_file(&cleaned_file);
    let _ = fs::remove_dir(&temp_dir);
}
