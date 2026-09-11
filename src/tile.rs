use bytes::Bytes;

use crate::array::Array;
use crate::decoder::DecoderRegistry;
use crate::error::{AsyncTiffResult, TiffError, TiffUnsupportedError};
use crate::ifd::CompressedBytes;
use crate::predictor::{fix_endianness, unpredict_float, unpredict_hdiff};
use crate::reader::Endianness;
use crate::tags::{Compression, PhotometricInterpretation, PlanarConfiguration, Predictor};
use crate::DataType;

/// A TIFF Tile response.
///
/// This contains the required information to decode the tile. Decoding is separated from fetching
/// so that sync and async operations can be separated and non-blocking.
///
/// This is returned by `fetch_tile`.
///
/// A strip of a stripped tiff is an image-width, rows-per-strip tile.
#[derive(Debug, Clone)]
pub struct Tile {
    pub(crate) x: usize,
    pub(crate) y: usize,
    pub(crate) data_type: Option<DataType>,
    pub(crate) samples_per_pixel: u16,
    pub(crate) bits_per_sample: u16,
    pub(crate) endianness: Endianness,
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) planar_configuration: PlanarConfiguration,
    pub(crate) predictor: Predictor,
    pub(crate) compressed_bytes: CompressedBytes,
    pub(crate) compression_method: Compression,
    pub(crate) photometric_interpretation: PhotometricInterpretation,
    pub(crate) jpeg_tables: Option<Bytes>,
    /// LERC parameters from the LercParameters tag: [version, compression_type, ...]
    /// compression_type: 0 = none, 1 = deflate, 2 = zstd
    pub(crate) lerc_parameters: Option<Vec<u32>>,
    /// The IFD's `GDAL_NODATA` value, if present; used to fill sparse tiles/bands.
    pub(crate) nodata: Option<f64>,
}

impl Tile {
    /// The column index of this tile.
    pub fn x(&self) -> usize {
        self.x
    }

    /// The row index of this tile.
    pub fn y(&self) -> usize {
        self.y
    }

    /// Access the compressed bytes underlying this tile.
    ///
    /// Note that [`Bytes`] is reference-counted, so it is very cheap to clone if needed.
    pub fn compressed_bytes(&self) -> &CompressedBytes {
        &self.compressed_bytes
    }

    /// Access the compression tag representing this tile.
    pub fn compression_method(&self) -> Compression {
        self.compression_method
    }

    /// Access the photometric interpretation tag representing this tile.
    pub fn photometric_interpretation(&self) -> PhotometricInterpretation {
        self.photometric_interpretation
    }

    /// Access the JPEG Tables, if any, from the IFD producing this tile.
    ///
    /// Note that [`Bytes`] is reference-counted, so it is very cheap to clone if needed.
    pub fn jpeg_tables(&self) -> Option<&Bytes> {
        self.jpeg_tables.as_ref()
    }

    /// Whether this tile is entirely sparse (never written by the encoder). `false` for a
    /// planar tile with only some bands sparse -- inspect [`Tile::compressed_bytes`] for
    /// per-band detail in that case.
    pub fn is_sparse(&self) -> bool {
        match &self.compressed_bytes {
            CompressedBytes::Chunky(bytes) => bytes.is_none(),
            CompressedBytes::Planar(bands) => bands.iter().all(|b| b.is_none()),
        }
    }

    /// Decode this tile to an [`Array`].
    ///
    /// Decoding is separate from data fetching so that sync and async operations do not block the
    /// same runtime.
    ///
    /// A sparse tile or band (see [`Tile::is_sparse`]) is filled with the nodata value
    /// rather than decoded.
    pub fn decode(self, decoder_registry: &DecoderRegistry) -> AsyncTiffResult<Array> {
        let samples = self.samples_per_pixel as usize;
        let bits_per_sample = self.bits_per_sample;
        // tile_width is the full encoded tile width — predictor must use this, not the cropped width
        let tile_width = self.width as usize;
        let height = self.height as usize;

        let decoded = match &self.compressed_bytes {
            CompressedBytes::Chunky(None) => self.fill_bytes(tile_width * height * samples),
            CompressedBytes::Chunky(Some(bytes)) => {
                let decoder = self.get_decoder(decoder_registry)?;
                let decoded_tile = decoder.decode_tile(
                    bytes.clone(),
                    self.photometric_interpretation,
                    self.jpeg_tables.as_deref(),
                    self.samples_per_pixel,
                    bits_per_sample,
                    self.lerc_parameters.as_deref(),
                )?;
                self.apply_predictor(decoded_tile, samples, tile_width)?
            }
            CompressedBytes::Planar(band_bytes) => {
                let bytes_per_sample = (bits_per_sample as usize).div_ceil(8);
                let plane_len = tile_width * height * bytes_per_sample;
                let mut result = Vec::with_capacity(band_bytes.len() * plane_len);

                for band in band_bytes {
                    let band_decoded = match band {
                        None => self.fill_bytes(tile_width * height),
                        Some(bytes) => {
                            let decoder = self.get_decoder(decoder_registry)?;
                            let decoded_band = decoder.decode_tile(
                                bytes.clone(),
                                self.photometric_interpretation,
                                self.jpeg_tables.as_deref(),
                                1,
                                bits_per_sample,
                                self.lerc_parameters.as_deref(),
                            )?;
                            // Each band is its own plane, so samples = 1 here, not
                            // self.samples_per_pixel.
                            self.apply_predictor(decoded_band, 1, tile_width)?
                        }
                    };
                    debug_assert_eq!(band_decoded.len(), plane_len);
                    result.extend_from_slice(&band_decoded);
                }

                result
            }
        };

        let shape = infer_shape(
            self.planar_configuration,
            self.width as _,
            self.height as _,
            samples,
        );
        Array::try_new(decoded, shape, self.data_type)
    }

