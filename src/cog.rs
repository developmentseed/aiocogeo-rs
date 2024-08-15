use std::collections::HashMap;
use std::io::Cursor;
use std::sync::Arc;

use byteorder::{LittleEndian, ReadBytesExt};
use bytes::Bytes;
use object_store::path::Path;
use object_store::ObjectStore;
use tiff::decoder::ifd::{Directory, Entry, Value};
use tiff::tags::Tag;

use crate::error::Result;

// TODO: in the future add buffering to this
struct ObjectStoreCursor {
    store: Arc<dyn ObjectStore>,
    path: Path,
    offset: usize,
}

impl ObjectStoreCursor {
    fn new(store: Arc<dyn ObjectStore>, path: Path) -> Self {
        Self {
            store,
            path,
            offset: 0,
        }
    }

    fn into_inner(self) -> (Arc<dyn ObjectStore>, Path) {
        (self.store, self.path)
    }

    async fn read(&mut self, length: usize) -> Bytes {
        let range = self.offset..self.offset + length;
        self.offset += length;
        self.store.get_range(&self.path, range).await.unwrap()
    }

    fn seek(&mut self, offset: usize) {
        self.offset = offset;
    }

    fn tell(&self) -> usize {
        self.offset
    }
}

pub struct COGReader {
    store: Arc<dyn ObjectStore>,
    path: Path,
    ifds: ImageFileDirectories,
}

impl COGReader {
    pub async fn try_open(store: Arc<dyn ObjectStore>, path: Path) -> Result<Self> {
        let mut cursor = ObjectStoreCursor::new(store, path);
        let magic_bytes = cursor.read(2).await;
        // Should be b"II" for little endian or b"MM" for big endian
        // For now we assert it's little endian
        assert_eq!(magic_bytes, Bytes::from_static(b"II"));
        dbg!(magic_bytes);

        let version_bytes = cursor.read(2).await;
        let version = Cursor::new(version_bytes)
            .read_i16::<LittleEndian>()
            .unwrap();
        dbg!(version);

        // Assert it's a standard non-big tiff
        assert_eq!(version, 42);

        // TODO: check in the spec whether these offsets are i32 or u32
        let first_ifd_location = cursor.read(4).await;
        let first_ifd_location = Cursor::new(first_ifd_location)
            .read_i32::<LittleEndian>()
            .unwrap();
        dbg!(first_ifd_location);

        let ifds = ImageFileDirectories::open(&mut cursor, first_ifd_location as usize).await;

        let (store, path) = cursor.into_inner();
        Ok(Self { store, path, ifds })
    }
}

/// A collection of all the IFD
// TODO: maybe separate out the primary/first image IFD out of the vec, as that one should have
// geospatial metadata?
struct ImageFileDirectories {
    /// There's always at least one IFD in a TIFF. We store this separately
    image_ifds: Vec<ImageIFD>,

    // Is it guaranteed that if masks exist that there will be one per image IFD? Or could there be
    // different numbers of image ifds and mask ifds?
    mask_ifds: Option<Vec<MaskIFD>>,
}

impl ImageFileDirectories {
    async fn open(cursor: &mut ObjectStoreCursor, ifd_offset: usize) -> Self {
        let mut next_ifd_offset = Some(ifd_offset);

        let mut image_ifds = vec![];
        let mut mask_ifds = vec![];
        while let Some(offset) = next_ifd_offset {
            let ifd = ImageFileDirectory::read(cursor, offset).await;
            next_ifd_offset = ifd.next_ifd_offset();
            match ifd {
                ImageFileDirectory::Image(image_ifd) => image_ifds.push(image_ifd),
                ImageFileDirectory::Mask(mask_ifd) => mask_ifds.push(mask_ifd),
            }
        }

        Self {
            image_ifds,
            // TODO: if empty, return None
            mask_ifds: Some(mask_ifds),
        }
    }
}

struct RequiredTags {
    bits_per_sample: Value,
    compression: Value,
    image_height: Value,
    image_width: Value,
    photometric_interpretation: Value,
    planar_configuration: Value,
    sample_format: Value,
    samples_per_pixel: Value,
    tile_byte_counts: Value,
    tile_height: Value,
    tile_offsets: Value,
    tile_width: Value,
}

impl RequiredTags {
    fn bands(&self) -> usize {
        self.samples_per_pixel.into_u32()
    }
}

struct OptionalTags {
    directory: Directory,
}

impl OptionalTags {
    /// Check if the IFD contains a full resolution image (not an overview)
    fn is_full_resolution(&self) -> bool {
        if let Some(val) = self.directory.get(&Tag::NewSubfileType) {
            // if self.NewSubfileType.value[0] == 0:
            todo!()
        } else {
            true
        }
        // self.directory.contains_key(T)
    }
}

/// An ImageFileDirectory representing Image content
struct ImageIFD {
    directory: Directory,
    next_ifd_offset: Option<usize>,
}

/// An ImageFileDirectory representing Mask content
struct MaskIFD {
    next_ifd_offset: Option<usize>,
}

enum ImageFileDirectory {
    Image(ImageIFD),
    Mask(MaskIFD),
}

impl ImageFileDirectory {
    async fn read(cursor: &mut ObjectStoreCursor, offset: usize) -> Self {
        let ifd_start = offset;
        cursor.seek(offset);

        let tag_count = cursor.read(2).await;
        let tag_count = Cursor::new(tag_count).read_i16::<LittleEndian>().unwrap();

        // let mut tags = HashMap::with_capacity(tag_count);
        for i in 0..tag_count {
            // todo: read tag
        }

        cursor.seek(ifd_start + (12 * tag_count as usize) + 2);
        let next_ifd_offset = cursor.read(4).await;
        let next_ifd_offset = Cursor::new(next_ifd_offset)
            .read_i32::<LittleEndian>()
            .unwrap() as usize;
        let next_ifd_offset = if next_ifd_offset == 0 {
            None
        } else {
            Some(next_ifd_offset)
        };

        if is_masked_ifd() {
            Self::Mask(MaskIFD { next_ifd_offset })
        } else {
            Self::Image(ImageIFD { next_ifd_offset })
        }
    }

    fn next_ifd_offset(&self) -> Option<usize> {
        match self {
            Self::Image(ifd) => ifd.next_ifd_offset,
            Self::Mask(ifd) => ifd.next_ifd_offset,
        }
    }
}

fn is_masked_ifd() -> bool {
    todo!()
    // https://github.com/geospatial-jeff/aiocogeo/blob/5a1d32c3f22c883354804168a87abb0a2ea1c328/aiocogeo/ifd.py#L66
}

#[cfg(test)]
mod test {
    use super::*;
    use object_store::local::LocalFileSystem;
    use object_store::ObjectStore;
    use tokio::fs::File;

    #[tokio::test]
    async fn tmp() {
        let folder = "/Users/kyle/github/developmentseed/aiocogeo-rs/";
        let path = Path::parse("m_4007307_sw_18_060_20220803.tif").unwrap();
        let store = Arc::new(LocalFileSystem::new_with_prefix(folder).unwrap());
        let reader = COGReader::try_open(store, path).await.unwrap();
    }
}
