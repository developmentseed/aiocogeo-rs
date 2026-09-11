use std::collections::HashMap;
use std::io::Read;

use bytes::Bytes;

use crate::error::{AsyncTiffError, AsyncTiffResult, TiffError, TiffFormatError};
use crate::metadata::fetch::MetadataCursor;
use crate::metadata::MetadataFetch;
use crate::reader::Endianness;
use crate::tag_value::TagValue;
use crate::tags::{Tag, Type};
use crate::{ImageFileDirectory, TIFF};

/// Entry point to reading TIFF metadata.
///
/// This is a stateful reader because we don't know how many IFDs will be encountered.
///
/// ```notest
/// // fetch implements MetadataFetch
/// let mut metadata_reader = TiffMetadataReader::try_open(&fetch).await?;
/// let ifds = metadata_reader.read_all_ifds(&fetch).await?;
/// ```
pub struct TiffMetadataReader {
    endianness: Endianness,
    bigtiff: bool,
    next_ifd_offset: Option<u64>,
}

impl TiffMetadataReader {
    /// Open a new TIFF file, validating the magic bytes, reading the endianness, and checking for
    /// the bigtiff flag.
    ///
    /// This does not read any IFD metadata.
    pub async fn try_open<F: MetadataFetch>(fetch: &F) -> AsyncTiffResult<Self> {
        let magic_bytes = fetch.fetch(0..2).await?;

        // Should be b"II" for little endian or b"MM" for big endian
        let endianness = if magic_bytes == Bytes::from_static(b"II") {
            Endianness::LittleEndian
        } else if magic_bytes == Bytes::from_static(b"MM") {
            Endianness::BigEndian
        } else {
            return Err(AsyncTiffError::General(format!(
                "unexpected magic bytes {magic_bytes:?}"
            )));
        };

        // Set offset to 2 since we've already read magic bytes.
        let mut cursor = MetadataCursor::new(fetch, endianness).with_offset(2);

        let version = cursor.read_u16().await?;
        let bigtiff = match version {
            42 => false,
            43 => {
                // Read bytesize of offsets (in bigtiff it's alway 8 but provide a way to move to 16 some day)
                if cursor.read_u16().await? != 8 {
                    return Err(
                        TiffError::FormatError(TiffFormatError::TiffSignatureNotFound).into(),
                    );
                }
                // This constant should always be 0
                if cursor.read_u16().await? != 0 {
                    return Err(
                        TiffError::FormatError(TiffFormatError::TiffSignatureNotFound).into(),
                    );
                }
                true
            }
            _ => return Err(TiffError::FormatError(TiffFormatError::TiffSignatureInvalid).into()),
        };

        let first_ifd_location = if bigtiff {
            cursor.read_u64().await?
        } else {
            cursor.read_u32().await?.into()
        };

        Ok(Self {
            endianness,
            bigtiff,
            next_ifd_offset: Some(first_ifd_location),
        })
    }

    /// Returns the endianness of the file.
    pub fn endianness(&self) -> Endianness {
        self.endianness
    }

    /// Returns `true` if this is a bigtiff file.
    pub fn bigtiff(&self) -> bool {
        self.bigtiff
    }

    /// Returns `true` if there are more IFDs to read.
    pub fn has_next_ifd(&self) -> bool {
        self.next_ifd_offset.is_some()
    }

    /// The byte offset of the start of the next IFD.
    ///
    /// This will be `None` if all IFDs have already been read.
    pub fn next_ifd_offset(&self) -> Option<u64> {
        self.next_ifd_offset
    }

    /// Read the next IFD from the file.
    ///
    /// If there are no more IFDs, returns `None`.
    pub async fn read_next_ifd<F: MetadataFetch>(
        &mut self,
        fetch: &F,
    ) -> AsyncTiffResult<Option<ImageFileDirectory>> {
        if let Some(ifd_start) = self.next_ifd_offset {
            let ifd_reader =
                ImageFileDirectoryReader::open(fetch, ifd_start, self.bigtiff, self.endianness)
                    .await?;
            let ifd = ifd_reader.read(fetch).await?;
            let next_ifd_offset = ifd_reader.finish(fetch).await?;
            self.next_ifd_offset = next_ifd_offset;
            Ok(Some(ifd))
        } else {
            Ok(None)
        }
    }

    /// Read a single IFD at a given byte offset.
    ///
    /// The spec allows an IFD at any offset in the file. SubIFDs (tag 330) are offsets to
    /// directories outside the top-level chain, so [`read_all_ifds`][Self::read_all_ifds] never
    /// reaches them. Pyramid levels are commonly written that way, so a reader that only follows
    /// the chain sees one resolution level.
    pub async fn read_ifd_at<F: MetadataFetch>(
        &self,
        fetch: &F,
        offset: u64,
    ) -> AsyncTiffResult<ImageFileDirectory> {
        let ifd_reader =
            ImageFileDirectoryReader::open(fetch, offset, self.bigtiff, self.endianness).await?;
        ifd_reader.read(fetch).await
    }

    /// Read all IFDs from the file.
    pub async fn read_all_ifds<F: MetadataFetch>(
        &mut self,
        fetch: &F,
    ) -> AsyncTiffResult<Vec<ImageFileDirectory>> {
        let mut ifds = vec![];
        while let Some(ifd) = self.read_next_ifd(fetch).await? {
            ifds.push(ifd);
        }
        Ok(ifds)
    }

    /// Read all IFDs from the file and return a complete TIFF structure.
    pub async fn read<F: MetadataFetch>(&mut self, fetch: &F) -> AsyncTiffResult<TIFF> {
        let ifds = self.read_all_ifds(fetch).await?;
        Ok(TIFF::new(ifds, self.endianness))
    }
}

/// Reads the [`ImageFileDirectory`] metadata.
///
/// TIFF metadata is not necessarily contiguous in the files: IFDs are normally all stored
/// contiguously in the header, but the spec allows them to be non-contiguous or spread out through
/// the file.
///
/// Note that you must call [`finish`][ImageFileDirectoryReader::finish] to read the offset of the
/// following IFD.
pub struct ImageFileDirectoryReader {
    endianness: Endianness,
    bigtiff: bool,
    /// The byte offset of the beginning of this IFD
    ifd_start_offset: u64,
    /// The number of tags in this IFD
    tag_count: u64,
    /// The number of bytes that each IFD entry takes up.
    /// This is 12 bytes for normal TIFF and 20 bytes for BigTIFF.
    ifd_entry_byte_size: u64,
    /// The number of bytes that the value for the number of tags takes up.
    tag_count_byte_size: u64,
}

impl ImageFileDirectoryReader {
    /// Read and parse the IFD starting at the given file offset
    pub async fn open<F: MetadataFetch>(
        fetch: &F,
        ifd_start_offset: u64,
        bigtiff: bool,
        endianness: Endianness,
    ) -> AsyncTiffResult<Self> {
        let mut cursor = MetadataCursor::new_with_offset(fetch, endianness, ifd_start_offset);

        // Tag   2 bytes
        // Type  2 bytes
        // Count:
        //  - bigtiff: 8 bytes
        //  - else: 4 bytes
        // TagValue:
        //  - bigtiff: 8 bytes either a pointer the value itself
        //  - else: 4 bytes either a pointer the value itself
        let ifd_entry_byte_size = if bigtiff { 20 } else { 12 };
        // The size of `tag_count` that we read above
        let tag_count_byte_size = if bigtiff { 8 } else { 2 };

        let tag_count = if bigtiff {
            cursor.read_u64().await?
        } else {
            cursor.read_u16().await?.into()
        };

        Ok(Self {
            endianness,
            bigtiff,
            ifd_entry_byte_size,
            tag_count,
            tag_count_byte_size,
            ifd_start_offset,
        })
    }

