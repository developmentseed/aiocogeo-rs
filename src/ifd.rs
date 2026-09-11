use std::collections::HashMap;
use std::ops::Range;
use std::sync::Arc;

use bytes::Bytes;
use num_enum::TryFromPrimitive;

use crate::error::{AsyncTiffError, AsyncTiffResult, TiffError, TiffFormatError};
use crate::geo::{GeoKeyDirectory, GeoKeyTag};
use crate::reader::{AsyncFileReader, Endianness};
use crate::tag_value::TagValue;
use crate::tags::{
    Compression, ExtraSamples, PhotometricInterpretation, PlanarConfiguration, Predictor,
    ResolutionUnit, SampleFormat, Tag,
};
use crate::{DataType, Tile};

const DOCUMENT_NAME: u16 = 269;

/// An ImageFileDirectory representing Image content
// The ordering of these tags matches the sorted order in TIFF spec Appendix A
#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq)]
pub struct ImageFileDirectory {
    pub(crate) endianness: Endianness,

    pub(crate) new_subfile_type: Option<u32>,

    /// The number of columns in the image, i.e., the number of pixels per row.
    pub(crate) image_width: u32,

    /// The number of rows of pixels in the image.
    pub(crate) image_height: u32,

    pub(crate) bits_per_sample: Vec<u16>,

    pub(crate) compression: Compression,

    pub(crate) photometric_interpretation: PhotometricInterpretation,

    pub(crate) document_name: Option<String>,

    pub(crate) image_description: Option<String>,

    pub(crate) strip_offsets: Option<Vec<u64>>,

    pub(crate) orientation: Option<u16>,

    /// The number of components per pixel.
    ///
    /// SamplesPerPixel is usually 1 for bilevel, grayscale, and palette-color images.
    /// SamplesPerPixel is usually 3 for RGB images. If this value is higher, ExtraSamples should
    /// give an indication of the meaning of the additional channels.
    pub(crate) samples_per_pixel: u16,

    pub(crate) rows_per_strip: Option<u32>,

    pub(crate) strip_byte_counts: Option<Vec<u64>>,

    pub(crate) min_sample_value: Option<Vec<u16>>,
    pub(crate) max_sample_value: Option<Vec<u16>>,

    /// The number of pixels per ResolutionUnit in the ImageWidth direction.
    pub(crate) x_resolution: Option<f64>,

    /// The number of pixels per ResolutionUnit in the ImageLength direction.
    pub(crate) y_resolution: Option<f64>,

    /// How the components of each pixel are stored.
    ///
    /// The specification defines these values:
    ///
    /// - Chunky format. The component values for each pixel are stored contiguously. For example,
    ///   for RGB data, the data is stored as RGBRGBRGB
    /// - Planar format. The components are stored in separate component planes. For example, RGB
    ///   data is stored with the Red components in one component plane, the Green in another, and
    ///   the Blue in another.
    ///
    /// The specification adds a warning that PlanarConfiguration=2 is not in widespread use and
    /// that Baseline TIFF readers are not required to support it.
    ///
    /// If SamplesPerPixel is 1, PlanarConfiguration is irrelevant, and need not be included.
    pub(crate) planar_configuration: PlanarConfiguration,

    pub(crate) resolution_unit: Option<ResolutionUnit>,

    /// Name and version number of the software package(s) used to create the image.
    pub(crate) software: Option<String>,

    /// Date and time of image creation.
    ///
    /// The format is: "YYYY:MM:DD HH:MM:SS", with hours like those on a 24-hour clock, and one
    /// space character between the date and the time. The length of the string, including the
    /// terminating NUL, is 20 bytes.
    pub(crate) date_time: Option<String>,
    pub(crate) artist: Option<String>,
    pub(crate) host_computer: Option<String>,

    pub(crate) predictor: Option<Predictor>,

    /// A color map for palette color images.
    ///
    /// This field defines a Red-Green-Blue color map (often called a lookup table) for
    /// palette-color images. In a palette-color image, a pixel value is used to index into an RGB
    /// lookup table. For example, a palette-color pixel having a value of 0 would be displayed
    /// according to the 0th Red, Green, Blue triplet.
    ///
    /// In a TIFF ColorMap, all the Red values come first, followed by the Green values, then the
    /// Blue values. The number of values for each color is 2**BitsPerSample. Therefore, the
    /// ColorMap field for an 8-bit palette-color image would have 3 * 256 values. The width of
    /// each value is 16 bits, as implied by the type of SHORT. 0 represents the minimum intensity,
    /// and 65535 represents the maximum intensity. Black is represented by 0,0,0, and white by
    /// 65535, 65535, 65535.
    ///
    /// ColorMap must be included in all palette-color images.
    ///
    /// In Specification Supplement 1, support was added for ColorMaps containing other then RGB
    /// values. This scheme includes the Indexed tag, with value 1, and a PhotometricInterpretation
    /// different from PaletteColor then next denotes the colorspace of the ColorMap entries.
    ///
    /// <https://web.archive.org/web/20240329145324/https://www.awaresystems.be/imaging/tiff/tifftags/colormap.html>
    pub(crate) color_map: Option<Arc<[u16]>>,

