use std::collections::HashMap;
use std::io::{Cursor, Read};

use byteorder::{LittleEndian, ReadBytesExt};
use bytes::Buf;
use num_enum::TryFromPrimitive;
use tiff::decoder::ifd::Value;
use tiff::tags::{
    CompressionMethod, PhotometricInterpretation, PlanarConfiguration, Predictor, ResolutionUnit,
    SampleFormat, Tag, Type,
};
use tiff::{TiffError, TiffResult};

use crate::affine::AffineTransform;
use crate::cursor::ObjectStoreCursor;
use crate::geo_key_directory::{GeoKeyDirectory, GeoKeyTag};

const DOCUMENT_NAME: u16 = 269;

/// A collection of all the IFD
// TODO: maybe separate out the primary/first image IFD out of the vec, as that one should have
// geospatial metadata?
pub(crate) struct ImageFileDirectories {
    /// There's always at least one IFD in a TIFF. We store this separately
    ifds: Vec<ImageFileDirectory>,
    // Is it guaranteed that if masks exist that there will be one per image IFD? Or could there be
    // different numbers of image ifds and mask ifds?
    // mask_ifds: Option<Vec<IFD>>,
}

impl AsRef<[ImageFileDirectory]> for ImageFileDirectories {
    fn as_ref(&self) -> &[ImageFileDirectory] {
        &self.ifds
    }
}

impl ImageFileDirectories {
    pub(crate) async fn open(
        cursor: &mut ObjectStoreCursor,
        ifd_offset: usize,
    ) -> TiffResult<Self> {
        let mut next_ifd_offset = Some(ifd_offset);

        let mut ifds = vec![];
        while let Some(offset) = next_ifd_offset {
            let ifd = ImageFileDirectory::read(cursor, offset).await?;
            next_ifd_offset = ifd.next_ifd_offset();
            ifds.push(ifd);
        }

        Ok(Self { ifds })
    }
}

/// An ImageFileDirectory representing Image content
// The ordering of these tags matches the sorted order in TIFF spec Appendix A
#[allow(dead_code)]
#[derive(Debug, Clone)]
pub(crate) struct ImageFileDirectory {
    pub(crate) new_subfile_type: Option<u32>,

    /// The number of columns in the image, i.e., the number of pixels per row.
    pub(crate) image_width: u32,

    /// The number of rows of pixels in the image.
    pub(crate) image_height: u32,

    pub(crate) bits_per_sample: Vec<u16>,

    pub(crate) compression: CompressionMethod,

    pub(crate) photometric_interpretation: PhotometricInterpretation,

    pub(crate) document_name: Option<String>,

    pub(crate) image_description: Option<String>,

    pub(crate) strip_offsets: Option<Vec<u32>>,

    pub(crate) orientation: Option<u16>,

    pub(crate) samples_per_pixel: u16,

    pub(crate) rows_per_strip: Option<u32>,

    pub(crate) strip_byte_counts: Option<Vec<u32>>,

    pub(crate) min_sample_value: Option<Vec<u16>>,
    pub(crate) max_sample_value: Option<Vec<u16>>,

    pub(crate) x_resolution: Option<f64>,

    pub(crate) y_resolution: Option<f64>,

    pub(crate) planar_configuration: PlanarConfiguration,

    pub(crate) resolution_unit: Option<ResolutionUnit>,

    pub(crate) software: Option<String>,

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
    pub(crate) color_map: Option<Vec<u16>>,

    pub(crate) tile_width: u32,
    pub(crate) tile_height: u32,

    pub(crate) tile_offsets: Vec<u32>,
    pub(crate) tile_byte_counts: Vec<u32>,

    pub(crate) extra_samples: Option<Vec<u8>>,

    pub(crate) sample_format: Vec<SampleFormat>,

    pub(crate) jpeg_tables: Option<Vec<u8>>,

    pub(crate) copyright: Option<String>,

    // Geospatial tags
    pub(crate) geo_key_directory: Option<GeoKeyDirectory>,
    pub(crate) model_pixel_scale: Option<Vec<f64>>,
    pub(crate) model_tiepoint: Option<Vec<f64>>,

