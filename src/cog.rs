use std::sync::Arc;

use bytes::Bytes;
use object_store::path::Path;
use object_store::ObjectStore;

use crate::cursor::{Endianness, ObjectStoreCursor};
use crate::error::Result;
use crate::ifd::ImageFileDirectories;

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
        if magic_bytes == Bytes::from_static(b"II") {
            cursor.set_endianness(Endianness::LittleEndian);
        } else if magic_bytes == Bytes::from_static(b"MM") {
            cursor.set_endianness(Endianness::BigEndian);
        } else {
            panic!("unexpected magic bytes {magic_bytes:?}");
        }

        let version = cursor.read_u16().await;
        dbg!(version);

        // Assert it's a standard non-big tiff
        assert_eq!(version, 42);

        let first_ifd_location = cursor.read_u32().await;
        dbg!(first_ifd_location);

        let ifds = ImageFileDirectories::open(&mut cursor, first_ifd_location as usize).await;

        let (store, path) = cursor.into_inner();
        Ok(Self { store, path, ifds })
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use object_store::local::LocalFileSystem;

    #[tokio::test]
    async fn tmp() {
        let folder = "/Users/kyle/github/developmentseed/aiocogeo-rs/";
        let path = Path::parse("m_4007307_sw_18_060_20220803.tif").unwrap();
        let store = Arc::new(LocalFileSystem::new_with_prefix(folder).unwrap());
        let _reader = COGReader::try_open(store, path).await.unwrap();
    }
}