    /// Manually read the tag with the specified index.
    ///
    /// Panics if the tag index is out of range of the tag count.
    ///
    /// This can be useful if you need to access tags at a low level. You'll need to call
    /// [`ImageFileDirectory::from_tags`] on the resulting collection of tags.
    pub async fn read_tag<F: MetadataFetch>(
        &self,
        fetch: &F,
        tag_idx: u64,
    ) -> AsyncTiffResult<(Tag, TagValue)> {
        assert!(tag_idx < self.tag_count);
        let tag_offset =
            self.ifd_start_offset + self.tag_count_byte_size + (self.ifd_entry_byte_size * tag_idx);
        let (tag_name, tag_value) =
            read_tag(fetch, tag_offset, self.endianness, self.bigtiff).await?;
        Ok((tag_name, tag_value))
    }

    /// Read all tags out of this IFD.
    ///
    /// Keep in mind that you'll still need to call [`finish`][Self::finish] to get the byte offset
    /// of the next IFD.
    pub async fn read<F: MetadataFetch>(&self, fetch: &F) -> AsyncTiffResult<ImageFileDirectory> {
        let mut tags = HashMap::with_capacity(self.tag_count as usize);
        for tag_idx in 0..self.tag_count {
            let (tag, value) = self.read_tag(fetch, tag_idx).await?;
            tags.insert(tag, value);
        }
        ImageFileDirectory::from_tags(tags, self.endianness)
    }

    /// Finish this reader, reading the byte offset of the next IFD
    pub async fn finish<F: MetadataFetch>(self, fetch: &F) -> AsyncTiffResult<Option<u64>> {
        // The byte offset for reading the next ifd
        let next_ifd_byte_offset = self.ifd_start_offset
            + self.tag_count_byte_size
            + (self.ifd_entry_byte_size * self.tag_count);
        let mut cursor =
            MetadataCursor::new_with_offset(fetch, self.endianness, next_ifd_byte_offset);

        let next_ifd_offset = if self.bigtiff {
            cursor.read_u64().await?
        } else {
            cursor.read_u32().await?.into()
        };

        // If the ifd_offset is 0, no more IFDs
        if next_ifd_offset == 0 {
            Ok(None)
        } else {
            Ok(Some(next_ifd_offset))
        }
    }
}

/// Read a single tag from the cursor
async fn read_tag<F: MetadataFetch>(
    fetch: &F,
    tag_offset: u64,
    endianness: Endianness,
    bigtiff: bool,
) -> AsyncTiffResult<(Tag, TagValue)> {
    let mut cursor = MetadataCursor::new_with_offset(fetch, endianness, tag_offset);

    let tag_name = Tag::from_u16_exhaustive(cursor.read_u16().await?);

    let tag_type_code = cursor.read_u16().await?;
    let tag_type = Type::from_u16(tag_type_code).expect(
        "Unknown tag type {tag_type_code}. TODO: we should skip entries with unknown tag types.",
    );
    let count = if bigtiff {
        cursor.read_u64().await?
    } else {
        cursor.read_u32().await?.into()
    };

    let tag_value = read_tag_value(&mut cursor, tag_type, count, bigtiff).await?;

    Ok((tag_name, tag_value))
}