    pub(crate) tile_width: Option<u32>,
    pub(crate) tile_height: Option<u32>,

    pub(crate) tile_offsets: Option<Vec<u64>>,
    pub(crate) tile_byte_counts: Option<Vec<u64>>,

    pub(crate) extra_samples: Option<Vec<ExtraSamples>>,

    pub(crate) sample_format: Vec<SampleFormat>,

    pub(crate) jpeg_tables: Option<Bytes>,

    pub(crate) copyright: Option<String>,

    // Geospatial tags
    pub(crate) geo_key_directory: Option<GeoKeyDirectory>,
    pub(crate) model_pixel_scale: Option<Vec<f64>>,
    pub(crate) model_tiepoint: Option<Vec<f64>>,
    pub(crate) model_transformation: Option<Vec<f64>>,

    // GDAL tags
    pub(crate) gdal_nodata: Option<String>,
    pub(crate) gdal_metadata: Option<String>,
    pub(crate) other_tags: HashMap<Tag, TagValue>,

    // Other
    pub(crate) lerc_parameters: Option<Vec<u32>>,
}

impl ImageFileDirectory {
    /// Create a new ImageFileDirectory from tag data
    pub fn from_tags(
        tag_data: HashMap<Tag, TagValue>,
        endianness: Endianness,
    ) -> AsyncTiffResult<Self> {
        let mut new_subfile_type = None;
        let mut image_width = None;
        let mut image_height = None;
        let mut bits_per_sample = None;
        let mut compression = None;
        let mut photometric_interpretation = None;
        let mut document_name = None;
        let mut image_description = None;
        let mut strip_offsets = None;
        let mut orientation = None;
        let mut samples_per_pixel = None;
        let mut rows_per_strip = None;
        let mut strip_byte_counts = None;
        let mut min_sample_value = None;
        let mut max_sample_value = None;
        let mut x_resolution = None;
        let mut y_resolution = None;
        let mut planar_configuration = None;
        let mut resolution_unit = None;
        let mut software = None;
        let mut date_time = None;
        let mut artist = None;
        let mut host_computer = None;
        let mut predictor = None;
        let mut color_map = None;
        let mut tile_width = None;
        let mut tile_height = None;
        let mut tile_offsets = None;
        let mut tile_byte_counts = None;
        let mut extra_samples = None;
        let mut sample_format = None;
        let mut jpeg_tables = None;
        let mut copyright = None;
        let mut geo_key_directory_data = None;
        let mut model_pixel_scale = None;
        let mut model_tiepoint = None;
        let mut model_transformation = None;
        let mut geo_ascii_params: Option<String> = None;
        let mut geo_double_params: Option<Vec<f64>> = None;
        let mut gdal_nodata = None;
        let mut gdal_metadata = None;
        let mut lerc_parameters = None;

        let mut other_tags = HashMap::new();

        tag_data.into_iter().try_for_each(|(tag, value)| {
            match tag {
                Tag::NewSubfileType => new_subfile_type = Some(value.into_u32()?),
                Tag::ImageWidth => image_width = Some(value.into_u32()?),
                Tag::ImageLength => image_height = Some(value.into_u32()?),
                Tag::BitsPerSample => bits_per_sample = Some(value.into_u16_vec()?),
                Tag::Compression => {
                    compression = Some(Compression::from_u16_exhaustive(value.into_u16()?))
                }
                Tag::PhotometricInterpretation => {
                    photometric_interpretation =
                        PhotometricInterpretation::from_u16(value.into_u16()?)
                }
                Tag::ImageDescription => image_description = Some(value.into_string()?),
                Tag::StripOffsets => strip_offsets = Some(value.into_u64_vec()?),
                Tag::Orientation => orientation = Some(value.into_u16()?),
                Tag::SamplesPerPixel => samples_per_pixel = Some(value.into_u16()?),
                Tag::RowsPerStrip => rows_per_strip = Some(value.into_u32()?),
                Tag::StripByteCounts => strip_byte_counts = Some(value.into_u64_vec()?),
                Tag::MinSampleValue => min_sample_value = Some(value.into_u16_vec()?),
                Tag::MaxSampleValue => max_sample_value = Some(value.into_u16_vec()?),
                Tag::XResolution => match value {
                    TagValue::Rational(n, d) => x_resolution = Some(n as f64 / d as f64),
                    _ => unreachable!("Expected rational type for XResolution."),
                },
                Tag::YResolution => match value {
                    TagValue::Rational(n, d) => y_resolution = Some(n as f64 / d as f64),
                    _ => unreachable!("Expected rational type for YResolution."),
                },
                Tag::PlanarConfiguration => {
                    planar_configuration = PlanarConfiguration::from_u16(value.into_u16()?)
                }
                Tag::ResolutionUnit => {
                    resolution_unit = ResolutionUnit::from_u16(value.into_u16()?)
                }
                Tag::Software => software = Some(value.into_string()?),
                Tag::DateTime => date_time = Some(value.into_string()?),
                Tag::Artist => artist = Some(value.into_string()?),
                Tag::HostComputer => host_computer = Some(value.into_string()?),
                Tag::Predictor => predictor = Predictor::from_u16(value.into_u16()?),
                Tag::ColorMap => color_map = Some(Arc::from(value.into_u16_vec()?)),
                Tag::TileWidth => tile_width = Some(value.into_u32()?),
                Tag::TileLength => tile_height = Some(value.into_u32()?),
                Tag::TileOffsets => tile_offsets = Some(value.into_u64_vec()?),
                Tag::TileByteCounts => tile_byte_counts = Some(value.into_u64_vec()?),
                Tag::ExtraSamples => {
                    let values = value.into_u16_vec()?;
                    extra_samples = Some(
                        values
                            .into_iter()
                            .map(|val| {
                                ExtraSamples::from_u16(val)
                                    .ok_or(TiffError::FormatError(TiffFormatError::InvalidTag))
                            })
                            .collect::<Result<Vec<_>, _>>()?,
                    );
                }
                Tag::SampleFormat => {
                    let values = value.into_u16_vec()?;
                    sample_format = Some(
                        values
                            .into_iter()
                            .map(SampleFormat::from_u16_exhaustive)
                            .collect(),
                    );
                }
                Tag::JPEGTables => jpeg_tables = Some(value.into_u8_vec()?.into()),
                Tag::Copyright => copyright = Some(value.into_string()?),

                // Geospatial tags
                // http://geotiff.maptools.org/spec/geotiff2.4.html
                Tag::GeoKeyDirectory => geo_key_directory_data = Some(value.into_u16_vec()?),
                Tag::ModelPixelScale => model_pixel_scale = Some(value.into_f64_vec()?),
                Tag::ModelTiepoint => model_tiepoint = Some(value.into_f64_vec()?),
                Tag::ModelTransformation => model_transformation = Some(value.into_f64_vec()?),
                Tag::GeoAsciiParams => geo_ascii_params = Some(value.into_string()?),
                Tag::GeoDoubleParams => geo_double_params = Some(value.into_f64_vec()?),
                Tag::GdalNodata => gdal_nodata = Some(value.into_string()?),
                Tag::GdalMetadata => gdal_metadata = Some(value.into_string()?),
                Tag::LercParameters => lerc_parameters = Some(value.into_u32_vec()?),
                // Tags for which the tiff crate doesn't have a hard-coded enum variant
                Tag::Unknown(DOCUMENT_NAME) => document_name = Some(value.into_string()?),
                _ => {
                    other_tags.insert(tag, value);
                }
            };
            Ok::<_, TiffError>(())
        })?;

        let mut geo_key_directory = None;

        // We need to actually parse the GeoKeyDirectory after parsing all other tags because the
        // GeoKeyDirectory relies on `GeoAsciiParamsTag` having been parsed.
        if let Some(data) = geo_key_directory_data {
            let mut chunks = data.chunks(4);

            let header = chunks
                .next()
                .expect("If the geo key directory exists, a header should exist.");
            let key_directory_version = header[0];
            assert_eq!(key_directory_version, 1);

            let key_revision = header[1];
            assert_eq!(key_revision, 1);

            let _key_minor_revision = header[2];
            let number_of_keys = header[3];

            let mut tags = HashMap::with_capacity(number_of_keys as usize);
            for _ in 0..number_of_keys {
                let chunk = chunks
                    .next()
                    .expect("There should be a chunk for each key.");

                let key_id = chunk[0];
                let tag_name = if let Ok(tag_name) = GeoKeyTag::try_from_primitive(key_id) {
                    tag_name
                } else {
                    // Skip unknown GeoKeyTag ids. Some GeoTIFFs include keys that were proposed
                    // but not included in the GeoTIFF spec. See
                    // https://github.com/developmentseed/async-tiff/pull/131 and
                    // https://github.com/virtual-zarr/virtual-tiff/issues/52
                    continue;
                };

                let tag_location = chunk[1];
                let count = chunk[2];
                let value_offset = chunk[3];

                if tag_location == 0 {
                    tags.insert(tag_name, TagValue::Short(value_offset));
                } else if Tag::from_u16_exhaustive(tag_location) == Tag::GeoAsciiParams {
                    // If the tag_location points to the value of Tag::GeoAsciiParams, then we
                    // need to extract a subslice from GeoAsciiParams

                    let geo_ascii_params = geo_ascii_params
                        .as_ref()
                        .expect("GeoAsciiParamsTag exists but geo_ascii_params does not.");
                    let value_offset = value_offset as usize;
                    let mut s = &geo_ascii_params[value_offset..value_offset + count as usize];

                    // It seems that this string subslice might always include the final |
                    // character?
                    if s.ends_with('|') {
                        s = &s[0..s.len() - 1];
                    }

                    tags.insert(tag_name, TagValue::Ascii(s.to_string()));
                } else if Tag::from_u16_exhaustive(tag_location) == Tag::GeoDoubleParams {
                    // If the tag_location points to the value of Tag::GeoDoubleParams, then we
                    // need to extract a subslice from GeoDoubleParams

                    let geo_double_params = geo_double_params
                        .as_ref()
                        .expect("GeoDoubleParamsTag exists but geo_double_params does not.");
                    let value_offset = value_offset as usize;
                    let value = if count == 1 {
                        TagValue::Double(geo_double_params[value_offset])
                    } else {
                        let x = geo_double_params[value_offset..value_offset + count as usize]
                            .iter()
                            .map(|val| TagValue::Double(*val))
                            .collect();
                        TagValue::List(x)
                    };
                    tags.insert(tag_name, value);
                }
            }
            geo_key_directory = Some(GeoKeyDirectory::from_tags(tags)?);
        }

        // SamplesPerPixel (tag 277) defaults to 1 when the tag is absent.
        // TIFF 6.0 spec, "SamplesPerPixel ... Default = 1": https://download.osgeo.org/libtiff/doc/TIFF6.pdf
        // Many single-channel grayscale TIFFs (e.g. PerkinElmer/Phenix microscopy) omit it.
        let samples_per_pixel = samples_per_pixel.unwrap_or(1);
        let planar_configuration = if let Some(planar_configuration) = planar_configuration {
            planar_configuration
        } else if samples_per_pixel == 1 {
            // If SamplesPerPixel is 1, PlanarConfiguration is irrelevant, and need not be included.
            // https://web.archive.org/web/20240329145253/https://www.awaresystems.be/imaging/tiff/tifftags/planarconfiguration.html
            PlanarConfiguration::Chunky
        } else {
            PlanarConfiguration::Chunky
        };
        Ok(Self {
            endianness,
            new_subfile_type,
            image_width: image_width.expect("image_width not found"),
            image_height: image_height.expect("image_height not found"),
            bits_per_sample: bits_per_sample.expect("bits per sample not found"),
            // Defaults to no compression
            // https://web.archive.org/web/20240329145331/https://www.awaresystems.be/imaging/tiff/tifftags/compression.html
            compression: compression.unwrap_or(Compression::None),
            photometric_interpretation: photometric_interpretation
                .expect("photometric interpretation not found"),
            document_name,
            image_description,
            strip_offsets,
            orientation,
            samples_per_pixel,
            rows_per_strip,
            strip_byte_counts,
            min_sample_value,
            max_sample_value,
            x_resolution,
            y_resolution,
            planar_configuration,
            resolution_unit,
            software,
            date_time,
            artist,
            host_computer,
            predictor,
            color_map,
            tile_width,
            tile_height,
            tile_offsets,
            tile_byte_counts,
            extra_samples,
            // Uint8 is the default for SampleFormat
            // https://web.archive.org/web/20240329145340/https://www.awaresystems.be/imaging/tiff/tifftags/sampleformat.html
            sample_format: sample_format
                .unwrap_or(vec![SampleFormat::Uint; samples_per_pixel as _]),
            copyright,
            jpeg_tables,
            geo_key_directory,
            model_pixel_scale,
            model_tiepoint,
            model_transformation,
            gdal_nodata,
            gdal_metadata,
            lerc_parameters,
            other_tags,
        })
    }

