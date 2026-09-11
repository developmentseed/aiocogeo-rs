use crate::test::util::open_tiff;
use crate::TypedArray;

#[tokio::test]
async fn test_sparse_tile_fills_with_nodata() {
    let filename = "geotiff-test-data/rasterio_generated/fixtures/uint8_1band_sparse_nodata.tif";
    let (reader, tiff) = open_tiff(filename).await;
    let ifd = &tiff.ifds()[0];
    assert_eq!(ifd.image_height(), 512);
    assert_eq!(ifd.image_width(), 512);
    assert_eq!(ifd.tile_width(), Some(256));
    assert_eq!(ifd.tile_height(), Some(256));

    // Tile (0, 0) was actually written by GDAL.
    let tile = ifd.fetch_tile(0, 0, &reader).await.unwrap();
    let array = tile.decode(&Default::default()).unwrap();
    assert_eq!(array.shape(), [256, 256, 1]);

    // Tile (1, 0) was never written -- GDAL left it sparse. This should decode to an array
    // filled with the fixture's GDAL_NODATA value (7), not error.
    let tile = ifd.fetch_tile(1, 0, &reader).await.unwrap();
    let array = tile.decode(&Default::default()).unwrap();
    match array.data() {
        TypedArray::UInt8(v) => assert!(v.iter().all(|&b| b == 7)),
        other => panic!("expected UInt8, got {other:?}"),
    }
}

#[tokio::test]
async fn test_sparse_tile_defaults_to_zero_without_nodata() {
    let filename = "geotiff-test-data/rasterio_generated/fixtures/uint8_1band_sparse_no_nodata.tif";
    let (reader, tiff) = open_tiff(filename).await;
    let ifd = &tiff.ifds()[0];

    let tile = ifd.fetch_tile(1, 0, &reader).await.unwrap();
    let array = tile.decode(&Default::default()).unwrap();
    match array.data() {
        TypedArray::UInt8(v) => assert!(v.iter().all(|&b| b == 0)),
        other => panic!("expected UInt8, got {other:?}"),
    }
}