/// Read a tag's value from the cursor
///
/// NOTE: this does not maintain cursor state
// This is derived from the upstream tiff crate:
// https://github.com/image-rs/image-tiff/blob/6dc7a266d30291db1e706c8133357931f9e2a053/src/decoder/ifd.rs#L369-L639
async fn read_tag_value<F: MetadataFetch>(
    cursor: &mut MetadataCursor<'_, F>,
    tag_type: Type,
    count: u64,
    bigtiff: bool,
) -> AsyncTiffResult<TagValue> {
    // Case 1: there are no values so we can return immediately.
    if count == 0 {
        return Ok(TagValue::List(vec![]));
    }

    let tag_size = match tag_type {
        Type::BYTE | Type::SBYTE | Type::ASCII | Type::UNDEFINED => 1,
        Type::SHORT | Type::SSHORT => 2,
        Type::LONG | Type::SLONG | Type::FLOAT | Type::IFD => 4,
        Type::LONG8
        | Type::SLONG8
        | Type::DOUBLE
        | Type::RATIONAL
        | Type::SRATIONAL
        | Type::IFD8 => 8,
    };

    let value_byte_length = count.checked_mul(tag_size).unwrap();

    // Case 2: there is one value.
    if count == 1 {
        // 2a: the value is 5-8 bytes and we're in BigTiff mode.
        if bigtiff && value_byte_length > 4 && value_byte_length <= 8 {
            let mut data = cursor.read(value_byte_length).await?;

            return Ok(match tag_type {
                Type::LONG8 => TagValue::UnsignedBig(data.read_u64()?),
                Type::SLONG8 => TagValue::SignedBig(data.read_i64()?),
                Type::DOUBLE => TagValue::Double(data.read_f64()?),
                Type::RATIONAL => TagValue::Rational(data.read_u32()?, data.read_u32()?),
                Type::SRATIONAL => TagValue::SRational(data.read_i32()?, data.read_i32()?),
                Type::IFD8 => TagValue::IfdBig(data.read_u64()?),
                Type::BYTE
                | Type::SBYTE
                | Type::ASCII
                | Type::UNDEFINED
                | Type::SHORT
                | Type::SSHORT
                | Type::LONG
                | Type::SLONG
                | Type::FLOAT
                | Type::IFD => unreachable!(),
            });
        }

        // NOTE: we should only be reading value_byte_length when it's 4 bytes or fewer. Right now
        // we're reading even if it's 8 bytes, but then only using the first 4 bytes of this
        // buffer.
        let mut data = cursor.read(value_byte_length).await?;

        // 2b: the value is at most 4 bytes or doesn't fit in the offset field.
        return Ok(match tag_type {
            Type::BYTE | Type::UNDEFINED => TagValue::Byte(data.read_u8()?),
            Type::SBYTE => TagValue::SignedByte(data.read_i8()?),
            Type::SHORT => TagValue::Short(data.read_u16()?),
            Type::SSHORT => TagValue::SignedShort(data.read_i16()?),
            Type::LONG => TagValue::Unsigned(data.read_u32()?),
            Type::SLONG => TagValue::Signed(data.read_i32()?),
            Type::FLOAT => TagValue::Float(data.read_f32()?),
            Type::ASCII => {
                if data.as_ref()[0] == 0 {
                    TagValue::Ascii("".to_string())
                } else {
                    panic!("Invalid tag");
                    // return Err(TiffError::FormatError(TiffFormatError::InvalidTag));
                }
            }
            Type::LONG8 => {
                let offset = data.read_u32()?;
                cursor.seek(offset as _);
                TagValue::UnsignedBig(cursor.read_u64().await?)
            }
            Type::SLONG8 => {
                let offset = data.read_u32()?;
                cursor.seek(offset as _);
                TagValue::SignedBig(cursor.read_i64().await?)
            }
            Type::DOUBLE => {
                let offset = data.read_u32()?;
                cursor.seek(offset as _);
                TagValue::Double(cursor.read_f64().await?)
            }
            Type::RATIONAL => {
                let offset = data.read_u32()?;
                cursor.seek(offset as _);
                let numerator = cursor.read_u32().await?;
                let denominator = cursor.read_u32().await?;
                TagValue::Rational(numerator, denominator)
            }
            Type::SRATIONAL => {
                let offset = data.read_u32()?;
                cursor.seek(offset as _);
                let numerator = cursor.read_i32().await?;
                let denominator = cursor.read_i32().await?;
                TagValue::SRational(numerator, denominator)
            }
            Type::IFD => TagValue::Ifd(data.read_u32()?),
            Type::IFD8 => {
                let offset = data.read_u32()?;
                cursor.seek(offset as _);
                TagValue::IfdBig(cursor.read_u64().await?)
            }
        });
    }

    // Case 3: There is more than one value, but it fits in the offset field.
    if value_byte_length <= 4 || bigtiff && value_byte_length <= 8 {
        let mut data = cursor.read(value_byte_length).await?;
        if bigtiff {
            cursor.advance(8 - value_byte_length);
        } else {
            cursor.advance(4 - value_byte_length);
        }

        match tag_type {
            Type::BYTE | Type::UNDEFINED => {
                return {
                    Ok(TagValue::List(
                        (0..count)
                            .map(|_| TagValue::Byte(data.read_u8().unwrap()))
                            .collect(),
                    ))
                };
            }
            Type::SBYTE => {
                return {
                    Ok(TagValue::List(
                        (0..count)
                            .map(|_| TagValue::SignedByte(data.read_i8().unwrap()))
                            .collect(),
                    ))
                }
            }
            Type::ASCII => {
                let mut buf = vec![0; count as usize];
                data.read_exact(&mut buf)?;
                if buf.is_ascii() && buf.ends_with(&[0]) {
                    let v = std::str::from_utf8(&buf)
                        .map_err(|err| AsyncTiffError::General(err.to_string()))?;
                    let v = v.trim_matches(char::from(0));
                    return Ok(TagValue::Ascii(v.into()));
                } else {
                    panic!("Invalid tag");
                    // return Err(TiffError::FormatError(TiffFormatError::InvalidTag));
                }
            }
            Type::SHORT => {
                let mut v = Vec::new();
                for _ in 0..count {
                    v.push(TagValue::Short(data.read_u16()?));
                }
                return Ok(TagValue::List(v));
            }
            Type::SSHORT => {
                let mut v = Vec::new();
                for _ in 0..count {
                    v.push(TagValue::SignedShort(data.read_i16()?));
                }
                return Ok(TagValue::List(v));
            }
            Type::LONG => {
                let mut v = Vec::new();
                for _ in 0..count {
                    v.push(TagValue::Unsigned(data.read_u32()?));
                }
                return Ok(TagValue::List(v));
            }
            Type::SLONG => {
                let mut v = Vec::new();
                for _ in 0..count {
                    v.push(TagValue::Signed(data.read_i32()?));
                }
                return Ok(TagValue::List(v));
            }
            Type::FLOAT => {
                let mut v = Vec::new();
                for _ in 0..count {
                    v.push(TagValue::Float(data.read_f32()?));
                }
                return Ok(TagValue::List(v));
            }
            Type::IFD => {
                let mut v = Vec::new();
                for _ in 0..count {
                    v.push(TagValue::Ifd(data.read_u32()?));
                }
                return Ok(TagValue::List(v));
            }
            Type::LONG8
            | Type::SLONG8
            | Type::RATIONAL
            | Type::SRATIONAL
            | Type::DOUBLE
            | Type::IFD8 => {
                unreachable!()
            }
        }
    }

    // Seek cursor
    let offset = if bigtiff {
        cursor.read_u64().await?
    } else {
        cursor.read_u32().await?.into()
    };
    cursor.seek(offset);

    // Case 4: there is more than one value, and it doesn't fit in the offset field.
    match tag_type {
        // TODO check if this could give wrong results
        // at a different endianess of file/computer.
        Type::BYTE | Type::UNDEFINED => {
            let mut v = Vec::with_capacity(count as _);
            for _ in 0..count {
                v.push(TagValue::Byte(cursor.read_u8().await?))
            }
            Ok(TagValue::List(v))
        }
        Type::SBYTE => {
            let mut v = Vec::with_capacity(count as _);
            for _ in 0..count {
                v.push(TagValue::SignedByte(cursor.read_i8().await?))
            }
            Ok(TagValue::List(v))
        }
        Type::SHORT => {
            let mut v = Vec::with_capacity(count as _);
            for _ in 0..count {
                v.push(TagValue::Short(cursor.read_u16().await?))
            }
            Ok(TagValue::List(v))
        }
        Type::SSHORT => {
            let mut v = Vec::with_capacity(count as _);
            for _ in 0..count {
                v.push(TagValue::SignedShort(cursor.read_i16().await?))
            }
            Ok(TagValue::List(v))
        }
        Type::LONG => {
            let mut v = Vec::with_capacity(count as _);
            for _ in 0..count {
                v.push(TagValue::Unsigned(cursor.read_u32().await?))
            }
            Ok(TagValue::List(v))
        }
        Type::SLONG => {
            let mut v = Vec::with_capacity(count as _);
            for _ in 0..count {
                v.push(TagValue::Signed(cursor.read_i32().await?))
            }
            Ok(TagValue::List(v))
        }
        Type::FLOAT => {
            let mut v = Vec::with_capacity(count as _);
            for _ in 0..count {
                v.push(TagValue::Float(cursor.read_f32().await?))
            }
            Ok(TagValue::List(v))
        }
        Type::DOUBLE => {
            let mut v = Vec::with_capacity(count as _);
            for _ in 0..count {
                v.push(TagValue::Double(cursor.read_f64().await?))
            }
            Ok(TagValue::List(v))
        }
        Type::RATIONAL => {
            let mut v = Vec::with_capacity(count as _);
            for _ in 0..count {
                v.push(TagValue::Rational(
                    cursor.read_u32().await?,
                    cursor.read_u32().await?,
                ))
            }
            Ok(TagValue::List(v))
        }
        Type::SRATIONAL => {
            let mut v = Vec::with_capacity(count as _);
            for _ in 0..count {
                v.push(TagValue::SRational(
                    cursor.read_i32().await?,
                    cursor.read_i32().await?,
                ))
            }
            Ok(TagValue::List(v))
        }
        Type::LONG8 => {
            let mut v = Vec::with_capacity(count as _);
            for _ in 0..count {
                v.push(TagValue::UnsignedBig(cursor.read_u64().await?))
            }
            Ok(TagValue::List(v))
        }
        Type::SLONG8 => {
            let mut v = Vec::with_capacity(count as _);
            for _ in 0..count {
                v.push(TagValue::SignedBig(cursor.read_i64().await?))
            }
            Ok(TagValue::List(v))
        }
        Type::IFD => {
            let mut v = Vec::with_capacity(count as _);
            for _ in 0..count {
                v.push(TagValue::Ifd(cursor.read_u32().await?))
            }
            Ok(TagValue::List(v))
        }
        Type::IFD8 => {
            let mut v = Vec::with_capacity(count as _);
            for _ in 0..count {
                v.push(TagValue::IfdBig(cursor.read_u64().await?))
            }
            Ok(TagValue::List(v))
        }
        Type::ASCII => {
            let mut out = vec![0; count as _];
            let mut buf = cursor.read(count).await?;
            buf.read_exact(&mut out)?;

            // Strings may be null-terminated, so we trim anything downstream of the null byte
            if let Some(first) = out.iter().position(|&b| b == 0) {
                out.truncate(first);
            }
            Ok(TagValue::Ascii(String::from_utf8_lossy(&out).into_owned()))
        }
    }
}

#[cfg(test)]
mod test {
    use async_trait::async_trait;

    use super::*;

    #[cfg_attr(target_arch = "wasm32", async_trait(?Send))]
    #[cfg_attr(not(target_arch = "wasm32"), async_trait)]
    impl MetadataFetch for Bytes {
        async fn fetch(&self, range: std::ops::Range<u64>) -> crate::error::AsyncTiffResult<Bytes> {
            let usize_range = range.start as usize..range.end as usize;
            Ok(self.slice(usize_range))
        }
    }