    // GDAL tags
    // no_data
    // gdal_metadata
    pub(crate) other_tags: HashMap<Tag, Value>,

    pub(crate) next_ifd_offset: Option<usize>,
}

impl ImageFileDirectory {
    async fn read(cursor: &mut ObjectStoreCursor, offset: usize) -> TiffResult<Self> {
        let ifd_start = offset;
        cursor.seek(offset);

        let tag_count = cursor.read_u16().await;
        // dbg!(tag_count);

        let mut tags = HashMap::with_capacity(tag_count as usize);
        for _ in 0..tag_count {
            let (tag_name, tag_value) = read_tag(cursor).await?;
            tags.insert(tag_name, tag_value);
        }

        cursor.seek(ifd_start + (12 * tag_count as usize) + 2);

        let next_ifd_offset = cursor.read_u32().await;
        let next_ifd_offset = if next_ifd_offset == 0 {
            None
        } else {
            Some(next_ifd_offset as usize)
        };

        Self::from_tags(tags, next_ifd_offset)
    }

    fn next_ifd_offset(&self) -> Option<usize> {
        self.next_ifd_offset
    }

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
        let mut geo_ascii_params: Option<String> = None;
        let mut geo_double_params: Option<Vec<f64>> = None;

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
                    compression = Some(CompressionMethod::from_u16_exhaustive(
                        value.into_u16().unwrap(),
                    ))
                }
                Tag::PhotometricInterpretation => {
                    photometric_interpretation =
                        PhotometricInterpretation::from_u16(value.into_u16().unwrap())
                }
                Tag::ImageDescription => image_description = Some(value.into_string()?),
                Tag::StripOffsets => strip_offsets = Some(value.into_u32_vec()?),
                Tag::Orientation => orientation = Some(value.into_u16().unwrap()),
                Tag::SamplesPerPixel => samples_per_pixel = Some(value.into_u16().unwrap()),
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
                    planar_configuration = PlanarConfiguration::from_u16(value.into_u16().unwrap())
                }
                Tag::ResolutionUnit => {
                    resolution_unit = ResolutionUnit::from_u16(value.into_u16().unwrap())
                }
                Tag::Software => software = Some(value.into_string()?),
                Tag::DateTime => date_time = Some(value.into_string()?),
                Tag::Artist => artist = Some(value.into_string()?),
                Tag::HostComputer => host_computer = Some(value.into_string()?),
                Tag::Predictor => predictor = Predictor::from_u16(value.into_u16().unwrap()),
                Tag::ColorMap => color_map = Some(value.into_u16_vec()?),
                Tag::TileWidth => tile_width = Some(value.into_u32()?),
                Tag::TileLength => tile_height = Some(value.into_u32()?),
                Tag::TileOffsets => tile_offsets = Some(value.into_u32_vec()?),
                Tag::TileByteCounts => tile_byte_counts = Some(value.into_u32_vec()?),
                Tag::ExtraSamples => extra_samples = Some(value.into_u8_vec()?),
                Tag::SampleFormat => {
                    let values = value.into_u16_vec()?;
                    sample_format = Some(
                        values
                            .into_iter()
                            .map(SampleFormat::from_u16_exhaustive)
                            .collect(),
                    );
                    // sample_format = SampleFormat::from_u16(value.into_u16_vec().unwrap())
                }
                Tag::JPEGTables => jpeg_tables = Some(value.into_u8_vec()?),
                Tag::Copyright => copyright = Some(value.into_string()?),

                // Geospatial tags
                Tag::GeoKeyDirectoryTag => {
                    // http://geotiff.maptools.org/spec/geotiff2.4.html
                    geo_key_directory_data = Some(value.into_u16_vec()?);
                }
                Tag::ModelPixelScaleTag => model_pixel_scale = Some(value.into_f64_vec()?),
                Tag::ModelTiepointTag => model_tiepoint = Some(value.into_f64_vec()?),
                Tag::GeoAsciiParamsTag => {
                    geo_ascii_params = Some(value.into_string()?);
                    // let s = value.into_string()?;
                    // geo_ascii_params = Some(s.split('|').map(|s| s.to_string()).collect())
                }
                Tag::GeoDoubleParamsTag => {
                    geo_double_params = Some(value.into_f64_vec()?);
                }
                // Tag::GdalNodata
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

            let header = chunks.next().unwrap();
            let key_directory_version = header[0];
            assert_eq!(key_directory_version, 1);

            let key_revision = header[1];
            assert_eq!(key_revision, 1);

            // let key_minor_revision = header[2];
            let number_of_keys = header[3];

            let mut tags = HashMap::with_capacity(number_of_keys as usize);
            for _ in 0..number_of_keys {
                let chunk = chunks.next().unwrap();

                let key_id = chunk[0];
                let tag_name = GeoKeyTag::try_from_primitive(key_id).unwrap();

                let tag_location = chunk[1];
                let count = chunk[2];
                let value_offset = chunk[3];

                if tag_location == 0 {
                    tags.insert(tag_name, Value::Short(value_offset));
                } else if Tag::from_u16_exhaustive(tag_location) == Tag::GeoAsciiParamsTag {
                    // If the tag_location points to the value of Tag::GeoAsciiParamsTag, then we
                    // need to extract a subslice from GeoAsciiParamsTag

                    let geo_ascii_params = geo_ascii_params.as_ref().unwrap();
                    let value_offset = value_offset as usize;
                    let mut s = &geo_ascii_params[value_offset..value_offset + count as usize];

                    // It seems that this string subslice might always include the final |
                    // character?
                    if s.ends_with('|') {
                        s = &s[0..s.len() - 1];
                    }

                    tags.insert(tag_name, Value::Ascii(s.to_string()));
                } else if Tag::from_u16_exhaustive(tag_location) == Tag::GeoDoubleParamsTag {
                    // If the tag_location points to the value of Tag::GeoDoubleParamsTag, then we
                    // need to extract a subslice from GeoDoubleParamsTag

                    let geo_double_params = geo_double_params.as_ref().unwrap();
                    let value_offset = value_offset as usize;
                    let value = if count == 1 {
                        Value::Double(geo_double_params[value_offset])
                    } else {
                        let x = geo_double_params[value_offset..value_offset + count as usize]
                            .iter()
                            .map(|val| Value::Double(*val))
                            .collect();
                        Value::List(x)
                    };
                    tags.insert(tag_name, value);
                }
            }
            geo_key_directory = Some(GeoKeyDirectory::from_tags(tags)?);
            dbg!(&geo_key_directory);
        }

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
            jpeg_tables,
            geo_key_directory,
            model_pixel_scale,
            model_tiepoint,
            other_tags,
            next_ifd_offset,
        })
    }

    /// Check if an IFD is masked based on a dictionary of tiff tags
    /// https://www.awaresystems.be/imaging/tiff/tifftags/newsubfiletype.html
    /// https://gdal.org/drivers/raster/gtiff.html#internal-nodata-masks
    pub fn is_masked(&self) -> bool {
        if let Some(subfile_type) = self.new_subfile_type {
            (subfile_type == 1 || subfile_type == 2)
                && self.photometric_interpretation == PhotometricInterpretation::TransparencyMask
                && self.compression == CompressionMethod::Deflate
        } else {
            false
        }
    }

    /// Construct colormap from colormap tag
    pub fn colormap(&self) -> Option<HashMap<usize, [u8; 3]>> {
        fn cmap_transform(val: u16) -> u8 {
            let val = ((val as f64 / 65535.0) * 255.0).floor();
            if val >= 255.0 {
                255
            } else if val < 0.0 {
                0
            } else {
                val as u8
            }
        }

        if let Some(cmap_data) = &self.color_map {
            let bits_per_sample = self.bits_per_sample[0];
            let count = 2_usize.pow(bits_per_sample as u32);
            let mut result = HashMap::new();

            // TODO: support nodata
            for idx in 0..count {
                let color: [u8; 3] =
                    std::array::from_fn(|i| cmap_transform(cmap_data[idx + i * count]));
                // TODO: Handle nodata value

                result.insert(idx, color);
            }

            Some(result)
        } else {
            None
        }
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
            val != 0
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

    /// Return the geotransform of the image
    ///
    /// This does not yet implement decimation
    pub fn geotransform(&self) -> Option<AffineTransform> {
        if let (Some(model_pixel_scale), Some(model_tiepoint)) =
            (&self.model_pixel_scale, &self.model_tiepoint)
        {
            Some(AffineTransform::new(
                model_pixel_scale[0],
                0.0,
                model_tiepoint[3],
                0.0,
                -model_pixel_scale[1],
                model_tiepoint[4],
            ))
        } else {
            None
        }
    }

    /// Return the bounds of the image in native crs
    pub fn native_bounds(&self) -> Option<(f64, f64, f64, f64)> {
        if let Some(gt) = self.geotransform() {
            let tlx = gt.c();
            let tly = gt.f();

            let brx = tlx + (gt.a() * self.image_width as f64);
            let bry = tly + (gt.e() * self.image_height as f64);
            Some((tlx, bry, brx, tly))
        } else {
            None
        }
    }
}

