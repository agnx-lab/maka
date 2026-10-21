/*
 * Licensed to the Apache Software Foundation (ASF) under one
 * or more contributor license agreements.  See the NOTICE file
 * distributed with this work for additional information
 * regarding copyright ownership.  The ASF licenses this file
 * to you under the Apache License, Version 2.0 (the
 * "License"); you may not use this file except in compliance
 * with the License.  You may obtain a copy of the License at
 *
 *     http://www.apache.org/licenses/LICENSE-2.0
 *
 * Unless required by applicable law or agreed to in writing,
 * software distributed under the License is distributed on an
 * "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
 * KIND, either express or implied.  See the License for the
 * specific language governing permissions and limitations
 * under the License.
 */

//! One explicit native clipboard read, in the original resource worker.
use super::super::file::paths;
use image::{ImageDecoder, ImageEncoder};
use maka_ui_runtime::{
    budget::{Kind, NativeBudget, Permit},
    file::{FileError, FileSnapshot, FileSnapshotWriter},
};
use std::{collections::VecDeque, io::Cursor, path::PathBuf, sync::Arc};

pub(super) struct Paths {
    pub items: VecDeque<PathBuf>,
    pub _memory: Permit,
}
pub(super) struct Text {
    pub value: String,
    pub _memory: Permit,
}
pub(super) enum Content {
    Files(Paths),
    Image(Arc<FileSnapshot>),
    Text(Text),
}
const PIXEL_BYTES: usize = 128 * 1024 * 1024;

pub(super) fn admit_paths(
    paths: impl IntoIterator<Item = PathBuf>,
    allow_files: bool,
    budget: &NativeBudget,
) -> Result<Paths, FileError> {
    if !allow_files {
        return Err(FileError::LimitExceeded);
    }
    let mut items = VecDeque::new();
    for path in paths {
        if !paths::valid(&path) {
            return Err(FileError::InvalidFile);
        }
        if !items.contains(&path) {
            items.push_back(path);
        }
    }
    if items.is_empty() {
        return Err(FileError::InvalidFile);
    }
    let capacity = items
        .capacity()
        .checked_mul(std::mem::size_of::<PathBuf>())
        .and_then(|bytes| {
            items
                .iter()
                .try_fold(bytes, |bytes, path| bytes.checked_add(path.capacity()))
        })
        .ok_or(FileError::BudgetExhausted)?;
    let memory = budget.reserve(Kind::Encoded, capacity)?;
    Ok(Paths {
        items,
        _memory: memory,
    })
}

// The native getter allocates before returning; retain its reservation until
// parsing and the separately charged path collection have both finished.
// Parameters drop in reverse order: files must drop before their reservation.
fn admit_clipboard_files(
    mut memory: Permit,
    files: Vec<String>,
    budget: &NativeBudget,
) -> Result<Paths, FileError> {
    let capacity = files
        .capacity()
        .checked_mul(std::mem::size_of::<String>())
        .and_then(|bytes| {
            files
                .iter()
                .try_fold(bytes, |bytes, file| bytes.checked_add(file.capacity()))
        })
        .ok_or(FileError::BudgetExhausted)?;
    if capacity > memory.capacity() {
        return Err(FileError::BudgetExhausted);
    }
    memory.shrink_to(capacity);
    let parsed = files
        .iter()
        .map(|file| paths::local_path(file))
        .collect::<Option<Vec<_>>>()
        .ok_or(FileError::InvalidFile)?;
    admit_paths(parsed, true, budget)
}

/// Text-only controls never inspect files or decode clipboard images.
#[cfg(any(target_os = "macos", target_os = "linux", windows))]
pub(super) fn read_text(budget: &NativeBudget) -> Result<Content, FileError> {
    use clipboard_rs::{Clipboard, ClipboardContext};
    let clipboard = ClipboardContext::new().map_err(|_| FileError::ClipboardUnavailable)?;
    let value = clipboard
        .get_text()
        .map_err(|_| FileError::ClipboardUnavailable)?;
    let memory = budget.reserve(Kind::Encoded, value.capacity())?;
    Ok(Content::Text(Text {
        value,
        _memory: memory,
    }))
}
#[cfg(not(any(target_os = "macos", target_os = "linux", windows)))]
pub(super) fn read_text(_: &NativeBudget) -> Result<Content, FileError> {
    Err(FileError::ClipboardUnavailable)
}