    #[tokio::test]
    #[rustfmt::skip]
    async fn test_single_fits_notbig() {
        // personal sanity checks
        assert_eq!(u16::from_le_bytes([42,0]),42);
        assert_eq!(u16::from_be_bytes([0,42]),42);
        assert_eq!(f32::from_le_bytes([0x42,0,0,0]),f32::from_bits(0x00_00_00_42));
        assert_eq!(f32::from_be_bytes([0,0,0,0x42]),f32::from_bits(0x00_00_00_42));
        let cases= [
        // tag type   count      offset
        // /\  / \   /     \   /       \
        ([1,1, 1, 0, 1,0,0,0, 42, 0, 0, 0], Endianness::LittleEndian, TagValue::Byte      (42                )),
        ([1,1, 0, 1, 0,0,0,1, 42, 0, 0, 0], Endianness::BigEndian,    TagValue::Byte      (42                )),
        ([1,1, 6, 0, 1,0,0,0, 42, 0, 0, 0], Endianness::LittleEndian, TagValue::SignedByte(42                )),
        ([1,1, 0, 6, 0,0,0,1, 42, 0, 0, 0], Endianness::BigEndian,    TagValue::SignedByte(42                )),
        ([1,1, 7, 0, 1,0,0,0, 42, 0, 0, 0], Endianness::LittleEndian, TagValue::Byte      (42                )), // undefined
        ([1,1, 0, 7, 0,0,0,1, 42, 0, 0, 0], Endianness::BigEndian,    TagValue::Byte      (42                )), // undefined
        ([1,1, 2, 0, 1,0,0,0,  0, 0, 0, 0], Endianness::LittleEndian, TagValue::Ascii     ("".into()         )),
        ([1,1, 0, 2, 0,0,0,1,  0, 0, 0, 0], Endianness::BigEndian,    TagValue::Ascii     ("".into()         )),
        ([1,1, 3, 0, 1,0,0,0, 42, 0, 0, 0], Endianness::LittleEndian, TagValue::Short       (42              )),
        ([1,1, 0, 3, 0,0,0,1,  0,42, 0, 0], Endianness::BigEndian,    TagValue::Short       (42              )),
        ([1,1, 8, 0, 1,0,0,0, 42, 0, 0, 0], Endianness::LittleEndian, TagValue::SignedShort (42              )),
        ([1,1, 0, 8, 0,0,0,1,  0,42, 0, 0], Endianness::BigEndian,    TagValue::SignedShort (42              )),
        ([1,1, 4, 0, 1,0,0,0, 42, 0, 0, 0], Endianness::LittleEndian, TagValue::Unsigned  (42                )),
        ([1,1, 0, 4, 0,0,0,1,  0, 0, 0,42], Endianness::BigEndian,    TagValue::Unsigned  (42                )),
        ([1,1, 9, 0, 1,0,0,0, 42, 0, 0, 0], Endianness::LittleEndian, TagValue::Signed    (42                )),
        ([1,1, 0, 9, 0,0,0,1,  0, 0, 0,42], Endianness::BigEndian,    TagValue::Signed    (42                )),
        ([1,1,13, 0, 1,0,0,0, 42, 0, 0, 0], Endianness::LittleEndian, TagValue::Ifd       (42                )),
        ([1,1, 0,13, 0,0,0,1,  0, 0, 0,42], Endianness::BigEndian,    TagValue::Ifd       (42                )),
        ([1,1,11, 0, 1,0,0,0, 42, 0, 0, 0], Endianness::LittleEndian, TagValue::Float     (f32::from_bits(42))),
        ([1,1, 0,11, 0,0,0,1,  0, 0, 0,42], Endianness::BigEndian,    TagValue::Float     (f32::from_bits(42))),
        // Double doesn't fit, neither 8-types and we special-case IFD
        ];
        for (buf, byte_order, res) in cases {
                let fetch = Bytes::copy_from_slice(&buf);
            assert_eq!(
                read_tag(&fetch, 0, byte_order, false).await.unwrap(),
                (Tag::from_u16_exhaustive(0x01_01),res)
            );
        }
    }

    #[tokio::test]
    #[rustfmt::skip]
    async fn test_single_fits_big() {
        // personal sanity checks
        assert_eq!(u16::from_le_bytes([42,0]),42);
        assert_eq!(u16::from_be_bytes([0,42]),42);

        assert_eq!(f32::from_le_bytes([0x42,0,0,0]),f32::from_bits(0x00_00_00_42));
        assert_eq!(f32::from_be_bytes([0,0,0,0x42]),f32::from_bits(0x00_00_00_42));
        let cases = [
        //      type       count            offset
        //       / \  1 2 3 4 5 6 7 8   1  2  3  4  5  6  7  8
        ([1,1,  1, 0, 1,0,0,0,0,0,0,0, 42, 0, 0, 0, 0, 0, 0, 0], Endianness::LittleEndian, TagValue::Byte       (42)                ),
        ([1,1,  0, 1, 0,0,0,0,0,0,0,1, 42, 0, 0, 0, 0, 0, 0, 0], Endianness::BigEndian,    TagValue::Byte       (42)                ),
        ([1,1,  6, 0, 1,0,0,0,0,0,0,0, 42, 0, 0, 0, 0, 0, 0, 0], Endianness::LittleEndian, TagValue::SignedByte (42)                ),
        ([1,1,  0, 6, 0,0,0,0,0,0,0,1, 42, 0, 0, 0, 0, 0, 0, 0], Endianness::BigEndian,    TagValue::SignedByte (42)                ),
        ([1,1,  7, 0, 1,0,0,0,0,0,0,0, 42, 0, 0, 0, 0, 0, 0, 0], Endianness::LittleEndian, TagValue::Byte       (42)                ), // undefined
        ([1,1,  0, 7, 0,0,0,0,0,0,0,1, 42, 0, 0, 0, 0, 0, 0, 0], Endianness::BigEndian,    TagValue::Byte       (42)                ), // undefined
        ([1,1,  2, 0, 1,0,0,0,0,0,0,0,  0, 0, 0, 0, 0, 0, 0, 0], Endianness::LittleEndian, TagValue::Ascii      ("".into())         ),
        ([1,1,  0, 2, 0,0,0,0,0,0,0,1,  0, 0, 0, 0, 0, 0, 0, 0], Endianness::BigEndian,    TagValue::Ascii      ("".into())         ),
        ([1,1,  3, 0, 1,0,0,0,0,0,0,0, 42, 0, 0, 0, 0, 0, 0, 0], Endianness::LittleEndian, TagValue::Short      (42)                ),
        ([1,1,  0, 3, 0,0,0,0,0,0,0,1,  0,42, 0, 0, 0, 0, 0, 0], Endianness::BigEndian,    TagValue::Short      (42)                ),
        ([1,1,  8, 0, 1,0,0,0,0,0,0,0, 42, 0, 0, 0, 0, 0, 0, 0], Endianness::LittleEndian, TagValue::SignedShort(42)                ),
        ([1,1,  0, 8, 0,0,0,0,0,0,0,1,  0,42, 0, 0, 0, 0, 0, 0], Endianness::BigEndian,    TagValue::SignedShort(42)                ),
        ([1,1,  4, 0, 1,0,0,0,0,0,0,0, 42, 0, 0, 0, 0, 0, 0, 0], Endianness::LittleEndian, TagValue::Unsigned   (42)                ),
        ([1,1,  0, 4, 0,0,0,0,0,0,0,1,  0, 0, 0,42, 0, 0, 0, 0], Endianness::BigEndian,    TagValue::Unsigned   (42)                ),
        ([1,1,  9, 0, 1,0,0,0,0,0,0,0, 42, 0, 0, 0, 0, 0, 0, 0], Endianness::LittleEndian, TagValue::Signed     (42)                ),
        ([1,1,  0, 9, 0,0,0,0,0,0,0,1,  0, 0, 0,42, 0, 0, 0, 0], Endianness::BigEndian,    TagValue::Signed     (42)                ),
        ([1,1, 13, 0, 1,0,0,0,0,0,0,0, 42, 0, 0, 0, 0, 0, 0, 0], Endianness::LittleEndian, TagValue::Ifd        (42)                ),
        ([1,1,  0,13, 0,0,0,0,0,0,0,1,  0, 0, 0,42, 0, 0, 0, 0], Endianness::BigEndian,    TagValue::Ifd        (42)                ),
        ([1,1, 16, 0, 1,0,0,0,0,0,0,0, 42, 0, 0, 0, 0, 0, 0, 0], Endianness::LittleEndian, TagValue::UnsignedBig(42)                ),
        ([1,1,  0,16, 0,0,0,0,0,0,0,1,  0, 0, 0, 0, 0, 0, 0,42], Endianness::BigEndian,    TagValue::UnsignedBig(42)                ),
        ([1,1, 17, 0, 1,0,0,0,0,0,0,0, 42, 0, 0, 0, 0, 0, 0, 0], Endianness::LittleEndian, TagValue::SignedBig  (42)                ),
        ([1,1,  0,17, 0,0,0,0,0,0,0,1,  0, 0, 0, 0, 0, 0, 0,42], Endianness::BigEndian,    TagValue::SignedBig  (42)                ),
        ([1,1, 18, 0, 1,0,0,0,0,0,0,0, 42, 0, 0, 0, 0, 0, 0, 0], Endianness::LittleEndian, TagValue::IfdBig     (42)                ),
        ([1,1,  0,18, 0,0,0,0,0,0,0,1,  0, 0, 0, 0, 0, 0, 0,42], Endianness::BigEndian,    TagValue::IfdBig     (42)                ),
        ([1,1, 11, 0, 1,0,0,0,0,0,0,0, 42, 0, 0, 0, 0, 0, 0, 0], Endianness::LittleEndian, TagValue::Float      (f32::from_bits(42))),
        ([1,1,  0,11, 0,0,0,0,0,0,0,1,  0, 0, 0,42, 0, 0, 0, 0], Endianness::BigEndian,    TagValue::Float      (f32::from_bits(42))),
        ([1,1, 12, 0, 1,0,0,0,0,0,0,0, 42, 0, 0, 0, 0, 0, 0, 0], Endianness::LittleEndian, TagValue::Double     (f64::from_bits(42))),
        ([1,1,  0,12, 0,0,0,0,0,0,0,1,  0, 0, 0, 0, 0, 0, 0,42], Endianness::BigEndian,    TagValue::Double     (f64::from_bits(42))),
        ([1,1,  5, 0, 1,0,0,0,0,0,0,0,  42,0, 0, 0,43, 0, 0, 0], Endianness::LittleEndian, TagValue::Rational   (42, 43)            ),
        ([1,1,  0, 5, 0,0,0,0,0,0,0,1,  0, 0, 0,42, 0, 0, 0,43], Endianness::BigEndian,    TagValue::Rational   (42, 43)            ),
        ([1,1,  10,0, 1,0,0,0,0,0,0,0, 42, 0, 0, 0,43, 0, 0, 0], Endianness::LittleEndian, TagValue::SRational  (42, 43)            ),
        ([1,1,  0,10, 0,0,0,0,0,0,0,1,  0, 0, 0,42, 0, 0, 0,43], Endianness::BigEndian,    TagValue::SRational  (42, 43)            ),
        // we special-case IFD
        ];
        for (buf, byte_order, res) in cases {
            let fetch = Bytes::copy_from_slice(&buf);
            assert_eq!(
                read_tag(&fetch, 0, byte_order, true).await.unwrap(),
                (Tag::from_u16_exhaustive(0x0101), res)
            )
        }
    }