    /// A general indication of the kind of data contained in this subfile.
    /// <https://web.archive.org/web/20240329145250/https://www.awaresystems.be/imaging/tiff/tifftags/newsubfiletype.html>
    pub fn new_subfile_type(&self) -> Option<u32> {
        self.new_subfile_type
    }

    /// The number of columns in the image, i.e., the number of pixels per row.
    /// <https://web.archive.org/web/20240329145250/https://www.awaresystems.be/imaging/tiff/tifftags/imagewidth.html>
    pub fn image_width(&self) -> u32 {
        self.image_width
    }

    /// The number of rows of pixels in the image.
    /// <https://web.archive.org/web/20240329145250/https://www.awaresystems.be/imaging/tiff/tifftags/imagelength.html>
    pub fn image_height(&self) -> u32 {
        self.image_height
    }

    /// Number of bits per component.
    /// <https://web.archive.org/web/20240329145250/https://www.awaresystems.be/imaging/tiff/tifftags/bitspersample.html>
    pub fn bits_per_sample(&self) -> &[u16] {
        &self.bits_per_sample
    }

    /// Compression scheme used on the image data.
    /// <https://web.archive.org/web/20240329145250/https://www.awaresystems.be/imaging/tiff/tifftags/compression.html>
    pub fn compression(&self) -> Compression {
        self.compression
    }

