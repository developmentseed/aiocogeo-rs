use std::collections::HashMap;
use std::sync::Arc;

use async_tiff::reader::AsyncFileReader;
use async_tiff::{ImageFileDirectory, TileByteRange};
use pyo3::exceptions::{PyTypeError, PyValueError};
use pyo3::prelude::*;
use pyo3::IntoPyObjectExt;
use pyo3_async_runtimes::tokio::future_into_py;

use crate::colormap::PyColormap;
use crate::enums::{
    PyCompression, PyExtraSamples, PyPhotometricInterpretation, PyPlanarConfiguration, PyPredictor,
    PyResolutionUnit, PySampleFormat,
};
use crate::error::PyAsyncTiffResult;
use crate::geo::PyGeoKeyDirectory;
use crate::tile::PyTile;
use crate::value::PyValue;

#[pyclass(name = "ImageFileDirectory", frozen, eq, skip_from_py_object)]
#[derive(Debug, Clone)]
pub(crate) struct PyImageFileDirectory {
    ifd: Arc<ImageFileDirectory>,
    reader: Arc<dyn AsyncFileReader>,
}

impl PyImageFileDirectory {
    pub(crate) fn new(
        ifd: Arc<ImageFileDirectory>,
        reader: Arc<dyn AsyncFileReader>,
    ) -> PyImageFileDirectory {
        PyImageFileDirectory { ifd, reader }
    }
}

#[pymethods]
impl PyImageFileDirectory {
    #[getter]
    pub fn new_subfile_type(&self) -> Option<u32> {
        self.ifd.new_subfile_type()
    }

    /// The number of columns in the image, i.e., the number of pixels per row.
    #[getter]
    pub fn image_width(&self) -> u32 {
        self.ifd.image_width()
    }

    /// The number of rows of pixels in the image.
    #[getter]
    pub fn image_height(&self) -> u32 {
        self.ifd.image_height()
    }

    #[getter]
    pub fn bits_per_sample(&self) -> &[u16] {
        self.ifd.bits_per_sample()
    }

    #[getter]
    pub fn compression(&self) -> PyCompression {
        self.ifd.compression().into()
    }

    #[getter]
    pub fn photometric_interpretation(&self) -> PyPhotometricInterpretation {
        self.ifd.photometric_interpretation().into()
    }

    #[getter]
    pub fn document_name(&self) -> Option<&str> {
        self.ifd.document_name()
    }

    #[getter]
    pub fn image_description(&self) -> Option<&str> {
        self.ifd.image_description()
    }

    #[getter]
    pub fn strip_offsets(&self) -> Option<&[u64]> {
        self.ifd.strip_offsets()
    }

    #[getter]
    pub fn orientation(&self) -> Option<u16> {
        self.ifd.orientation()
    }

    /// The number of components per pixel.
    ///
    /// SamplesPerPixel is usually 1 for bilevel, grayscale, and palette-color images.
    /// SamplesPerPixel is usually 3 for RGB images. If this value is higher, ExtraSamples should
    /// give an indication of the meaning of the additional channels.
    #[getter]
    pub fn samples_per_pixel(&self) -> u16 {
        self.ifd.samples_per_pixel()
    }

    #[getter]
    pub fn rows_per_strip(&self) -> Option<u32> {
        self.ifd.rows_per_strip()
    }

    #[getter]
    pub fn strip_byte_counts(&self) -> Option<&[u64]> {
        self.ifd.strip_byte_counts()
    }

    #[getter]
    pub fn min_sample_value(&self) -> Option<&[u16]> {
        self.ifd.min_sample_value()
    }

    #[getter]
    pub fn max_sample_value(&self) -> Option<&[u16]> {
        self.ifd.max_sample_value()
    }

    /// The number of pixels per ResolutionUnit in the ImageWidth direction.
    #[getter]
    pub fn x_resolution(&self) -> Option<f64> {
        self.ifd.x_resolution()
    }