    #[tokio::test]
    #[rustfmt::skip]
    async fn test_fits_multi_notbig() {
        // personal sanity checks
        assert_eq!(u16::from_le_bytes([42,0]),42);
        assert_eq!(u16::from_be_bytes([0,42]),42);

        assert_eq!(f32::from_le_bytes([0x42,0,0,0]),f32::from_bits(0x00_00_00_42));
        assert_eq!(f32::from_be_bytes([0,0,0,0x42]),f32::from_bits(0x00_00_00_42));
        let cases = [
        //  tag type  count    offset
        //  // /  \  /     \   /     \
        ([1,1, 1, 0, 4,0,0,0, 42,42,42,42], Endianness::LittleEndian, TagValue::List(vec![TagValue::Byte       (42); 4]) ),
        ([1,1, 0, 1, 0,0,0,4, 42,42,42,42], Endianness::BigEndian,    TagValue::List(vec![TagValue::Byte       (42); 4]) ),
        ([1,1, 6, 0, 4,0,0,0, 42,42,42,42], Endianness::LittleEndian, TagValue::List(vec![TagValue::SignedByte (42); 4]) ),
        ([1,1, 0, 6, 0,0,0,4, 42,42,42,42], Endianness::BigEndian,    TagValue::List(vec![TagValue::SignedByte (42); 4]) ),
        ([1,1, 7, 0, 4,0,0,0, 42,42,42,42], Endianness::LittleEndian, TagValue::List(vec![TagValue::Byte     (42); 4]) ), // undefined
        ([1,1, 0, 7, 0,0,0,4, 42,42,42,42], Endianness::BigEndian,    TagValue::List(vec![TagValue::Byte     (42); 4]) ), // undefined
        ([1,1, 2, 0, 4,0,0,0, 42,42,42, 0], Endianness::LittleEndian, TagValue::Ascii("***".into())),
        ([1,1, 0, 2, 0,0,0,4, 42,42,42, 0], Endianness::BigEndian,    TagValue::Ascii("***".into())),
        ([1,1, 3, 0, 2,0,0,0, 42, 0,42, 0], Endianness::LittleEndian, TagValue::List(vec![TagValue::Short       (42); 2]) ),
        ([1,1, 0, 3, 0,0,0,2,  0,42, 0,42], Endianness::BigEndian,    TagValue::List(vec![TagValue::Short       (42); 2]) ),
        ([1,1, 8, 0, 2,0,0,0, 42, 0,42, 0], Endianness::LittleEndian, TagValue::List(vec![TagValue::SignedShort (42); 2]) ),
        ([1,1, 0, 8, 0,0,0,2,  0,42, 0,42], Endianness::BigEndian,    TagValue::List(vec![TagValue::SignedShort (42); 2]) ),
        ([1,1, 0, 2, 0,0,0,4, b'A',b'B',b'C',0], Endianness::BigEndian, TagValue::Ascii("ABC".into())),
        // others don't fit, neither 8-types and we special-case IFD
        ];
        for (buf, byte_order, res) in cases {
            println!("testing {buf:?} to be {res:?}");
            let fetch = Bytes::copy_from_slice(&buf);
            assert_eq!(
                read_tag(&fetch, 0, byte_order, false).await.unwrap(),
                (Tag::from_u16_exhaustive(0x0101), res)
            )
        }
    }