    /// The color space of the image data.
    /// <https://web.archive.org/web/20240329145250/https://www.awaresystems.be/imaging/tiff/tifftags/photometricinterpretation.html>
    pub fn photometric_interpretation(&self) -> PhotometricInterpretation {
        self.photometric_interpretation
    }

    /// Document name.
    pub fn document_name(&self) -> Option<&str> {
        self.document_name.as_deref()
    }

    /// A string that describes the subject of the image.
    /// <https://web.archive.org/web/20240329145250/https://www.awaresystems.be/imaging/tiff/tifftags/imagedescription.html>
    pub fn image_description(&self) -> Option<&str> {
        self.image_description.as_deref()
    }

    /// For each strip, the byte offset of that strip.
    /// <https://web.archive.org/web/20240329145250/https://www.awaresystems.be/imaging/tiff/tifftags/stripoffsets.html>
    pub fn strip_offsets(&self) -> Option<&[u64]> {
        self.strip_offsets.as_deref()
    }

    /// The orientation of the image with respect to the rows and columns.
    /// <https://web.archive.org/web/20240329145250/https://www.awaresystems.be/imaging/tiff/tifftags/orientation.html>
    pub fn orientation(&self) -> Option<u16> {
        self.orientation
    }

    /// The number of components per pixel.
    ///
    /// SamplesPerPixel is usually 1 for bilevel, grayscale, and palette-color images.
    /// SamplesPerPixel is usually 3 for RGB images. If this value is higher, ExtraSamples should
    /// give an indication of the meaning of the additional channels.
    pub fn samples_per_pixel(&self) -> u16 {
        self.samples_per_pixel
    }

