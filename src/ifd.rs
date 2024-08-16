use std::collections::HashMap;

use tiff::decoder::ifd::{Directory, Value};
use tiff::tags::{
    CompressionMethod, PhotometricInterpretation, PlanarConfiguration, Predictor, ResolutionUnit,
    SampleFormat, Tag, Type,
};
use tiff::{TiffError, TiffResult};

use crate::cursor::ObjectStoreCursor;

const DOCUMENT_NAME: u16 = 269;

/// A collection of all the IFD
// TODO: maybe separate out the primary/first image IFD out of the vec, as that one should have
// geospatial metadata?
pub(crate) struct ImageFileDirectories {
    /// There's always at least one IFD in a TIFF. We store this separately
    image_ifds: Vec<ImageIFD>,

    // Is it guaranteed that if masks exist that there will be one per image IFD? Or could there be
    // different numbers of image ifds and mask ifds?
    mask_ifds: Option<Vec<MaskIFD>>,
}

impl ImageFileDirectories {
    pub(crate) async fn open(cursor: &mut ObjectStoreCursor, ifd_offset: usize) -> Self {
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

/// An ImageFileDirectory representing Image content
// The ordering of these tags matches the sorted order in TIFF spec Appendix A
#[allow(dead_code)]
struct ImageIFD {
    new_subfile_type: Option<u32>,

    /// The number of columns in the image, i.e., the number of pixels per row.
    image_width: u32,

    /// The number of rows of pixels in the image.
    image_height: u32,

    bits_per_sample: Vec<u16>,

    compression: CompressionMethod,

    photometric_interpretation: PhotometricInterpretation,

    document_name: Option<String>,

    image_description: Option<String>,

    strip_offsets: Option<Vec<u32>>,

    orientation: Option<u16>,

    samples_per_pixel: u16,

    rows_per_strip: Option<u32>,

    strip_byte_counts: Option<Vec<u32>>,

    min_sample_value: Option<Vec<u16>>,
    max_sample_value: Option<Vec<u16>>,

    x_resolution: Option<f64>,

    y_resolution: Option<f64>,

    planar_configuration: PlanarConfiguration,

    resolution_unit: Option<ResolutionUnit>,

    software: Option<String>,

    date_time: Option<String>,
    artist: Option<String>,
    host_computer: Option<String>,

    predictor: Option<Predictor>,

    color_map: Option<Vec<u16>>,

    tile_width: u32,
    tile_height: u32,

    tile_offsets: Vec<u32>,
    tile_byte_counts: Vec<u32>,

    extra_samples: Option<Vec<u8>>,

    sample_format: SampleFormat,

    copyright: Option<String>,

    // Geospatial tags
    // geo_key_directory
    // model_pixel_scale
    // model_tiepoint

    // GDAL tags
    // no_data
    // gdal_metadata
    other_tags: HashMap<Tag, Value>,

    next_ifd_offset: Option<usize>,
}

impl ImageIFD {
    fn from_tags(
        mut tag_data: HashMap<Tag, Value>,
        next_ifd_offset: Option<usize>,
    ) -> TiffResult<Self> {
        let mut new_subfile_type = None;
        let mut image_width = None;
        let mut image_height = None;
        let mut bits_per_sample = None;
        let mut compression = None;
        let mut photometric_interpretation = None;
        // Note: tiff crate doesn't have a tag for document name
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
        let mut copyright = None;

        let mut other_tags = HashMap::new();

        tag_data.drain().try_for_each(|(tag, value)| {
            match tag {
                Tag::NewSubfileType => new_subfile_type = Some(value.into_u32()?),
                Tag::ImageWidth => {
                    image_width = Some(value.into_u32()?);
                }
                Tag::ImageLength => {
                    image_height = Some(value.into_u32()?);
                }
                Tag::BitsPerSample => {
                    bits_per_sample = Some(value.into_u16_vec()?);
                }
                Tag::Compression => {
                    compression = Some(CompressionMethod::from_u16_exhaustive(value.into_u16()?))
                }
                Tag::PhotometricInterpretation => {
                    photometric_interpretation =
                        PhotometricInterpretation::from_u16(value.into_u16()?)
                }
                Tag::ImageDescription => image_description = Some(value.into_string()?),
                Tag::StripOffsets => strip_offsets = Some(value.into_u32_vec()?),
                Tag::Orientation => orientation = Some(value.into_u16()?),
                Tag::SamplesPerPixel => samples_per_pixel = Some(value.into_u16()?),
                Tag::RowsPerStrip => rows_per_strip = Some(value.into_u32()?),
                Tag::StripByteCounts => strip_byte_counts = Some(value.into_u32_vec()?),
                Tag::MinSampleValue => min_sample_value = Some(value.into_u16_vec()?),
                Tag::MaxSampleValue => max_sample_value = Some(value.into_u16_vec()?),
                Tag::XResolution => match value {
                    Value::Rational(n, d) => x_resolution = Some(n as f64 / d as f64),
                    _ => unreachable!(),
                },
                Tag::YResolution => match value {
                    Value::Rational(n, d) => y_resolution = Some(n as f64 / d as f64),
                    _ => unreachable!(),
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
                Tag::ColorMap => color_map = Some(value.into_u16_vec()?),
                Tag::TileWidth => tile_width = Some(value.into_u32()?),
                Tag::TileLength => tile_height = Some(value.into_u32()?),
                Tag::TileOffsets => tile_offsets = Some(value.into_u32_vec()?),
                Tag::TileByteCounts => tile_byte_counts = Some(value.into_u32_vec()?),
                Tag::ExtraSamples => extra_samples = Some(value.into_u8_vec()?),
                Tag::SampleFormat => sample_format = SampleFormat::from_u16(value.into_u16()?),
                Tag::Copyright => copyright = Some(value.into_string()?),

                // Geospatial tags
                // Tag::GeoKeyDirectoryTag
                // Tag::ModelPixelScaleTag
                // Tag::ModelTiepointTag
                // Tag::GdalNodata
                Tag::Unknown(code) => {
                    match code {
                        DOCUMENT_NAME => document_name = Some(value.into_string()?),
                        _ => panic!("Unknown tag code {code}"),
                    }
                    todo!("handle unknown")
                }

                _ => {
                    other_tags.insert(tag, value);
                }
            };
            Ok::<_, TiffError>(())
        })?;

        Ok(Self {
            new_subfile_type,
            image_width: image_width.unwrap(),
            image_height: image_height.unwrap(),
            bits_per_sample: bits_per_sample.unwrap(),
            compression: compression.unwrap(),
            photometric_interpretation: photometric_interpretation.unwrap(),
            document_name,
            image_description,
            strip_offsets,
            orientation,
            samples_per_pixel: samples_per_pixel.unwrap(),
            rows_per_strip,
            strip_byte_counts,
            min_sample_value,
            max_sample_value,
            x_resolution,
            y_resolution,
            planar_configuration: planar_configuration.unwrap(),
            resolution_unit,
            software,
            date_time,
            artist,
            host_computer,
            predictor,
            color_map,
            tile_width: tile_width.unwrap(),
            tile_height: tile_height.unwrap(),
            tile_offsets: tile_offsets.unwrap(),
            tile_byte_counts: tile_byte_counts.unwrap(),
            extra_samples,
            sample_format: sample_format.unwrap(),
            copyright,
            other_tags,
            next_ifd_offset,
        })
    }

    pub fn compression(&self) -> CompressionMethod {
        self.compression
    }

    pub fn bands(&self) -> u16 {
        self.samples_per_pixel
    }

    // pub fn dtype(&self)

    // pub fn nodata(&self)

    pub fn has_extra_samples(&self) -> bool {
        self.extra_samples.is_some()
    }

    /// Return the interleave of the IFD
    pub fn interleave(&self) -> PlanarConfiguration {
        self.planar_configuration
    }

    /// Returns true if this IFD contains a full resolution image (not an overview)
    pub fn is_full_resolution(&self) -> bool {
        if let Some(val) = self.new_subfile_type {
            if val == 0 {
                false
            } else {
                true
            }
        } else {
            true
        }
    }

    pub async fn get_tile(&self, x: usize, y: usize) {
        let idx = (y * self.tile_count().0) + x;
        let offset = self.tile_offsets[idx];
        // TODO: aiocogeo has a -1 here, but I think that was in error
        let byte_count = self.tile_byte_counts[idx];
        todo!()
    }

    /// Return the number of x/y tiles in the IFD
    pub fn tile_count(&self) -> (usize, usize) {
        let x_count = (self.image_width as f64 / self.tile_width as f64).ceil();
        let y_count = (self.image_height as f64 / self.tile_height as f64).ceil();
        (x_count as usize, y_count as usize)
    }
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

        let tag_count = cursor.read_u16().await;
        dbg!(tag_count);

        let mut tags = HashMap::with_capacity(tag_count as usize);
        for _ in 0..tag_count {
            if let Some((tag_name, tag_value)) = read_tag(cursor).await {
                tags.insert(tag_name, tag_value);
            }
        }

        cursor.seek(ifd_start + (12 * tag_count as usize) + 2);

        let next_ifd_offset = cursor.read_u32().await;
        let next_ifd_offset = if next_ifd_offset == 0 {
            None
        } else {
            Some(next_ifd_offset as usize)
        };

        if is_masked_ifd() {
            todo!()
            // Self::Mask(MaskIFD { next_ifd_offset })
        } else {
            Self::Image(ImageIFD::from_tags(tags, next_ifd_offset).unwrap())
        }
    }

    fn next_ifd_offset(&self) -> Option<usize> {
        match self {
            Self::Image(ifd) => ifd.next_ifd_offset,
            Self::Mask(ifd) => ifd.next_ifd_offset,
        }
    }
}

async fn read_tag(cursor: &mut ObjectStoreCursor) -> Option<(Tag, Value)> {
    let code = cursor.read_u16().await;
    let tag_name = Tag::from_u16(code);
    dbg!(&tag_name);

    if let Some(tag) = tag_name {
        let tag_type = Type::from_u16(cursor.read_u16().await).unwrap();
        let count = cursor.read_u32().await;
        let length = tag_type.tag_size() * count as usize;
        if length <= 4 {
            let data = cursor.read(length).await;
            // data.read
            // TODO: parse tag data
            cursor.advance(4 - length);

            Some((tag, Value::Byte(0)))
        } else {
            let value_offset = cursor.read_u32().await;
            dbg!(value_offset);
            dbg!("support for reading tag values elsewhere in file");
            None
        }
    } else {
        dbg!("TIFF Tag with code {code} is not supported");
        cursor.advance(10);
        None
    }
}

fn is_masked_ifd() -> bool {
    false
    // https://github.com/geospatial-jeff/aiocogeo/blob/5a1d32c3f22c883354804168a87abb0a2ea1c328/aiocogeo/ifd.py#L66
}

async fn read_tag_value(
    cursor: &mut ObjectStoreCursor,
    tag_type: Type,
    count: usize,
    length: usize,
) -> Value {
    if count == 0 {
        return Value::List(vec![]);
    }

    let value_bytes = count.checked_mul(tag_type.tag_size()).unwrap();
    if count == 1 {
        // TODO: support bigtiff
        // match tag_type {
        //     Type::BYTE =>
        // }
    }

    todo!()
}

trait TagTypeSize {
    fn tag_size(&self) -> usize;
}

impl TagTypeSize for Type {
    fn tag_size(&self) -> usize {
        match self {
            Type::BYTE | Type::SBYTE | Type::ASCII | Type::UNDEFINED => 1,
            Type::SHORT | Type::SSHORT => 2,
            Type::LONG | Type::SLONG | Type::FLOAT | Type::IFD => 4,
            Type::LONG8
            | Type::SLONG8
            | Type::DOUBLE
            | Type::RATIONAL
            | Type::SRATIONAL
            | Type::IFD8 => 8,
            t => panic!("unexpected type {t:?}"),
        }
    }
}