    #[tokio::test]
    #[rustfmt::skip]
    async fn test_fits_multi_big() {
        // personal sanity checks
        assert_eq!(u16::from_le_bytes([42,0]),42);
        assert_eq!(u16::from_be_bytes([0,42]),42);

        assert_eq!(f32::from_le_bytes([0x42,0,0,0]),f32::from_bits(0x00_00_00_42));
        assert_eq!(f32::from_be_bytes([0,0,0,0x42]),f32::from_bits(0x00_00_00_42));
        let cases = [
        //     type       count            offset
        //     / \  1 2 3 4 5 6 7 8   1  2  3  4  5  6  7  8
        ([1,1, 1, 0, 8,0,0,0,0,0,0,0, 42,42,42,42,42,42,42,42], Endianness::LittleEndian, TagValue::List(vec![TagValue::Byte      (42)                ; 8])),
        ([1,1, 0, 1, 0,0,0,0,0,0,0,8, 42,42,42,42,42,42,42,42], Endianness::BigEndian,    TagValue::List(vec![TagValue::Byte      (42)                ; 8])),
        ([1,1, 6, 0, 8,0,0,0,0,0,0,0, 42,42,42,42,42,42,42,42], Endianness::LittleEndian, TagValue::List(vec![TagValue::SignedByte(42)                ; 8])),
        ([1,1, 0, 6, 0,0,0,0,0,0,0,8, 42,42,42,42,42,42,42,42], Endianness::BigEndian,    TagValue::List(vec![TagValue::SignedByte(42)                ; 8])),
        ([1,1, 7, 0, 8,0,0,0,0,0,0,0, 42,42,42,42,42,42,42,42], Endianness::LittleEndian, TagValue::List(vec![TagValue::Byte      (42)                ; 8])), //undefined u8
        ([1,1, 0, 7, 0,0,0,0,0,0,0,8, 42,42,42,42,42,42,42,42], Endianness::BigEndian,    TagValue::List(vec![TagValue::Byte      (42)                ; 8])), //undefined u8
        ([1,1, 2, 0, 8,0,0,0,0,0,0,0, 42,42,42,42,42,42,42, 0], Endianness::LittleEndian, TagValue::Ascii                      ("*******".into()       )),
        ([1,1, 0, 2, 0,0,0,0,0,0,0,8, 42,42,42,42,42,42,42, 0], Endianness::BigEndian,    TagValue::Ascii                      ("*******".into()       )),
        ([1,1, 3, 0, 4,0,0,0,0,0,0,0, 42, 0,42, 0,42, 0,42, 0], Endianness::LittleEndian, TagValue::List(vec![TagValue::Short       (42)              ; 4])),
        ([1,1, 0, 3, 0,0,0,0,0,0,0,4,  0,42, 0,42, 0,42, 0,42], Endianness::BigEndian,    TagValue::List(vec![TagValue::Short       (42)              ; 4])),
        ([1,1, 8, 0, 4,0,0,0,0,0,0,0, 42, 0,42, 0,42, 0,42, 0], Endianness::LittleEndian, TagValue::List(vec![TagValue::SignedShort (42)              ; 4])),
        ([1,1, 0, 8, 0,0,0,0,0,0,0,4,  0,42, 0,42, 0,42, 0,42], Endianness::BigEndian,    TagValue::List(vec![TagValue::SignedShort (42)              ; 4])),
        ([1,1, 4, 0, 2,0,0,0,0,0,0,0, 42, 0, 0, 0,42, 0, 0, 0], Endianness::LittleEndian, TagValue::List(vec![TagValue::Unsigned  (42)                ; 2])),
        ([1,1, 0, 4, 0,0,0,0,0,0,0,2,  0, 0, 0,42, 0, 0, 0,42], Endianness::BigEndian,    TagValue::List(vec![TagValue::Unsigned  (42)                ; 2])),
        ([1,1, 9, 0, 2,0,0,0,0,0,0,0, 42, 0, 0, 0,42, 0, 0, 0], Endianness::LittleEndian, TagValue::List(vec![TagValue::Signed    (42)                ; 2])),
        ([1,1, 0, 9, 0,0,0,0,0,0,0,2,  0, 0, 0,42, 0, 0, 0,42], Endianness::BigEndian,    TagValue::List(vec![TagValue::Signed    (42)                ; 2])),
        ([1,1,13, 0, 2,0,0,0,0,0,0,0, 42, 0, 0, 0,42, 0, 0, 0], Endianness::LittleEndian, TagValue::List(vec![TagValue::Ifd       (42)                ; 2])),
        ([1,1, 0,13, 0,0,0,0,0,0,0,2,  0, 0, 0,42, 0, 0, 0,42], Endianness::BigEndian,    TagValue::List(vec![TagValue::Ifd       (42)                ; 2])),
        ([1,1,11, 0, 2,0,0,0,0,0,0,0, 42, 0, 0, 0,42, 0, 0, 0], Endianness::LittleEndian, TagValue::List(vec![TagValue::Float     (f32::from_bits(42)); 2])),
        ([1,1, 0,11, 0,0,0,0,0,0,0,2,  0, 0, 0,42, 0, 0, 0,42], Endianness::BigEndian,    TagValue::List(vec![TagValue::Float     (f32::from_bits(42)); 2])),
        // we special-case IFD
        ];
        for (buf, byte_order, res) in cases {
            let fetch = Bytes::copy_from_slice(&buf);
            assert_eq!(
                read_tag(&fetch, 0, byte_order, true).await.unwrap(),
                (Tag::from_u16_exhaustive(0x0101), res)
            )
        }
    }