    /// The number of rows per strip.
    /// <https://web.archive.org/web/20240329145250/https://www.awaresystems.be/imaging/tiff/tifftags/rowsperstrip.html>
    pub fn rows_per_strip(&self) -> Option<u32> {
        self.rows_per_strip
    }

    /// For each strip, the number of bytes in the strip after compression.
    /// <https://web.archive.org/web/20240329145250/https://www.awaresystems.be/imaging/tiff/tifftags/stripbytecounts.html>
    pub fn strip_byte_counts(&self) -> Option<&[u64]> {
        self.strip_byte_counts.as_deref()
    }

    /// The minimum component value used.
    /// <https://web.archive.org/web/20240329145250/https://www.awaresystems.be/imaging/tiff/tifftags/minsamplevalue.html>
    pub fn min_sample_value(&self) -> Option<&[u16]> {
        self.min_sample_value.as_deref()
    }

    /// The maximum component value used.
    /// <https://web.archive.org/web/20240329145250/https://www.awaresystems.be/imaging/tiff/tifftags/maxsamplevalue.html>
    pub fn max_sample_value(&self) -> Option<&[u16]> {
        self.max_sample_value.as_deref()
    }

    /// The number of pixels per ResolutionUnit in the ImageWidth direction.
    /// <https://web.archive.org/web/20240329145250/https://www.awaresystems.be/imaging/tiff/tifftags/xresolution.html>
    pub fn x_resolution(&self) -> Option<f64> {
        self.x_resolution
    }

    /// The number of pixels per ResolutionUnit in the ImageLength direction.
    /// <https://web.archive.org/web/20240329145250/https://www.awaresystems.be/imaging/tiff/tifftags/yresolution.html>
    pub fn y_resolution(&self) -> Option<f64> {
        self.y_resolution
    }

    /// How the components of each pixel are stored.
    ///
    /// The specification defines these values:
    ///
    /// - Chunky format. The component values for each pixel are stored contiguously. For example,
    ///   for RGB data, the data is stored as RGBRGBRGB
    /// - Planar format. The components are stored in separate component planes. For example, RGB
    ///   data is stored with the Red components in one component plane, the Green in another, and
    ///   the Blue in another.
    ///
    /// The specification adds a warning that PlanarConfiguration=2 is not in widespread use and
    /// that Baseline TIFF readers are not required to support it.
    ///
    /// If SamplesPerPixel is 1, PlanarConfiguration is irrelevant, and need not be included.
    ///
    /// <https://web.archive.org/web/20240329145250/https://www.awaresystems.be/imaging/tiff/tifftags/planarconfiguration.html>
    pub fn planar_configuration(&self) -> PlanarConfiguration {
        self.planar_configuration
    }

    /// The unit of measurement for XResolution and YResolution.
    /// <https://web.archive.org/web/20240329145250/https://www.awaresystems.be/imaging/tiff/tifftags/resolutionunit.html>
    pub fn resolution_unit(&self) -> Option<ResolutionUnit> {
        self.resolution_unit
    }

    /// Name and version number of the software package(s) used to create the image.
    /// <https://web.archive.org/web/20240329145250/https://www.awaresystems.be/imaging/tiff/tifftags/software.html>
    pub fn software(&self) -> Option<&str> {
        self.software.as_deref()
    }

    /// Date and time of image creation.
    ///
    /// The format is: "YYYY:MM:DD HH:MM:SS", with hours like those on a 24-hour clock, and one
    /// space character between the date and the time. The length of the string, including the
    /// terminating NUL, is 20 bytes.
    ///
    /// <https://web.archive.org/web/20240329145250/https://www.awaresystems.be/imaging/tiff/tifftags/datetime.html>
    pub fn date_time(&self) -> Option<&str> {
        self.date_time.as_deref()
    }