#[cfg(any(target_os = "macos", target_os = "linux", windows))]
pub(super) fn read(allow_files: bool, budget: &NativeBudget) -> Result<Content, FileError> {
    use clipboard_rs::{Clipboard, ClipboardContext, ContentFormat};
    let clipboard = ClipboardContext::new().map_err(|_| FileError::ClipboardUnavailable)?;
    if clipboard.has(ContentFormat::Files) {
        // Native clipboard getters allocate their platform response internally.
        // Reserve before calling; reject oversized responses before retaining or decoding.
        if !allow_files {
            return Err(FileError::LimitExceeded);
        }
        let memory = budget.reserve_remaining(Kind::Encoded)?;
        let files = clipboard
            .get_files()
            .map_err(|_| FileError::ClipboardUnavailable)?;
        return admit_clipboard_files(memory, files, budget).map(Content::Files);
    }
    let has_image = clipboard.has(ContentFormat::Image);
    #[cfg(windows)]
    let has_image = has_image || clipboard_win::is_format_avail(clipboard_win::formats::CF_DIBV5);
    if has_image {
        if !allow_files {
            return Err(FileError::LimitExceeded);
        }
        let mut memory = budget.reserve_remaining(Kind::Encoded)?;
        let bytes = image_bytes(&clipboard)?;
        if bytes.capacity() > memory.capacity() {
            return Err(FileError::BudgetExhausted);
        }
        memory.shrink_to(bytes.capacity());
        let image = encode_image(&bytes, budget)?;
        return Ok(Content::Image(image));
    }
    if clipboard.has(ContentFormat::Text) {
        let value = clipboard
            .get_text()
            .map_err(|_| FileError::ClipboardUnavailable)?;
        let memory = budget.reserve(Kind::Encoded, value.capacity())?;
        return Ok(Content::Text(Text {
            value,
            _memory: memory,
        }));
    }
    Err(FileError::ClipboardUnavailable)
}
#[cfg(not(any(target_os = "macos", target_os = "linux", windows)))]
pub(super) fn read(_: bool, _: &NativeBudget) -> Result<Content, FileError> {
    Err(FileError::ClipboardUnavailable)
}

#[cfg(any(target_os = "macos", target_os = "linux", windows))]
fn image_bytes(clipboard: &clipboard_rs::ClipboardContext) -> Result<Vec<u8>, FileError> {
    use clipboard_rs::Clipboard;
    #[cfg(target_os = "macos")]
    let bytes = clipboard
        .get_buffer("public.png")
        .or_else(|_| clipboard.get_buffer("public.tiff"));
    #[cfg(target_os = "linux")]
    let bytes = clipboard.get_buffer("image/png");
    #[cfg(windows)]
    let bytes = clipboard.get_buffer("PNG");
    if let Ok(bytes) = bytes {
        return Ok(bytes);
    }
    #[cfg(windows)]
    {
        use clipboard_win::{formats, get_clipboard};
        return get_clipboard(formats::RawData(formats::CF_DIBV5))
            .or_else(|_| get_clipboard(formats::RawData(formats::CF_DIB)))
            .map_err(|_| FileError::ClipboardUnavailable)
            .and_then(dib_bitmap);
    }
    #[cfg(not(windows))]
    Err(FileError::ClipboardUnavailable)
}

