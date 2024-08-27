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

        // Assert it's a standard non-big tiff
        assert_eq!(version, 42);

        let first_ifd_location = cursor.read_u32().await;

        let ifds = ImageFileDirectories::open(&mut cursor, first_ifd_location as usize)
            .await
            .unwrap();

        let (store, path) = cursor.into_inner();
        Ok(Self { store, path, ifds })
    }

    /// Return the EPSG code representing the crs of the image
    pub fn epsg(&self) -> Option<u16> {
        let ifd = &self.ifds.as_ref()[0];
        ifd.geo_key_directory
            .as_ref()
            .and_then(|gkd| gkd.epsg_code())
    }

    /// Return the bounds of the image in native crs
    pub fn native_bounds(&self) -> Option<(f64, f64, f64, f64)> {
        let ifd = &self.ifds.as_ref()[0];
        ifd.native_bounds()
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
        let reader = COGReader::try_open(store.clone(), path.clone())
            .await
            .unwrap();
        let cursor = ObjectStoreCursor::new(store.clone(), path.clone());
        let ifd = &reader.ifds.as_ref()[0];
        let tile = ifd.get_tile(0, 0, &cursor).await.unwrap();
        dbg!(tile.len());
    }
}