    /// Person who created the image.
    /// <https://web.archive.org/web/20240329145250/https://www.awaresystems.be/imaging/tiff/tifftags/artist.html>
    pub fn artist(&self) -> Option<&str> {
        self.artist.as_deref()
    }

    /// The computer and/or operating system in use at the time of image creation.
    /// <https://web.archive.org/web/20240329145250/https://www.awaresystems.be/imaging/tiff/tifftags/hostcomputer.html>
    pub fn host_computer(&self) -> Option<&str> {
        self.host_computer.as_deref()
    }

    /// A mathematical operator that is applied to the image data before an encoding scheme is
    /// applied.
    /// <https://web.archive.org/web/20240329145250/https://www.awaresystems.be/imaging/tiff/tifftags/predictor.html>
    pub fn predictor(&self) -> Option<Predictor> {
        self.predictor
    }

    /// The tile width in pixels. This is the number of columns in each tile.
    /// <https://web.archive.org/web/20240329145250/https://www.awaresystems.be/imaging/tiff/tifftags/tilewidth.html>
    pub fn tile_width(&self) -> Option<u32> {
        self.tile_width
    }

    /// The tile length (height) in pixels. This is the number of rows in each tile.
    /// <https://web.archive.org/web/20240329145250/https://www.awaresystems.be/imaging/tiff/tifftags/tilelength.html>
    pub fn tile_height(&self) -> Option<u32> {
        self.tile_height
    }

    /// For each tile, the byte offset of that tile, as compressed and stored on disk.
    /// <https://web.archive.org/web/20240329145250/https://www.awaresystems.be/imaging/tiff/tifftags/tileoffsets.html>
    pub fn tile_offsets(&self) -> Option<&[u64]> {
        self.tile_offsets.as_deref()
    }

    /// For each tile, the number of (compressed) bytes in that tile.
    /// <https://web.archive.org/web/20240329145339/https://www.awaresystems.be/imaging/tiff/tifftags/tilebytecounts.html>
    pub fn tile_byte_counts(&self) -> Option<&[u64]> {
        self.tile_byte_counts.as_deref()
    }

    /// Description of extra components.
    /// <https://web.archive.org/web/20240329145250/https://www.awaresystems.be/imaging/tiff/tifftags/extrasamples.html>
    pub fn extra_samples(&self) -> Option<&[ExtraSamples]> {
        self.extra_samples.as_deref()
    }

    /// Specifies how to interpret each data sample in a pixel.
    /// <https://web.archive.org/web/20240329145250/https://www.awaresystems.be/imaging/tiff/tifftags/sampleformat.html>
    pub fn sample_format(&self) -> &[SampleFormat] {
        &self.sample_format
    }

    /// JPEG quantization and/or Huffman tables.
    /// <https://web.archive.org/web/20240329145250/https://www.awaresystems.be/imaging/tiff/tifftags/jpegtables.html>
    pub fn jpeg_tables(&self) -> Option<&[u8]> {
        self.jpeg_tables.as_deref()
    }

    /// Copyright notice.
    /// <https://web.archive.org/web/20240329145250/https://www.awaresystems.be/imaging/tiff/tifftags/copyright.html>
    pub fn copyright(&self) -> Option<&str> {
        self.copyright.as_deref()
    }

    /// Geospatial tags
    /// <https://web.archive.org/web/20240329145313/https://www.awaresystems.be/imaging/tiff/tifftags/geokeydirectorytag.html>
    pub fn geo_key_directory(&self) -> Option<&GeoKeyDirectory> {
        self.geo_key_directory.as_ref()
    }

    /// Used in interchangeable GeoTIFF files.
    /// <https://web.archive.org/web/20240329145238/https://www.awaresystems.be/imaging/tiff/tifftags/modelpixelscaletag.html>
    pub fn model_pixel_scale(&self) -> Option<&[f64]> {
        self.model_pixel_scale.as_deref()
    }

    /// Used in interchangeable GeoTIFF files.
    /// <https://web.archive.org/web/20240329145303/https://www.awaresystems.be/imaging/tiff/tifftags/modeltiepointtag.html>
    pub fn model_tiepoint(&self) -> Option<&[f64]> {
        self.model_tiepoint.as_deref()
    }

    /// Stores a full 4×4 affine transformation matrix that maps pixel/line coordinates directly
    /// into model (map) coordinates.
    pub fn model_transformation(&self) -> Option<&[f64]> {
        self.model_transformation.as_deref()
    }

    /// GDAL NoData value
    /// <https://gdal.org/en/stable/drivers/raster/gtiff.html#nodata-value>
    pub fn gdal_nodata(&self) -> Option<&str> {
        self.gdal_nodata.as_deref()
    }

    /// GDAL Metadata XML information
    ///
    /// Non standard metadata items are grouped together into a XML string stored in the non
    /// standard `TIFFTAG_GDAL_METADATA` ASCII tag (code `42112`).
    pub fn gdal_metadata(&self) -> Option<&str> {
        self.gdal_metadata.as_deref()
    }

