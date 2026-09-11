use crate::test::util::open_tiff;
use crate::tags::PhotometricInterpretation;

/// Regression test for TIFFs that omit the `SamplesPerPixel` tag (277).
///
/// The TIFF 6.0 specification defines a default of 1 for `SamplesPerPixel`, and
/// some single-channel grayscale TIFFs (e.g. PerkinElmer/Phenix microscopy
/// images) omit the tag entirely. Parsing such files used to panic with
/// "samples_per_pixel not found"; it must instead fall back to the spec default.
#[tokio::test]
async fn test_missing_samples_per_pixel_defaults_to_one() {
    let filename = "other/grayscale_no_samplesperpixel.tif";
    let (_reader, tiff) = open_tiff(filename).await;
    let ifd = &tiff.ifds()[0];

    assert_eq!(ifd.samples_per_pixel(), 1);
    assert_eq!(ifd.image_width(), 2);
    assert_eq!(ifd.image_height(), 2);
    assert_eq!(
        ifd.photometric_interpretation(),
        PhotometricInterpretation::BlackIsZero
    );
}
