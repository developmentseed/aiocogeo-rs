//! Pyramids whose reduced resolutions are SubIFDs (tag 330), not top-level IFDs.

use crate::metadata::TiffMetadataReader;
use crate::tags::Tag;
use crate::test::util::open_tiff;
use crate::TagValue;

const SUBIFDS: Tag = Tag::Unknown(330);

/// Tag 330 holds one offset per reduced level, as plain integers.
fn subifd_offsets(value: &TagValue) -> Vec<u64> {
    let as_offset = |v: &TagValue| match v {
        TagValue::Ifd(offset) => *offset as u64,
        TagValue::IfdBig(offset) => *offset,
        other => panic!("not an IFD offset: {other:?}"),
    };
    match value {
        TagValue::List(values) => values.iter().map(as_offset).collect(),
        single => vec![as_offset(single)],
    }
}

async fn assert_pyramid(filename: &str) {
    let (reader, tiff) = open_tiff(filename).await;

    // The reduced levels are not in the top-level chain.
    assert_eq!(tiff.ifds().len(), 1);
    let full = &tiff.ifds()[0];
    assert_eq!(full.image_height(), 256);
    assert_eq!(full.image_width(), 256);
    assert_eq!(full.tile_height(), Some(64));

    let offsets = subifd_offsets(&full.other_tags()[&SUBIFDS]);
    assert_eq!(offsets.len(), 2);

    // The offsets point at real IFDs: one per level, each half the size of the previous level.
    let metadata_reader = TiffMetadataReader::try_open(&reader).await.unwrap();
    for (offset, expected_size) in offsets.into_iter().zip([128, 64]) {
        let level = metadata_reader.read_ifd_at(&reader, offset).await.unwrap();
        assert_eq!(level.image_height(), expected_size);
        assert_eq!(level.image_width(), expected_size);

        let tile = level.fetch_tile(0, 0, &reader).await.unwrap();
        let array = tile.decode(&Default::default()).unwrap();
        assert_eq!(array.shape, [64, 64, 1]);
    }
}

/// In a classic TIFF the tag is typed IFD.
#[tokio::test]
async fn test_classic() {
    assert_pyramid("geotiff-test-data/tifffile_generated/fixtures/subifd_pyramid_classic.tif")
        .await;
}

/// In a BigTIFF the tag is typed IFD8.
#[tokio::test]
async fn test_bigtiff() {
    assert_pyramid("geotiff-test-data/tifffile_generated/fixtures/subifd_pyramid_bigtiff.tif")
        .await;
}