    /// Tags for which this crate doesn't have a hard-coded enum variant.
    pub fn other_tags(&self) -> &HashMap<Tag, TagValue> {
        &self.other_tags
    }

    /// LERC parameters, used in [LERC]-compressed TIFFs.
    ///
    /// [LERC]: https://esri.github.io/lerc/
    pub fn lerc_parameters(&self) -> Option<&[u32]> {
        self.lerc_parameters.as_deref()
    }

    /// A color map for palette color images.
    ///
    /// This field defines a Red-Green-Blue color map (often called a lookup table) for
    /// palette-color images. In a palette-color image, a pixel value is used to index into an RGB
    /// lookup table. For example, a palette-color pixel having a value of 0 would be displayed
    /// according to the 0th Red, Green, Blue triplet.
    ///
    /// In a TIFF ColorMap, all the Red values come first, followed by the Green values, then the
    /// Blue values. The number of values for each color is `2**BitsPerSample`. Therefore, the
    /// ColorMap field for an 8-bit palette-color image would have `3 * 256` values. The width of
    /// each value is 16 bits, as implied by the type of SHORT. 0 represents the minimum intensity,
    /// and 65535 represents the maximum intensity. Black is represented by 0,0,0, and white by
    /// 65535, 65535, 65535.
    ///
    /// ColorMap must be included in all palette-color images.
    ///
    /// <https://web.archive.org/web/20240329145324/https://www.awaresystems.be/imaging/tiff/tifftags/colormap.html>
    pub fn colormap(&self) -> Option<&Arc<[u16]>> {
        self.color_map.as_ref()
    }

    /// Find the byte range(s) for the tile located at `x` column and `y` row.
    pub fn tile_byte_range(&self, x: usize, y: usize) -> Option<TileByteRange> {
        TileByteRange::from_ifd_tile(self, x, y)
    }

    /// Find the byte ranges for the tiles located at `x` column and `y` row.
    pub fn tiles_byte_ranges(&self, xy: &[(usize, usize)]) -> Option<TilesByteRanges> {
        TilesByteRanges::from_ifd_tiles(self, xy)
    }

    /// Fetch the tile located at `x` column and `y` row using the provided reader.
    ///
    /// For planar configuration TIFFs, this automatically fetches all bands for the tile
    /// at position (x, y) and combines them into a single Tile.
    pub async fn fetch_tile(
        &self,
        x: usize,
        y: usize,
        reader: &dyn AsyncFileReader,
    ) -> AsyncTiffResult<Tile> {
        let byte_ranges = self
            .tile_byte_range(x, y)
            .ok_or(AsyncTiffError::General("Not a tiled TIFF".to_string()))?;
        let compressed_bytes = byte_ranges.into_fetch(reader).await?;
        Ok(compressed_bytes.into_tile(x, y, self))
    }

    /// Fetch the tiles located at `x` column and `y` row using the provided reader.
    pub async fn fetch_tiles(
        &self,
        xy: &[(usize, usize)],
        reader: &dyn AsyncFileReader,
    ) -> AsyncTiffResult<Vec<Tile>> {
        let byte_ranges = self
            .tiles_byte_ranges(xy)
            .ok_or(AsyncTiffError::General("Not a tiled TIFF".to_string()))?;
        let compressed_bytes = byte_ranges.into_fetch(reader).await?;
        Ok(compressed_bytes
            .into_iter()
            .zip(xy)
            .map(|(buffer, (x, y))| buffer.into_tile(*x, *y, self))
            .collect())
    }

    /// Return the number of x/y tiles in the IFD
    /// Returns `None` if this is not a tiled TIFF
    pub fn tile_count(&self) -> Option<(usize, usize)> {
        let x_count = (self.image_width as f64 / self.tile_width? as f64).ceil();
        let y_count = (self.image_height as f64 / self.tile_height? as f64).ceil();
        Some((x_count as usize, y_count as usize))
    }
}

/// A description of the byte ranges for a tile, which may differ based on whether the TIFF is in
/// chunky or planar format.
pub enum TileByteRange {
    /// For chunky TIFFs, a single byte range for the tile that includes all bands.
    Chunky(Range<u64>),

    /// For planar TIFFs, separate byte ranges for each band of the tile.
    Planar(Vec<Range<u64>>),
}

impl TileByteRange {
    async fn into_fetch(self, reader: &dyn AsyncFileReader) -> AsyncTiffResult<CompressedBytes> {
        match self {
            Self::Chunky(range) => Ok(CompressedBytes::Chunky(reader.get_bytes(range).await?)),
            Self::Planar(ranges) => Ok(CompressedBytes::Planar(
                reader.get_byte_ranges(ranges).await?,
            )),
        }
    }