    #[tokio::test]
    #[rustfmt::skip]
    async fn test_notfits_notbig() {
        // personal sanity checks
        assert_eq!(u16::from_le_bytes([42,0]),42);
        assert_eq!(u16::from_be_bytes([0,42]),42);

        assert_eq!(f32::from_le_bytes([0x42,0,0,0]),f32::from_bits(0x00_00_00_42));
        assert_eq!(f32::from_be_bytes([0,0,0,0x42]),f32::from_bits(0x00_00_00_42));
        let cases = [
        //          type  count    offset 12
        //          /\   /     \   /     \
        (vec![1,1, 1, 0, 5,0,0,0, 12, 0, 0, 0, 42,42,42,42,42],          Endianness::LittleEndian, TagValue::List(vec![TagValue::Byte       (42                 );5])),
        (vec![1,1, 0, 1, 0,0,0,5,  0, 0, 0,12, 42,42,42,42,42],          Endianness::BigEndian   , TagValue::List(vec![TagValue::Byte       (42                 );5])),
        (vec![1,1, 6, 0, 5,0,0,0, 12, 0, 0, 0, 42,42,42,42,42],          Endianness::LittleEndian, TagValue::List(vec![TagValue::SignedByte (42                 );5])),
        (vec![1,1, 0, 6, 0,0,0,5,  0, 0, 0,12, 42,42,42,42,42],          Endianness::BigEndian   , TagValue::List(vec![TagValue::SignedByte (42                 );5])),
        (vec![1,1, 7, 0, 5,0,0,0, 12, 0, 0, 0, 42,42,42,42,42],          Endianness::LittleEndian, TagValue::List(vec![TagValue::Byte       (42                 );5])), // Type::UNDEFINED ),
        (vec![1,1, 0, 7, 0,0,0,5,  0, 0, 0,12, 42,42,42,42,42],          Endianness::BigEndian   , TagValue::List(vec![TagValue::Byte       (42                 );5])), // Type::UNDEFINED ),
        (vec![1,1, 2, 0, 5,0,0,0, 12, 0, 0, 0, 42,42,42,42, 0],          Endianness::LittleEndian,                  TagValue::Ascii      ("****".into()      )    ),
        (vec![1,1, 0, 2, 0,0,0,5,  0, 0, 0,12, 42,42,42,42, 0],          Endianness::BigEndian   ,                  TagValue::Ascii      ("****".into()      )    ),
        (vec![1,1, 3, 0, 3,0,0,0, 12, 0, 0, 0, 42, 0,42, 0,42, 0],       Endianness::LittleEndian, TagValue::List(vec![TagValue::Short      (42                 );3])),
        (vec![1,1, 0, 3, 0,0,0,3,  0, 0, 0,12,  0,42, 0,42, 0,42],       Endianness::BigEndian   , TagValue::List(vec![TagValue::Short      (42                 );3])),
        (vec![1,1, 8, 0, 3,0,0,0, 12, 0, 0, 0, 42, 0,42, 0,42, 0],       Endianness::LittleEndian, TagValue::List(vec![TagValue::SignedShort(42                 );3])),
        (vec![1,1, 0, 8, 0,0,0,3,  0, 0, 0,12,  0,42, 0,42, 0,42],       Endianness::BigEndian   , TagValue::List(vec![TagValue::SignedShort(42                 );3])),
        (vec![1,1, 4, 0, 2,0,0,0, 12, 0, 0, 0, 42, 0, 0, 0,42, 0, 0, 0], Endianness::LittleEndian, TagValue::List(vec![TagValue::Unsigned   (42                 );2])),
        (vec![1,1, 0, 4, 0,0,0,2,  0, 0, 0,12,  0, 0, 0,42, 0, 0, 0,42], Endianness::BigEndian   , TagValue::List(vec![TagValue::Unsigned   (42                 );2])),
        (vec![1,1, 9, 0, 2,0,0,0, 12, 0, 0, 0, 42, 0, 0, 0,42, 0, 0, 0], Endianness::LittleEndian, TagValue::List(vec![TagValue::Signed     (42                 );2])),
        (vec![1,1, 0, 9, 0,0,0,2,  0, 0, 0,12,  0, 0, 0,42, 0, 0, 0,42], Endianness::BigEndian   , TagValue::List(vec![TagValue::Signed     (42                 );2])),
        (vec![1,1,13, 0, 2,0,0,0, 12, 0, 0, 0, 42, 0, 0, 0,42, 0, 0, 0], Endianness::LittleEndian, TagValue::List(vec![TagValue::Ifd        (42                 );2])),
        (vec![1,1, 0,13, 0,0,0,2,  0, 0, 0,12,  0, 0, 0,42, 0, 0, 0,42], Endianness::BigEndian   , TagValue::List(vec![TagValue::Ifd        (42                 );2])),
        (vec![1,1, 16,0, 1,0,0,0, 12, 0, 0, 0, 42, 0, 0, 0, 0, 0, 0, 0], Endianness::LittleEndian,                  TagValue::UnsignedBig(42                 )    ),
        (vec![1,1, 0,16, 0,0,0,1,  0, 0, 0,12,  0, 0, 0, 0, 0, 0, 0,42], Endianness::BigEndian   ,                  TagValue::UnsignedBig(42                 )    ),
        (vec![1,1, 17,0, 1,0,0,0, 12, 0, 0, 0, 42, 0, 0, 0, 0, 0, 0, 0], Endianness::LittleEndian,                  TagValue::SignedBig  (42                 )    ),
        (vec![1,1, 0,17, 0,0,0,1,  0, 0, 0,12,  0, 0, 0, 0, 0, 0, 0,42], Endianness::BigEndian   ,                  TagValue::SignedBig  (42                 )    ),
        (vec![1,1, 18,0, 1,0,0,0, 12, 0, 0, 0, 42, 0, 0, 0, 0, 0, 0, 0], Endianness::LittleEndian,                  TagValue::IfdBig     (42                 )    ),
        (vec![1,1, 0,18, 0,0,0,1,  0, 0, 0,12,  0, 0, 0, 0, 0, 0, 0,42], Endianness::BigEndian   ,                  TagValue::IfdBig     (42                 )    ),
        (vec![1,1, 11,0, 2,0,0,0, 12, 0, 0, 0, 42, 0, 0, 0,42, 0, 0, 0], Endianness::LittleEndian, TagValue::List(vec![TagValue::Float      (f32::from_bits(42) );2])),
        (vec![1,1, 0,11, 0,0,0,2,  0, 0, 0,12,  0, 0, 0,42, 0, 0, 0,42], Endianness::BigEndian   , TagValue::List(vec![TagValue::Float      (f32::from_bits(42) );2])),
        (vec![1,1, 12,0, 1,0,0,0, 12, 0, 0, 0, 42, 0, 0, 0, 0, 0, 0, 0], Endianness::LittleEndian,                  TagValue::Double     (f64::from_bits(42))     ),
        (vec![1,1, 0,12, 0,0,0,1,  0, 0, 0,12,  0, 0, 0, 0, 0, 0, 0,42], Endianness::BigEndian   ,                  TagValue::Double     (f64::from_bits(42))     ),
        (vec![1,1, 5, 0, 1,0,0,0, 12, 0, 0, 0, 42, 0, 0, 0,42, 0, 0, 0], Endianness::LittleEndian,                  TagValue::Rational   (42, 42             )    ),
        (vec![1,1, 0, 5, 0,0,0,1,  0, 0, 0,12,  0, 0, 0,42, 0, 0, 0,42], Endianness::BigEndian   ,                  TagValue::Rational   (42, 42             )    ),
        (vec![1,1, 10,0, 1,0,0,0, 12, 0, 0, 0, 42, 0, 0, 0,42, 0, 0, 0], Endianness::LittleEndian,                  TagValue::SRational  (42, 42             )    ),
        (vec![1,1, 0,10, 0,0,0,1,  0, 0, 0,12,  0, 0, 0,42, 0, 0, 0,42], Endianness::BigEndian   ,                  TagValue::SRational  (42, 42             )    ),
        ];
        for (buf, byte_order, res) in cases {
            println!("reading {buf:?} to be {res:?}");
            let fetch = Bytes::from_owner(buf);
            assert_eq!(
                read_tag(&fetch, 0, byte_order, false).await.unwrap(),
                (Tag::from_u16_exhaustive(0x0101), res)
            )
        }
    }