    /// The number of pixels per ResolutionUnit in the ImageLength direction.
    #[getter]
    pub fn y_resolution(&self) -> Option<f64> {
        self.ifd.y_resolution()
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
    #[getter]
    pub fn planar_configuration(&self) -> PyPlanarConfiguration {
        self.ifd.planar_configuration().into()
    }

    #[getter]
    pub fn resolution_unit(&self) -> Option<PyResolutionUnit> {
        self.ifd.resolution_unit().map(|x| x.into())
    }

    /// Name and version number of the software package(s) used to create the image.
    #[getter]
    pub fn software(&self) -> Option<&str> {
        self.ifd.software()
    }

    /// Date and time of image creation.
    ///
    /// The format is: "YYYY:MM:DD HH:MM:SS", with hours like those on a 24-hour clock, and one
    /// space character between the date and the time. The length of the string, including the
    /// terminating NUL, is 20 bytes.
    #[getter]
    pub fn date_time(&self) -> Option<&str> {
        self.ifd.date_time()
    }

    #[getter]
    pub fn artist(&self) -> Option<&str> {
        self.ifd.artist()
    }

    #[getter]
    pub fn host_computer(&self) -> Option<&str> {
        self.ifd.host_computer()
    }

    #[getter]
    pub fn predictor(&self) -> Option<PyPredictor> {
        self.ifd.predictor().map(|x| x.into())
    }

    #[getter]
    pub fn tile_width(&self) -> Option<u32> {
        self.ifd.tile_width()
    }
    #[getter]
    pub fn tile_height(&self) -> Option<u32> {
        self.ifd.tile_height()
    }

    #[getter]
    pub fn tile_offsets(&self) -> Option<&[u64]> {
        self.ifd.tile_offsets()
    }
    #[getter]
    pub fn tile_byte_counts(&self) -> Option<&[u64]> {
        self.ifd.tile_byte_counts()
    }

    #[getter]
    pub fn extra_samples(&self) -> Option<Vec<PyExtraSamples>> {
        self.ifd
            .extra_samples()
            .map(|samples| samples.iter().map(|x| (*x).into()).collect())
    }

    #[getter]
    pub fn sample_format(&self) -> Vec<PySampleFormat> {
        self.ifd
            .sample_format()
            .iter()
            .map(|x| (*x).into())
            .collect()
    }

    #[getter]
    pub fn jpeg_tables(&self) -> Option<&[u8]> {
        self.ifd.jpeg_tables()
    }

    #[getter]
    pub fn copyright(&self) -> Option<&str> {
        self.ifd.copyright()
    }

    // Geospatial tags
    #[getter]
    pub fn geo_key_directory(&self) -> Option<PyGeoKeyDirectory> {
        self.ifd.geo_key_directory().cloned().map(|x| x.into())
    }

    #[getter]
    pub fn model_pixel_scale(&self) -> Option<&[f64]> {
        self.ifd.model_pixel_scale()
    }

    #[getter]
    pub fn model_tiepoint(&self) -> Option<&[f64]> {
        self.ifd.model_tiepoint()
    }

    #[getter]
    pub fn model_transformation(&self) -> Option<&[f64]> {
        self.ifd.model_transformation()
    }

    #[getter]
    pub fn gdal_nodata(&self) -> Option<&str> {
        self.ifd.gdal_nodata()
    }

    #[getter]
    pub fn gdal_metadata(&self) -> Option<&str> {
        self.ifd.gdal_metadata()
    }

    #[getter]
    pub fn other_tags(&self) -> HashMap<u16, PyValue> {
        let iter = self
            .ifd
            .other_tags()
            .iter()
            .map(|(key, val)| (key.to_u16(), val.clone().into()));
        HashMap::from_iter(iter)
    }

    #[getter]
    pub fn lerc_parameters(&self) -> Option<&[u32]> {
        self.ifd.lerc_parameters()
    }

    #[getter]
    pub fn colormap(&self) -> Option<PyColormap> {
        self.ifd.colormap().map(|c| PyColormap::new(c.clone()))
    }

    /// This exists to implement the Mapping protocol, so we support `dict(ifd)`.`
    fn keys(&self) -> Vec<&'static str> {
        // Always present keys
        let mut keys = vec![
            "image_width",
            "image_height",
            "bits_per_sample",
            "compression",
            "photometric_interpretation",
            "samples_per_pixel",
            "planar_configuration",
            "sample_format",
            "other_tags",
        ];

        // Optional keys
        if self.new_subfile_type().is_some() {
            keys.push("new_subfile_type");
        }
        if self.document_name().is_some() {
            keys.push("document_name");
        }
        if self.image_description().is_some() {
            keys.push("image_description");
        }
        if self.strip_offsets().is_some() {
            keys.push("strip_offsets");
        }
        if self.orientation().is_some() {
            keys.push("orientation");
        }
        if self.rows_per_strip().is_some() {
            keys.push("rows_per_strip");
        }
        if self.strip_byte_counts().is_some() {
            keys.push("strip_byte_counts");
        }
        if self.min_sample_value().is_some() {
            keys.push("min_sample_value");
        }
        if self.max_sample_value().is_some() {
            keys.push("max_sample_value");
        }
        if self.x_resolution().is_some() {
            keys.push("x_resolution");
        }
        if self.y_resolution().is_some() {
            keys.push("y_resolution");
        }
        if self.resolution_unit().is_some() {
            keys.push("resolution_unit");
        }
        if self.software().is_some() {
            keys.push("software");
        }
        if self.date_time().is_some() {
            keys.push("date_time");
        }
        if self.artist().is_some() {
            keys.push("artist");
        }
        if self.host_computer().is_some() {
            keys.push("host_computer");
        }
        if self.predictor().is_some() {
            keys.push("predictor");
        }
        if self.tile_width().is_some() {
            keys.push("tile_width");
        }
        if self.tile_height().is_some() {
            keys.push("tile_height");
        }
        if self.tile_offsets().is_some() {
            keys.push("tile_offsets");
        }
        if self.tile_byte_counts().is_some() {
            keys.push("tile_byte_counts");
        }
        if self.extra_samples().is_some() {
            keys.push("extra_samples");
        }
        if self.jpeg_tables().is_some() {
            keys.push("jpeg_tables");
        }
        if self.copyright().is_some() {
            keys.push("copyright");
        }
        if self.geo_key_directory().is_some() {
            keys.push("geo_key_directory");
        }
        if self.model_pixel_scale().is_some() {
            keys.push("model_pixel_scale");
        }
        if self.model_tiepoint().is_some() {
            keys.push("model_tiepoint");
        }
        if self.model_transformation().is_some() {
            keys.push("model_transformation");
        }
        if self.gdal_nodata().is_some() {
            keys.push("gdal_nodata");
        }
        if self.gdal_metadata().is_some() {
            keys.push("gdal_metadata");
        }
        if self.lerc_parameters().is_some() {
            keys.push("lerc_parameters");
        }
        if self.colormap().is_some() {
            keys.push("colormap");
        }

        keys
    }