    fn from_ifd_tile(ifd: &ImageFileDirectory, x: usize, y: usize) -> Option<Self> {
        let tile_offsets = ifd.tile_offsets.as_deref()?;
        let tile_byte_counts = ifd.tile_byte_counts.as_deref()?;
        let (tiles_per_row, tiles_per_col) = ifd.tile_count()?;
        match ifd.planar_configuration {
            PlanarConfiguration::Chunky => {
                let idx = (y * tiles_per_row) + x;
                let offset = tile_offsets[idx];
                let byte_count = tile_byte_counts[idx];
                Some(TileByteRange::Chunky(offset..(offset + byte_count)))
            }
            PlanarConfiguration::Planar => {
                let tiles_per_band = tiles_per_row * tiles_per_col;
                let num_bands = ifd.samples_per_pixel as usize;
                let band_ranges = (0..num_bands)
                    .map(|band| {
                        let band_idx = (band * tiles_per_band) + (y * tiles_per_row) + x;
                        let offset = tile_offsets[band_idx];
                        let byte_count = tile_byte_counts[band_idx];
                        offset..(offset + byte_count)
                    })
                    .collect::<Vec<_>>();
                Some(TileByteRange::Planar(band_ranges))
            }
        }
    }
}

/// A description of the byte ranges for multiple tiles
pub enum TilesByteRanges {
    /// For chunky TIFFs, a byte range for each tile that includes all bands.
    Chunky(Vec<Range<u64>>),

    /// For planar TIFFs, separate byte ranges for each band of each tile.
    Planar(Vec<Vec<Range<u64>>>),
}

impl TilesByteRanges {
    async fn into_fetch(
        self,
        reader: &dyn AsyncFileReader,
    ) -> AsyncTiffResult<Vec<CompressedBytes>> {
        match self {
            Self::Chunky(ranges) => {
                let buffers = reader.get_byte_ranges(ranges).await?;
                Ok(buffers.into_iter().map(CompressedBytes::Chunky).collect())
            }
            Self::Planar(ranges) => {
                // Record how many bands each tile has, then flatten into a single fetch
                let band_counts: Vec<usize> = ranges.iter().map(|r| r.len()).collect();
                let flat_ranges: Vec<Range<u64>> = ranges.into_iter().flatten().collect();
                let flat_buffers = reader.get_byte_ranges(flat_ranges).await?;
                // Re-chunk the flat results back into per-tile band vecs
                let mut flat_iter = flat_buffers.into_iter();
                band_counts
                    .into_iter()
                    .map(|n| {
                        let band_bytes: Vec<Bytes> = flat_iter.by_ref().take(n).collect();
                        Ok(CompressedBytes::Planar(band_bytes))
                    })
                    .collect()
            }
        }
    }

    fn from_ifd_tiles(ifd: &ImageFileDirectory, xy: &[(usize, usize)]) -> Option<Self> {
        if xy.is_empty() {
            return match ifd.planar_configuration {
                PlanarConfiguration::Chunky => Some(TilesByteRanges::Chunky(vec![])),
                PlanarConfiguration::Planar => Some(TilesByteRanges::Planar(vec![])),
            };
        }

        let ranges = xy
            .iter()
            .map(|(x, y)| TileByteRange::from_ifd_tile(ifd, *x, *y))
            .collect::<Option<Vec<_>>>()?;

        match ifd.planar_configuration {
            PlanarConfiguration::Chunky => {
                let chunky_ranges = ranges
                    .into_iter()
                    .map(|r| match r {
                        TileByteRange::Chunky(range) => range,
                        _ => unreachable!(),
                    })
                    .collect();
                Some(TilesByteRanges::Chunky(chunky_ranges))
            }
            PlanarConfiguration::Planar => {
                let planar_ranges = ranges
                    .into_iter()
                    .map(|r| match r {
                        TileByteRange::Planar(band_ranges) => band_ranges,
                        _ => unreachable!(),
                    })
                    .collect();
                Some(TilesByteRanges::Planar(planar_ranges))
            }
        }
    }
}

/// Compressed tile data, either as a single chunk (chunky) or multiple chunks (planar).
#[derive(Debug, Clone)]
pub enum CompressedBytes {
    /// Single compressed chunk for chunky (pixel-interleaved) format.
    Chunky(Bytes),

    /// Multiple compressed chunks, one per band, for planar (band-interleaved) format.
    Planar(Vec<Bytes>),
}

impl CompressedBytes {
    fn into_tile(self, x: usize, y: usize, ifd: &ImageFileDirectory) -> Tile {
        let data_type = DataType::from_tags(&ifd.sample_format, &ifd.bits_per_sample);
        Tile {
            x,
            y,
            data_type,
            width: ifd.tile_width.unwrap_or(ifd.image_width),
            height: ifd.tile_height.unwrap_or(ifd.image_height),
            planar_configuration: ifd.planar_configuration,
            samples_per_pixel: ifd.samples_per_pixel,
            bits_per_sample: ifd.bits_per_sample[0],
            endianness: ifd.endianness,
            predictor: ifd.predictor.unwrap_or(Predictor::None),
            compressed_bytes: self,
            compression_method: ifd.compression,
            photometric_interpretation: ifd.photometric_interpretation,
            jpeg_tables: ifd.jpeg_tables.clone(),
            lerc_parameters: ifd.lerc_parameters.clone(),
        }
    }
}