    #[tokio::test]
    #[rustfmt::skip]
    async fn test_notfits_big() {
        // personal sanity checks
        assert_eq!(u16::from_le_bytes([42,0]),42);
        assert_eq!(u16::from_be_bytes([0,42]),42);
        assert_eq!(f32::from_le_bytes([0x42,0,0,0]),f32::from_bits(0x00_00_00_42));
        assert_eq!(f32::from_be_bytes([0,0,0,0x42]),f32::from_bits(0x00_00_00_42));
        let cases = [
        //           type       count            offset
        //           / \  1 2 3 4 5 6 7 8   1  2  3  4  5  6  7  8
        (vec![1,1,  1, 0, 9,0,0,0,0,0,0,0, 20, 0, 0, 0, 0, 0, 0, 0, 42,42,42,42,42,42,42,42,42],                      Endianness::LittleEndian, TagValue::List(vec![TagValue::Byte       (42                );9])),
        (vec![1,1,  0, 1, 0,0,0,0,0,0,0,9,  0, 0, 0, 0, 0, 0, 0,20, 42,42,42,42,42,42,42,42,42],                      Endianness::BigEndian   , TagValue::List(vec![TagValue::Byte       (42                );9])),
        (vec![1,1,  6, 0, 9,0,0,0,0,0,0,0, 20, 0, 0, 0, 0, 0, 0, 0, 42,42,42,42,42,42,42,42,42],                      Endianness::LittleEndian, TagValue::List(vec![TagValue::SignedByte (42                );9])),
        (vec![1,1,  0, 6, 0,0,0,0,0,0,0,9,  0, 0, 0, 0, 0, 0, 0,20, 42,42,42,42,42,42,42,42,42],                      Endianness::BigEndian   , TagValue::List(vec![TagValue::SignedByte (42                );9])),
        (vec![1,1,  7, 0, 9,0,0,0,0,0,0,0, 20, 0, 0, 0, 0, 0, 0, 0, 42,42,42,42,42,42,42,42,42],                      Endianness::LittleEndian, TagValue::List(vec![TagValue::Byte       (42                );9])), //TagType::UNDEFINED ),
        (vec![1,1,  0, 7, 0,0,0,0,0,0,0,9,  0, 0, 0, 0, 0, 0, 0,20, 42,42,42,42,42,42,42,42,42],                      Endianness::BigEndian   , TagValue::List(vec![TagValue::Byte       (42                );9])), //TagType::UNDEFINED ),
        (vec![1,1,  2, 0, 9,0,0,0,0,0,0,0, 20, 0, 0, 0, 0, 0, 0, 0, 42,42,42,42,42,42,42,42, 0],                      Endianness::LittleEndian,                  TagValue::Ascii      ("********".into() )    ),
        (vec![1,1,  0, 2, 0,0,0,0,0,0,0,9,  0, 0, 0, 0, 0, 0, 0,20, 42,42,42,42,42,42,42,42, 0],                      Endianness::BigEndian   ,                  TagValue::Ascii      ("********".into() )    ),
        (vec![1,1,  3, 0, 5,0,0,0,0,0,0,0, 20, 0, 0, 0, 0, 0, 0, 0, 42, 0,42, 0,42, 0,42, 0,42, 0],                   Endianness::LittleEndian, TagValue::List(vec![TagValue::Short      (42                );5])),
        (vec![1,1,  0, 3, 0,0,0,0,0,0,0,5,  0, 0, 0, 0, 0, 0, 0,20,  0,42, 0,42, 0,42, 0,42, 0,42],                   Endianness::BigEndian   , TagValue::List(vec![TagValue::Short      (42                );5])),
        (vec![1,1,  8, 0, 5,0,0,0,0,0,0,0, 20, 0, 0, 0, 0, 0, 0, 0, 42, 0,42, 0,42, 0,42, 0,42, 0],                   Endianness::LittleEndian, TagValue::List(vec![TagValue::SignedShort(42                );5])),
        (vec![1,1,  0, 8, 0,0,0,0,0,0,0,5,  0, 0, 0, 0, 0, 0, 0,20,  0,42, 0,42, 0,42, 0,42, 0,42],                   Endianness::BigEndian   , TagValue::List(vec![TagValue::SignedShort(42                );5])),
        (vec![1,1,  4, 0, 3,0,0,0,0,0,0,0, 20, 0, 0, 0, 0, 0, 0, 0, 42, 0, 0, 0,42, 0, 0, 0,42, 0, 0, 0],             Endianness::LittleEndian, TagValue::List(vec![TagValue::Unsigned   (42                );3])),
        (vec![1,1,  0, 4, 0,0,0,0,0,0,0,3,  0, 0, 0, 0, 0, 0, 0,20,  0, 0, 0,42, 0, 0, 0,42, 0, 0, 0,42],             Endianness::BigEndian   , TagValue::List(vec![TagValue::Unsigned   (42                );3])),
        (vec![1,1,  9, 0, 3,0,0,0,0,0,0,0, 20, 0, 0, 0, 0, 0, 0, 0, 42, 0, 0, 0,42, 0, 0, 0,42, 0, 0, 0],             Endianness::LittleEndian, TagValue::List(vec![TagValue::Signed     (42                );3])),
        (vec![1,1,  0, 9, 0,0,0,0,0,0,0,3,  0, 0, 0, 0, 0, 0, 0,20,  0, 0, 0,42, 0, 0, 0,42, 0, 0, 0,42],             Endianness::BigEndian   , TagValue::List(vec![TagValue::Signed     (42                );3])),
        (vec![1,1, 13, 0, 3,0,0,0,0,0,0,0, 20, 0, 0, 0, 0, 0, 0, 0, 42, 0, 0, 0,42, 0, 0, 0,42, 0, 0, 0],             Endianness::LittleEndian, TagValue::List(vec![TagValue::Ifd        (42                );3])),
        (vec![1,1,  0,13, 0,0,0,0,0,0,0,3,  0, 0, 0, 0, 0, 0, 0,20,  0, 0, 0,42, 0, 0, 0,42, 0, 0, 0,42],             Endianness::BigEndian   , TagValue::List(vec![TagValue::Ifd        (42                );3])),
        (vec![1,1, 16, 0, 2,0,0,0,0,0,0,0, 20, 0, 0, 0, 0, 0, 0, 0, 42, 0, 0, 0, 0, 0, 0, 0,42, 0, 0, 0, 0, 0, 0, 0], Endianness::LittleEndian, TagValue::List(vec![TagValue::UnsignedBig(42                );2])),
        (vec![1,1,  0,16, 0,0,0,0,0,0,0,2,  0, 0, 0, 0, 0, 0, 0,20,  0, 0, 0, 0, 0, 0, 0,42, 0, 0, 0, 0, 0, 0, 0,42], Endianness::BigEndian   , TagValue::List(vec![TagValue::UnsignedBig(42                );2])),
        (vec![1,1, 17, 0, 2,0,0,0,0,0,0,0, 20, 0, 0, 0, 0, 0, 0, 0, 42, 0, 0, 0, 0, 0, 0, 0,42, 0, 0, 0, 0, 0, 0, 0], Endianness::LittleEndian, TagValue::List(vec![TagValue::SignedBig  (42                );2])),
        (vec![1,1,  0,17, 0,0,0,0,0,0,0,2,  0, 0, 0, 0, 0, 0, 0,20,  0, 0, 0, 0, 0, 0, 0,42, 0, 0, 0, 0, 0, 0, 0,42], Endianness::BigEndian   , TagValue::List(vec![TagValue::SignedBig  (42                );2])),
        (vec![1,1, 18, 0, 2,0,0,0,0,0,0,0, 20, 0, 0, 0, 0, 0, 0, 0, 42, 0, 0, 0, 0, 0, 0, 0,42, 0, 0, 0, 0, 0, 0, 0], Endianness::LittleEndian, TagValue::List(vec![TagValue::IfdBig     (42                );2])),
        (vec![1,1,  0,18, 0,0,0,0,0,0,0,2,  0, 0, 0, 0, 0, 0, 0,20,  0, 0, 0, 0, 0, 0, 0,42, 0, 0, 0, 0, 0, 0, 0,42], Endianness::BigEndian   , TagValue::List(vec![TagValue::IfdBig     (42                );2])),
        (vec![1,1, 11, 0, 3,0,0,0,0,0,0,0, 20, 0, 0, 0, 0, 0, 0, 0, 42, 0, 0, 0,42, 0, 0, 0,42, 0, 0, 0,],            Endianness::LittleEndian, TagValue::List(vec![TagValue::Float      (f32::from_bits(42));3])),
        (vec![1,1,  0,11, 0,0,0,0,0,0,0,3,  0, 0, 0, 0, 0, 0, 0,20,  0, 0, 0,42, 0, 0, 0,42, 0, 0, 0,42,],            Endianness::BigEndian   , TagValue::List(vec![TagValue::Float      (f32::from_bits(42));3])),
        (vec![1,1, 12, 0, 2,0,0,0,0,0,0,0, 20, 0, 0, 0, 0, 0, 0, 0, 42, 0, 0, 0, 0, 0, 0, 0,42, 0, 0, 0, 0, 0, 0, 0], Endianness::LittleEndian, TagValue::List(vec![TagValue::Double     (f64::from_bits(42));2])),
        (vec![1,1,  0,12, 0,0,0,0,0,0,0,2,  0, 0, 0, 0, 0, 0, 0,20,  0, 0, 0, 0, 0, 0, 0,42, 0, 0, 0, 0, 0, 0, 0,42], Endianness::BigEndian   , TagValue::List(vec![TagValue::Double     (f64::from_bits(42));2])),
        (vec![1,1,  5, 0, 2,0,0,0,0,0,0,0, 20, 0, 0, 0, 0, 0, 0, 0, 42, 0, 0, 0,42, 0, 0, 0,42, 0, 0, 0,42, 0, 0, 0], Endianness::LittleEndian, TagValue::List(vec![TagValue::Rational   (42, 42            );2])),
        (vec![1,1,  0, 5, 0,0,0,0,0,0,0,2,  0, 0, 0, 0, 0, 0, 0,20,  0, 0, 0,42, 0, 0, 0,42, 0, 0, 0,42, 0, 0, 0,42], Endianness::BigEndian   , TagValue::List(vec![TagValue::Rational   (42, 42            );2])),
        (vec![1,1, 10, 0, 2,0,0,0,0,0,0,0, 20, 0, 0, 0, 0, 0, 0, 0, 42, 0, 0, 0,42, 0, 0, 0,42, 0, 0, 0,42, 0, 0, 0], Endianness::LittleEndian, TagValue::List(vec![TagValue::SRational  (42, 42            );2])),
        (vec![1,1,  0,10, 0,0,0,0,0,0,0,2,  0, 0, 0, 0, 0, 0, 0,20,  0, 0, 0,42, 0, 0, 0,42, 0, 0, 0,42, 0, 0, 0,42], Endianness::BigEndian   , TagValue::List(vec![TagValue::SRational  (42, 42            );2])),
        ];
        for (buf, byte_order, res) in cases {
            println!("reading {buf:?} to be {res:?}");
            let fetch = Bytes::from_owner(buf);
            assert_eq!(read_tag(&fetch, 0, byte_order, true).await.unwrap(), (Tag::from_u16_exhaustive(0x0101), res))
        }
    }
}