    fn get_decoder<'a>(
        &self,
        decoder_registry: &'a DecoderRegistry,
    ) -> AsyncTiffResult<&'a dyn crate::decoder::Decoder> {
        let decoder = decoder_registry
            .as_ref()
            .get(&self.compression_method)
            .ok_or(TiffError::UnsupportedError(
                TiffUnsupportedError::UnsupportedCompression(self.compression_method),
            ))?;
        Ok(decoder.as_ref())
    }

    fn apply_predictor(
        &self,
        decoded: Vec<u8>,
        samples: usize,
        tile_width: usize,
    ) -> AsyncTiffResult<Vec<u8>> {
        let bits_per_sample = self.bits_per_sample;
        Ok(match self.predictor {
            Predictor::None => {
                let mut decoded = decoded;
                fix_endianness(&mut decoded, self.endianness, bits_per_sample);
                decoded
            }
            Predictor::Horizontal => unpredict_hdiff(
                decoded,
                self.endianness,
                samples,
                bits_per_sample,
                tile_width,
            ),
            Predictor::FloatingPoint => {
                unpredict_float(decoded, samples, bits_per_sample, tile_width)?
            }
        })
    }

    /// `element_count` samples worth of already-decoded bytes, each set to the resolved
    /// nodata value (or `0`).
    fn fill_bytes(&self, element_count: usize) -> Vec<u8> {
        let value = self.nodata.unwrap_or(0.0);
        match self.data_type {
            None | Some(DataType::UInt8) => {
                repeat_ne_bytes((value as u8).to_ne_bytes(), element_count)
            }
            Some(DataType::Bool) => {
                // Packed bit form, 1 bit per sample. Valid (true) only for an explicit
                // non-zero nodata; default is all-invalid.
                let byte = if value != 0.0 { 0xFF } else { 0x00 };
                vec![byte; element_count.div_ceil(8)]
            }
            Some(DataType::UInt16) => repeat_ne_bytes((value as u16).to_ne_bytes(), element_count),
            Some(DataType::UInt32) => repeat_ne_bytes((value as u32).to_ne_bytes(), element_count),
            Some(DataType::UInt64) => repeat_ne_bytes((value as u64).to_ne_bytes(), element_count),
            Some(DataType::Int8) => repeat_ne_bytes((value as i8).to_ne_bytes(), element_count),
            Some(DataType::Int16) => repeat_ne_bytes((value as i16).to_ne_bytes(), element_count),
            Some(DataType::Int32) => repeat_ne_bytes((value as i32).to_ne_bytes(), element_count),
            Some(DataType::Int64) => repeat_ne_bytes((value as i64).to_ne_bytes(), element_count),
            Some(DataType::Float32) => repeat_ne_bytes((value as f32).to_ne_bytes(), element_count),
            Some(DataType::Float64) => repeat_ne_bytes(value.to_ne_bytes(), element_count),
        }
    }
}

fn repeat_ne_bytes<const N: usize>(sample: [u8; N], count: usize) -> Vec<u8> {
    let mut out = Vec::with_capacity(N * count);
    for _ in 0..count {
        out.extend_from_slice(&sample);
    }
    out
}

fn infer_shape(
    planar_configuration: PlanarConfiguration,
    width: usize,
    height: usize,
    samples_per_pixel: usize,
) -> [usize; 3] {
    match planar_configuration {
        PlanarConfiguration::Chunky => [height, width, samples_per_pixel],
        PlanarConfiguration::Planar => [samples_per_pixel, height, width],
    }
}