    /// This exists to implement the Mapping protocol, so we support `dict(ifd)`.`
    fn __iter__<'py>(&'py self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        self.keys().into_pyobject(py)?.call_method0("__iter__")
    }

    /// Get an IFD property by name
    /// This exists to implement the Mapping protocol, so we support `dict(ifd)`.`
    fn __getitem__<'py>(&self, py: Python<'py>, key: &str) -> PyResult<Bound<'py, PyAny>> {
        match key {
            "new_subfile_type" => self.new_subfile_type().into_bound_py_any(py),
            "image_width" => self.image_width().into_bound_py_any(py),
            "image_height" => self.image_height().into_bound_py_any(py),
            "bits_per_sample" => self.bits_per_sample().into_bound_py_any(py),
            "compression" => self.compression().into_bound_py_any(py),
            "photometric_interpretation" => self.photometric_interpretation().into_bound_py_any(py),
            "document_name" => self.document_name().into_bound_py_any(py),
            "image_description" => self.image_description().into_bound_py_any(py),
            "strip_offsets" => self.strip_offsets().into_bound_py_any(py),
            "orientation" => self.orientation().into_bound_py_any(py),
            "samples_per_pixel" => self.samples_per_pixel().into_bound_py_any(py),
            "rows_per_strip" => self.rows_per_strip().into_bound_py_any(py),
            "strip_byte_counts" => self.strip_byte_counts().into_bound_py_any(py),
            "min_sample_value" => self.min_sample_value().into_bound_py_any(py),
            "max_sample_value" => self.max_sample_value().into_bound_py_any(py),
            "x_resolution" => self.x_resolution().into_bound_py_any(py),
            "y_resolution" => self.y_resolution().into_bound_py_any(py),
            "planar_configuration" => self.planar_configuration().into_bound_py_any(py),
            "resolution_unit" => self.resolution_unit().into_bound_py_any(py),
            "software" => self.software().into_bound_py_any(py),
            "date_time" => self.date_time().into_bound_py_any(py),
            "artist" => self.artist().into_bound_py_any(py),
            "host_computer" => self.host_computer().into_bound_py_any(py),
            "predictor" => self.predictor().into_bound_py_any(py),
            "tile_width" => self.tile_width().into_bound_py_any(py),
            "tile_height" => self.tile_height().into_bound_py_any(py),
            "tile_offsets" => self.tile_offsets().into_bound_py_any(py),
            "tile_byte_counts" => self.tile_byte_counts().into_bound_py_any(py),
            "extra_samples" => self.extra_samples().into_bound_py_any(py),
            "sample_format" => self.sample_format().into_bound_py_any(py),
            "jpeg_tables" => self.jpeg_tables().into_bound_py_any(py),
            "copyright" => self.copyright().into_bound_py_any(py),
            "geo_key_directory" => self.geo_key_directory().into_bound_py_any(py),
            "model_pixel_scale" => self.model_pixel_scale().into_bound_py_any(py),
            "model_tiepoint" => self.model_tiepoint().into_bound_py_any(py),
            "model_transformation" => self.model_transformation().into_bound_py_any(py),
            "other_tags" => self.other_tags().into_bound_py_any(py),
            "gdal_nodata" => self.gdal_nodata().into_bound_py_any(py),
            "gdal_metadata" => self.gdal_metadata().into_bound_py_any(py),
            "lerc_parameters" => self.lerc_parameters().into_bound_py_any(py),
            "colormap" => self.colormap().into_bound_py_any(py),
            _ => Err(pyo3::exceptions::PyKeyError::new_err(format!(
                "Unknown IFD property: {}",
                key
            ))),
        }
    }

    fn fetch_tile<'py>(
        &'py self,
        py: Python<'py>,
        x: usize,
        y: usize,
    ) -> PyResult<Bound<'py, PyAny>> {
        let reader = self.reader.clone();
        let ifd = self.ifd.clone();
        future_into_py(py, async move {
            let tile = ifd
                .fetch_tile(x, y, reader.as_ref())
                .await
                .map_err(|err| PyTypeError::new_err(err.to_string()))?;

            Ok(PyTile::new(tile))
        })
    }

    pub(crate) fn fetch_tiles<'py>(
        &'py self,
        py: Python<'py>,
        xy: Vec<(usize, usize)>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let reader = self.reader.clone();
        let ifd = self.ifd.clone();
        future_into_py(py, async move {
            let tiles = ifd
                .fetch_tiles(&xy, reader.as_ref())
                .await
                .map_err(|err| PyTypeError::new_err(err.to_string()))?;
            let py_tiles = tiles.into_iter().map(PyTile::new).collect::<Vec<_>>();
            Ok(py_tiles)
        })
    }

    fn tile_byte_range(&self, x: usize, y: usize) -> PyAsyncTiffResult<PyTileByteRange> {
        let byte_range = self
            .ifd
            .tile_byte_range(x, y)
            .ok_or(PyValueError::new_err("Not a tiled tiff"))?;
        Ok(PyTileByteRange(byte_range))
    }

    fn is_tile_sparse(&self, x: usize, y: usize) -> PyAsyncTiffResult<bool> {
        self.ifd
            .is_tile_sparse(x, y)
            .ok_or(PyValueError::new_err("Not a tiled tiff").into())
    }

    #[getter]
    fn tile_count(&self) -> Option<(usize, usize)> {
        self.ifd.tile_count()
    }
}

impl PartialEq for PyImageFileDirectory {
    fn eq(&self, other: &Self) -> bool {
        self.ifd == other.ifd
    }
}

struct PyTileByteRange(TileByteRange);

impl<'py> IntoPyObject<'py> for PyTileByteRange {
    type Target = PyAny;
    type Output = Bound<'py, PyAny>;
    type Error = PyErr;

    fn into_pyobject(self, py: Python<'py>) -> Result<Self::Output, Self::Error> {
        match self.0 {
            TileByteRange::Chunky(range) => (range.start, range.end).into_bound_py_any(py),
            TileByteRange::Planar(ranges) => {
                let py_ranges = ranges
                    .iter()
                    .map(|range| (range.start, range.end).into_bound_py_any(py))
                    .collect::<Result<Vec<_>, PyErr>>()?;
                py_ranges.into_bound_py_any(py)
            }
        }
    }
}
