# 🦀 CleanPixel

> **Ultra-fast, lossless forensic metadata and GUID stripper for JPEG and PNG written in Rust.**  
> *0% loss of image quality. 0 recompression. Removes GPS, Google Photos tags, XMP, Adobe GUIDs, and hidden EXIF thumbnails at 1000+ files per second.*

---

## ⚡ Why CleanPixel?

Most existing metadata removal tools either:
1. **Recompress the image (Lossy):** Common image editors re-encode JPEG/PNG data through DCT/zlib, reducing sharpness, altering colors, and introducing compression artifacts.
2. **Are heavy and slow:** `exiftool` is written in Perl, takes several hundred milliseconds per file, and requires complex Perl environments.

**CleanPixel** uses **Byte-Stream Splicing**:
* It reads the binary container markers directly (`APP1`, `APP13`, `tEXt`, `zTXt`, `eXIf`);
* Removes all tracking blocks, Google XMP IDs, and GPS coordinates;
* Copies the raw quantization tables, Huffman tables, and entropy-coded pixel stream **bit-for-bit without decoding**;
* Results in **0.000% image quality loss** with maximum execution speed.

---

## 🚀 Installation & Build

```bash
git clone https://github.com/vovamalina19742-ru/cleanpixel.git
cd cleanpixel
cargo build --release
```
The compiled standalone binary will be in `target/release/cleanpixel` (~1.4 MB).

---

## 🛠️ Usage

### 1. Inspect metadata without modifying
```bash
cleanpixel inspect photo.jpg
```
Output:
```text
=== 🔍 CleanPixel Metadata Inspector ===
File: photo.jpg
Format: JPEG
GPS Coordinates: ⚠️ DETECTED
Google/Adobe GUID: ⚠️ DETECTED (XMP)

Detected Metadata Blocks:
  • EXIF block (32 bytes)
  • XMP block (2450 bytes)
```

### 2. Clean a single file
```bash
# Save cleaned image to new file
cleanpixel input.jpg -o output.jpg

# Or clean in-place (overwrite original)
cleanpixel input.jpg --in-place
```

### 3. Batch clean an entire folder
```bash
# Multi-threaded parallel processing with Rayon
cleanpixel ./my_photos/ --in-place --recursive
```

### 4. Sidecar Mode (JSON output)
Ideal for integration into desktop apps, bots, or CI/CD pipelines:
```bash
cleanpixel photo.jpg --json
```
Output:
```json
{
  "file_path": "photo.jpg",
  "format": "JPEG",
  "original_size_bytes": 1048576,
  "cleaned_size_bytes": 983040,
  "saved_bytes": 65536,
  "saved_percent": 6.25,
  "stripped_items": [
    "APP1-Exif (GPS/Камера/Миниатюра): 16384 байт",
    "APP1-XMP (Google GUID/Adobe Content Credentials): 49152 байт"
  ],
  "success": true,
  "error": null
}
```

---

## 🛡️ Stripped Metadata Types

* **JPEG:**
  * `APP1` (Exif metadata, GPS coordinates, device serial numbers, hidden thumbnails);
  * `APP1` (XMP / Adobe Content Credentials / Google Photos Gaia IDs);
  * `APP13` (Photoshop Image Resource Blocks / IPTC);
  * `APP2..APP15` (FlashPix, vendor proprietary tags);
  * `COM` (Plaintext comments).
* **PNG:**
  * `tEXt`, `zTXt`, `iTXt` (Text chunks, international metadata, software tags);
  * `eXIf` (Raw EXIF and GPS chunk);
  * `tIME` (Timestamp chunks);
  * `dSIG` (Digital signatures).

---

## 📜 License
MIT License. Created by Vladimir (`vovamalina19742-ru`).