// A packed clipboard DIB omits BITMAPFILEHEADER. Supply an explicit pixel
// offset: image 0.25.10's headerless V4/V5 path skips twelve extra mask bytes.
// V4/V5 masks are already fields of those headers; palette/masks after the
// 40-byte BITMAPINFOHEADER are counted separately.
// https://learn.microsoft.com/en-us/windows/win32/api/wingdi/ns-wingdi-bitmapinfoheader
#[cfg(any(windows, test))]
fn dib_bitmap(mut bytes: Vec<u8>) -> Result<Vec<u8>, FileError> {
    let invalid = FileError::InvalidFile;
    let word = |at: usize| {
        bytes
            .get(at..at + 4)
            .map(|value| u32::from_le_bytes(value.try_into().unwrap()))
    };
    let header = word(0).ok_or(invalid)? as usize;
    if !matches!(header, 12 | 40 | 52 | 56 | 108 | 124) || bytes.len() < header {
        return Err(invalid);
    }
    let (bit_at, colors, mask_bytes, color_bytes) = if header == 12 {
        (10, 0, 0, 3)
    } else {
        let masks = if header == 40 {
            match word(16).ok_or(invalid)? {
                3 => 12,
                6 => 16,
                _ => 0,
            }
        } else {
            0
        };
        (14, word(32).ok_or(invalid)? as usize, masks, 4)
    };
    let bits = u16::from_le_bytes(bytes[bit_at..bit_at + 2].try_into().unwrap());
    if !matches!(bits, 1 | 4 | 8 | 16 | 24 | 32) {
        return Err(invalid);
    }
    let colors = if colors == 0 && bits <= 8 {
        1usize << bits
    } else {
        colors
    };
    let offset = colors
        .checked_mul(color_bytes)
        .and_then(|n| n.checked_add(header + mask_bytes))
        .ok_or(invalid)?;
    if offset >= bytes.len() {
        return Err(invalid);
    }
    let length = bytes.len();
    let bitmap_length = length
        .checked_add(14)
        .and_then(|length| u32::try_from(length).ok())
        .ok_or(FileError::InvalidFile)?;
    bytes.reserve_exact(14);
    bytes.resize(length + 14, 0);
    bytes.copy_within(0..length, 14);
    bytes[..2].copy_from_slice(b"BM");
    bytes[2..6].copy_from_slice(&bitmap_length.to_le_bytes());
    bytes[6..10].fill(0);
    bytes[10..14].copy_from_slice(&((offset + 14) as u32).to_le_bytes());
    Ok(bytes)
}

fn encode_image(bytes: &[u8], budget: &NativeBudget) -> Result<Arc<FileSnapshot>, FileError> {
    let _pixels = budget.reserve(Kind::Pixels, PIXEL_BYTES)?;
    let mut limits = image::Limits::default();
    limits.max_alloc = Some(PIXEL_BYTES as u64);
    let mut reader = image::ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|_| FileError::InvalidFile)?;
    reader.limits(limits);
    let decoder = reader.into_decoder().map_err(|_| FileError::InvalidFile)?;
    encode_decoder(decoder, budget)
}

fn encode_decoder(
    decoder: impl ImageDecoder,
    budget: &NativeBudget,
) -> Result<Arc<FileSnapshot>, FileError> {
    let (width, height) = decoder.dimensions();
    if decoder.total_bytes() > PIXEL_BYTES as u64 {
        return Err(FileError::LimitExceeded);
    }
    let image = image::DynamicImage::from_decoder(decoder).map_err(|_| FileError::InvalidFile)?;
    let mut writer = FileSnapshotWriter::new("clipboard.png", budget.clone())?;
    let encoded = image::codecs::png::PngEncoder::new(&mut writer).write_image(
        image.as_bytes(),
        width,
        height,
        image.color().into(),
    );
    let snapshot = writer.finish()?;
    encoded.map_err(|_| FileError::InvalidFile)?;
    Ok(snapshot)
}

