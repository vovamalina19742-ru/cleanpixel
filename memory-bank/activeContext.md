# CleanPixel: Active Context

## Текущее состояние
- **Фаза:** P0 и P1 уязвимости и баги полностью устранены, протестированы и закоммичены.
- **Текущий коммит:** `3b89dea` (`fix(core): enhance path safety, progressive JPEG, PNG CRC validation, and structured EXIF inspection`).
- **Ветка:** `main` (синхронизирована с `origin/main` на GitHub).

## Ключевые компоненты
- `src/lib.rs`: `clean_image_file` с гарантией защиты оригиналов, `generate_cleaned_path`, `atomic_write_file` с защитой от гонок Rayon и блокировок Windows.
- `src/main.rs`: CLI с параметрами `-o`, `-d` (`--output-dir`), `-i` (`--in-place`), `-r` (`--recursive`), подкоманда `inspect`, ненулевой код завершения при ошибках.
- `src/formats/jpeg.rs`: `strip_jpeg` с поддержкой Progressive JPEG и сохранением ICC (`APP2`) / Adobe (`APP14`).
- `src/formats/png.rs`: `strip_png` с `crc32`, валидацией `IHDR`/`IEND` и сохранением `iCCP`.
- `src/formats/mod.rs`: `detect_format_from_file` с чтением только 16 байт с диска.
- `src/inspector.rs`: `inspect_bytes`, `parse_tiff_structure`, `inspect_xmp_xml`.
- `tests/integration_tests.rs`: Интеграционные тесты всех сценариев (Progressive JPEG, ICC, PNG CRC, защита оригиналов).

## Следующие шаги
- Интеграция CleanPixel как легковесного Sidecar-модуля для платформы Sentinel-OSINT при очистке фотоотчетов.