/// Read a single tag from the cursor
async fn read_tag(cursor: &mut ObjectStoreCursor) -> TiffResult<(Tag, Value)> {
    let code = cursor.read_u16().await;
    let tag_name = Tag::from_u16_exhaustive(code);
    // dbg!(&tag_name);

    let current_cursor_position = cursor.position();

    let tag_type = Type::from_u16(cursor.read_u16().await).unwrap();
    let count = cursor.read_u32().await as usize;

    let tag_value = read_tag_value(cursor, tag_type, count).await?;

    // TODO: better handle management of cursor state
    cursor.seek(current_cursor_position + 10);

    Ok((tag_name, tag_value))
}

/// Read a tag's value from the cursor
///
/// NOTE: this does not maintain cursor state
// This is derived from the upstream tiff crate:
// https://github.com/image-rs/image-tiff/blob/6dc7a266d30291db1e706c8133357931f9e2a053/src/decoder/ifd.rs#L369-L639
async fn read_tag_value(
    cursor: &mut ObjectStoreCursor,
    tag_type: Type,
    count: usize,
    // length: usize,
) -> TiffResult<Value> {
    // Case 1: there are no values so we can return immediately.
    if count == 0 {
        return Ok(Value::List(vec![]));
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
        t => panic!("unexpected type {t:?}"),
    };

    let value_byte_length = count.checked_mul(tag_size).unwrap();

    // Case 2: there is one value.
    if count == 1 {
        // 2a: the value is 5-8 bytes and we're in BigTiff mode.
        // We don't support bigtiff yet

        // dbg!(value_byte_length);
        // dbg!(tag_type);
        // NOTE: we should only be reading value_byte_length when it's 4 bytes or fewer. Right now
        // we're reading even if it's 8 bytes, but then only using the first 4 bytes of this
        // buffer.
        let data = cursor.read(value_byte_length).await;

        // 2b: the value is at most 4 bytes or doesn't fit in the offset field.
        return Ok(match tag_type {
            Type::BYTE | Type::UNDEFINED => Value::Byte(data.reader().read_u8().unwrap()),
            Type::SBYTE => Value::Signed(data.reader().read_i8().unwrap() as i32),
            Type::SHORT => Value::Short(data.reader().read_u16::<LittleEndian>().unwrap()),
            Type::SSHORT => Value::Signed(data.reader().read_i16::<LittleEndian>().unwrap() as i32),
            Type::LONG => Value::Unsigned(data.reader().read_u32::<LittleEndian>().unwrap()),
            Type::SLONG => Value::Signed(data.reader().read_i32::<LittleEndian>().unwrap()),
            Type::FLOAT => Value::Float(data.reader().read_f32::<LittleEndian>().unwrap()),
            Type::ASCII => {
                if data[0] == 0 {
                    Value::Ascii("".to_string())
                } else {
                    panic!("Invalid tag");
                    // return Err(TiffError::FormatError(TiffFormatError::InvalidTag));
                }
            }
            Type::LONG8 => {
                let offset = data.reader().read_u32::<LittleEndian>().unwrap();
                cursor.seek(offset as usize);
                Value::UnsignedBig(cursor.read_u64().await)
            }
            Type::SLONG8 => {
                let offset = data.reader().read_u32::<LittleEndian>().unwrap();
                cursor.seek(offset as usize);
                Value::SignedBig(cursor.read_i64().await)
            }
            Type::DOUBLE => {
                let offset = data.reader().read_u32::<LittleEndian>().unwrap();
                cursor.seek(offset as usize);
                Value::Double(cursor.read_f64().await)
            }
            Type::RATIONAL => {
                let offset = data.reader().read_u32::<LittleEndian>().unwrap();
                cursor.seek(offset as usize);
                let numerator = cursor.read_u32().await;
                let denominator = cursor.read_u32().await;
                Value::Rational(numerator, denominator)
            }
            Type::SRATIONAL => {
                let offset = data.reader().read_u32::<LittleEndian>().unwrap();
                cursor.seek(offset as usize);
                let numerator = cursor.read_i32().await;
                let denominator = cursor.read_i32().await;
                Value::SRational(numerator, denominator)
            }
            Type::IFD => Value::Ifd(data.reader().read_u32::<LittleEndian>().unwrap()),
            Type::IFD8 => {
                let offset = data.reader().read_u32::<LittleEndian>().unwrap();
                cursor.seek(offset as usize);
                Value::IfdBig(cursor.read_u64().await)
            }
            t => panic!("unexpected tag type {t:?}"),
        });
    }

    // Case 3: There is more than one value, but it fits in the offset field.
    if value_byte_length <= 4 {
        let data = cursor.read(value_byte_length).await;
        cursor.advance(4 - value_byte_length);

        match tag_type {
            Type::BYTE | Type::UNDEFINED => {
                return {
                    let mut data_cursor = Cursor::new(data);
                    Ok(Value::List(
                        (0..count)
                            .map(|_| Value::Byte(data_cursor.read_u8().unwrap()))
                            .collect(),
                    ))
                }
            }
            Type::SBYTE => {
                return {
                    let mut data_cursor = Cursor::new(data);
                    Ok(Value::List(
                        (0..count)
                            .map(|_| Value::Signed(data_cursor.read_i8().unwrap() as i32))
                            .collect(),
                    ))
                }
            }
            Type::ASCII => {
                let mut buf = vec![0; count];
                data.reader().read_exact(&mut buf).unwrap();
                if buf.is_ascii() && buf.ends_with(&[0]) {
                    let v = std::str::from_utf8(&buf)?;
                    let v = v.trim_matches(char::from(0));
                    return Ok(Value::Ascii(v.into()));
                } else {
                    panic!("Invalid tag");
                    // return Err(TiffError::FormatError(TiffFormatError::InvalidTag));
                }
            }
            Type::SHORT => {
                let mut reader = data.reader();
                let mut v = Vec::new();
                for _ in 0..count {
                    v.push(Value::Short(reader.read_u16::<LittleEndian>()?));
                }
                return Ok(Value::List(v));
            }
            Type::SSHORT => {
                let mut reader = data.reader();
                let mut v = Vec::new();
                for _ in 0..count {
                    v.push(Value::Signed(i32::from(reader.read_i16::<LittleEndian>()?)));
                }
                return Ok(Value::List(v));
            }
            Type::LONG => {
                let mut reader = data.reader();
                let mut v = Vec::new();
                for _ in 0..count {
                    v.push(Value::Unsigned(reader.read_u32::<LittleEndian>()?));
                }
                return Ok(Value::List(v));
            }
            Type::SLONG => {
                let mut reader = data.reader();
                let mut v = Vec::new();
                for _ in 0..count {
                    v.push(Value::Signed(reader.read_i32::<LittleEndian>()?));
                }
                return Ok(Value::List(v));
            }
            Type::FLOAT => {
                let mut reader = data.reader();
                let mut v = Vec::new();
                for _ in 0..count {
                    v.push(Value::Float(reader.read_f32::<LittleEndian>()?));
                }
                return Ok(Value::List(v));
            }
            Type::IFD => {
                let mut reader = data.reader();
                let mut v = Vec::new();
                for _ in 0..count {
                    v.push(Value::Ifd(reader.read_u32::<LittleEndian>()?));
                }
                return Ok(Value::List(v));
            }
            Type::LONG8
            | Type::SLONG8
            | Type::RATIONAL
            | Type::SRATIONAL
            | Type::DOUBLE
            | Type::IFD8 => {
                unreachable!()
            }
            t => panic!("unexpected tag type {t:?}"),
        }
    }

    // Seek cursor
    let offset = cursor.read_u32().await;
    cursor.seek(offset as usize);

    // Case 4: there is more than one value, and it doesn't fit in the offset field.
    match tag_type {
        // TODO check if this could give wrong results
        // at a different endianess of file/computer.
        Type::BYTE | Type::UNDEFINED => {
            let mut v = Vec::with_capacity(count);
            for _ in 0..count {
                v.push(Value::Byte(cursor.read_u8().await))
            }
            Ok(Value::List(v))
        }
        Type::SBYTE => {
            let mut v = Vec::with_capacity(count);
            for _ in 0..count {
                v.push(Value::Signed(cursor.read_i8().await as i32))
            }
            Ok(Value::List(v))
        }
        Type::SHORT => {
            let mut v = Vec::with_capacity(count);
            for _ in 0..count {
                v.push(Value::Short(cursor.read_u16().await))
            }
            Ok(Value::List(v))
        }
        Type::SSHORT => {
            let mut v = Vec::with_capacity(count);
            for _ in 0..count {
                v.push(Value::Signed(cursor.read_i16().await as i32))
            }
            Ok(Value::List(v))
        }
        Type::LONG => {
            let mut v = Vec::with_capacity(count);
            for _ in 0..count {
                v.push(Value::Unsigned(cursor.read_u32().await))
            }
            Ok(Value::List(v))
        }
        Type::SLONG => {
            let mut v = Vec::with_capacity(count);
            for _ in 0..count {
                v.push(Value::Signed(cursor.read_i32().await))
            }
            Ok(Value::List(v))
        }
        Type::FLOAT => {
            let mut v = Vec::with_capacity(count);
            for _ in 0..count {
                v.push(Value::Float(cursor.read_f32().await))
            }
            Ok(Value::List(v))
        }
        Type::DOUBLE => {
            let mut v = Vec::with_capacity(count);
            for _ in 0..count {
                v.push(Value::Double(cursor.read_f64().await))
            }
            Ok(Value::List(v))
        }
        Type::RATIONAL => {
            let mut v = Vec::with_capacity(count);
            for _ in 0..count {
                v.push(Value::Rational(
                    cursor.read_u32().await,
                    cursor.read_u32().await,
                ))
            }
            Ok(Value::List(v))
        }
        Type::SRATIONAL => {
            let mut v = Vec::with_capacity(count);
            for _ in 0..count {
                v.push(Value::SRational(
                    cursor.read_i32().await,
                    cursor.read_i32().await,
                ))
            }
            Ok(Value::List(v))
        }
        Type::LONG8 => {
            let mut v = Vec::with_capacity(count);
            for _ in 0..count {
                v.push(Value::UnsignedBig(cursor.read_u64().await))
            }
            Ok(Value::List(v))
        }
        Type::SLONG8 => {
            let mut v = Vec::with_capacity(count);
            for _ in 0..count {
                v.push(Value::SignedBig(cursor.read_i64().await))
            }
            Ok(Value::List(v))
        }
        Type::IFD => {
            let mut v = Vec::with_capacity(count);
            for _ in 0..count {
                v.push(Value::Ifd(cursor.read_u32().await))
            }
            Ok(Value::List(v))
        }
        Type::IFD8 => {
            let mut v = Vec::with_capacity(count);
            for _ in 0..count {
                v.push(Value::IfdBig(cursor.read_u64().await))
            }
            Ok(Value::List(v))
        }
        Type::ASCII => {
            let n = count;
            let mut out = vec![0; n];
            let buf = cursor.read(n).await;
            buf.reader().read_exact(&mut out).unwrap();

            // Strings may be null-terminated, so we trim anything downstream of the null byte
            if let Some(first) = out.iter().position(|&b| b == 0) {
                out.truncate(first);
            }
            Ok(Value::Ascii(String::from_utf8(out)?))
        }
        t => panic!("unexpected tag type {t:?}"),
    }
}