#[cfg(test)]
mod tests {
    use super::*;
    use maka_capabilities::call::ReadResource;
    use maka_ui_runtime::budget::BudgetLimits;
    #[test]
    fn clipboard_images_encode_directly_into_original_budgeted_file_chunks() {
        for format in [
            image::ImageFormat::Png,
            image::ImageFormat::Tiff,
            image::ImageFormat::Bmp,
        ] {
            let mut encoded = Cursor::new(Vec::new());
            let original = image::DynamicImage::ImageRgba8(image::ImageBuffer::from_pixel(
                8,
                4,
                image::Rgba([17, 51, 99, 128]),
            ));
            original.write_to(&mut encoded, format).unwrap();
            let budget = NativeBudget::default();
            let bytes = encoded.into_inner();
            let snapshot = encode_image(&bytes, &budget).unwrap();
            assert_eq!(snapshot.metadata()["name"], "clipboard.png");
            let png = snapshot.read(0..snapshot.bytes()).unwrap();
            let decoded = image::load_from_memory(&png).unwrap();
            assert_eq!(decoded.to_rgba8(), original.to_rgba8());
            assert_eq!(budget.usage(), (65536, 0, 0));
            drop(snapshot);
            assert_eq!(budget.usage(), (0, 0, 0));
            let small = NativeBudget::new(BudgetLimits {
                encoded_bytes: 100,
                ..Default::default()
            });
            assert!(matches!(
                encode_image(&bytes, &small),
                Err(FileError::BudgetExhausted)
            ));
            assert_eq!(small.usage(), (0, 0, 0));
            if format == image::ImageFormat::Bmp {
                let restored = dib_bitmap(bytes[14..].to_vec()).unwrap();
                assert_eq!(restored, bytes);
                let snapshot = encode_image(&restored, &budget).unwrap();
                assert_eq!(snapshot.metadata()["name"], "clipboard.png");
            }
        }
    }
    #[test]
    fn native_paths_keep_spaces_and_deduplicate_without_reinterpreting_file_names() {
        let budget = NativeBudget::default();
        let directory = tempfile::tempdir().unwrap();
        let expected: Vec<_> = (0..9)
            .map(|index| {
                let path = directory.path().join(format!("中文 file {index}.txt"));
                std::fs::write(&path, format!("original file {index}")).unwrap();
                paths::local_path(path.to_str().unwrap()).unwrap()
            })
            .collect();
        let mut input = expected.clone();
        input.insert(1, expected[0].clone());
        input.push(expected[8].clone());
        let paths = admit_paths(input, true, &budget).unwrap();
        assert_eq!(
            paths.items.iter().collect::<Vec<_>>(),
            expected.iter().collect::<Vec<_>>()
        );
        assert!(budget.usage().0 > 0);
        drop(paths);
        assert_eq!(budget.usage(), (0, 0, 0));
    }
    #[test]
    fn clipboard_file_capacity_is_charged_before_parsing_and_released_on_failure() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("original.txt");
        std::fs::write(&path, b"original").unwrap();
        let original = path.to_str().unwrap().to_owned();
        let limits = BudgetLimits {
            encoded_bytes: 4096,
            ..Default::default()
        };
        for vector_capacity in [false, true] {
            let mut files = if vector_capacity {
                Vec::with_capacity(4096 / std::mem::size_of::<String>() + 1)
            } else {
                Vec::new()
            };
            let mut value = if vector_capacity {
                String::new()
            } else {
                String::with_capacity(4096)
            };
            value.push_str(&original);
            files.push(value);
            let budget = NativeBudget::new(limits);
            let memory = budget.reserve_remaining(Kind::Encoded).unwrap();
            assert!(matches!(
                admit_clipboard_files(memory, files, &budget),
                Err(FileError::BudgetExhausted)
            ));
            assert_eq!(budget.usage(), (0, 0, 0));
        }
        let budget = NativeBudget::new(limits);
        let retained = budget.reserve(Kind::Encoded, 128).unwrap();
        let memory = budget.reserve_remaining(Kind::Encoded).unwrap();
        assert_eq!(memory.capacity(), 3968);
        assert_eq!(budget.usage(), (4096, 0, 0));
        assert!(matches!(
            budget.reserve_remaining(Kind::Encoded),
            Err(maka_ui_runtime::budget::BudgetExhausted)
        ));
        let admitted = admit_clipboard_files(memory, vec![original], &budget).unwrap();
        assert_eq!(admitted.items, VecDeque::from([path]));
        assert_eq!(budget.usage(), (128 + admitted._memory.capacity(), 0, 0));
        drop(admitted);
        assert_eq!(budget.usage(), (128, 0, 0));
        drop(retained);
        assert_eq!(budget.usage(), (0, 0, 0));
    }
    #[test]
    fn image_pixel_limit_rejects_before_decoding_or_output_allocation() {
        struct Oversized;
        impl ImageDecoder for Oversized {
            fn dimensions(&self) -> (u32, u32) {
                (8192, 4097)
            }
            fn color_type(&self) -> image::ColorType {
                image::ColorType::Rgba8
            }
            fn read_image(self, _: &mut [u8]) -> image::ImageResult<()> {
                panic!("oversized pixels were decoded")
            }
            fn read_image_boxed(self: Box<Self>, bytes: &mut [u8]) -> image::ImageResult<()> {
                (*self).read_image(bytes)
            }
        }
        let budget = NativeBudget::default();
        assert!(matches!(
            encode_decoder(Oversized, &budget),
            Err(FileError::LimitExceeded)
        ));
        assert_eq!(budget.usage(), (0, 0, 0));
    }
}
