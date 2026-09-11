//! A file whose first IFD is at the end, as streaming writers produce.

use std::path::PathBuf;
use std::sync::Arc;

use object_store::local::LocalFileSystem;

use crate::metadata::cache::ReadaheadMetadataCache;
use crate::metadata::TiffMetadataReader;
use crate::reader::{AsyncFileReader, ObjectReader};

const FILENAME: &str = "geotiff-test-data/tifffile_generated/fixtures/trailing_directory.tif";

#[tokio::test]
async fn test_trailing_directory() {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let store = Arc::new(LocalFileSystem::new_with_prefix(&manifest_dir).unwrap());
    let path = format!("fixtures/{FILENAME}");
    let reader =
        Arc::new(ObjectReader::new(store, path.as_str().into())) as Arc<dyn AsyncFileReader>;

    // The directory sits past the default 32 KiB window, so the read uses the window fetch
    // path rather than the sequential growth path.
    let cache = ReadaheadMetadataCache::new(reader.clone());
    let mut metadata_reader = TiffMetadataReader::try_open(&cache).await.unwrap();
    let tiff = metadata_reader.read(&cache).await.unwrap();

    assert_eq!(tiff.ifds().len(), 1);
    let ifd = &tiff.ifds()[0];
    assert_eq!(ifd.image_height(), 256);
    assert_eq!(ifd.image_width(), 256);
    assert_eq!(ifd.tile_height(), Some(64));

    let tile = ifd.fetch_tile(0, 0, &reader).await.unwrap();
    let array = tile.decode(&Default::default()).unwrap();
    assert_eq!(array.shape, [64, 64, 1]);
}
