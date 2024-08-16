use std::collections::HashMap;

use num_enum::{IntoPrimitive, TryFromPrimitive};
use tiff::decoder::ifd::Value;
use tiff::{TiffError, TiffResult};

// TODO: copy over the rest of the projected and vertical keys from
// https://docs.ogc.org/is/19-008r4/19-008r4.html#_summary_of_geokey_ids_and_names
// to here.

#[derive(Clone, Copy, Debug, PartialEq, TryFromPrimitive, IntoPrimitive)]
#[repr(u16)]
pub enum GeoKeyTag {
    // GeoTIFF configuration keys
    ModelType = 1024,
    RasterType = 1025,
    Citation = 1026,

    // Geodetic CRS Parameter Keys
    GeographicType = 2048,
    GeogCitation = 2049,
    GeogGeodeticDatum = 2050,
    GeogPrimeMeridian = 2051,
    GeogLinearUnits = 2052,
    GeogLinearUnitSize = 2053,
    GeogAngularUnits = 2054,
    GeogAngularUnitSize = 2055,
    GeogEllipsoid = 2056,
    GeogSemiMajorAxis = 2057,
    GeogSemiMinorAxis = 2058,
    GeogInvFlattening = 2059,
    GeogAzimuthUnits = 2060,
    GeogPrimeMeridianLong = 2061,

    // Projected CRS Parameter Keys
    ProjectedType = 3072,
    // Vertical CRS Parameter Keys (4096-5119)
}

/// http://docs.opengeospatial.org/is/19-008r4/19-008r4.html#_requirements_class_geokeydirectorytag
pub struct GeoKeyDirectory {
    model_type: Option<u16>,
    raster_type: Option<u16>,
    citation: Option<String>,

    geographic_type: Option<u16>,
    geog_citation: Option<String>,
    geog_geodetic_datum: Option<u16>,
    geog_prime_meridian: Option<u16>,
    geog_linear_units: Option<u16>,
    geog_linear_unit_size: Option<f64>,
    geog_angular_units: Option<u16>,
    geog_angular_unit_size: Option<f64>,
    geog_ellipsoid: Option<u16>,
    geog_semi_major_axis: Option<f64>,
    geog_semi_minor_axis: Option<f64>,
    geog_inv_flattening: Option<f64>,
    geog_azimuth_units: Option<u16>,
    geog_prime_meridian_long: Option<f64>,

    projected_type: Option<u16>,
}

impl GeoKeyDirectory {
    fn from_tags(
        mut tag_data: HashMap<GeoKeyTag, Value>,
        next_ifd_offset: Option<usize>,
    ) -> TiffResult<Self> {
        let mut model_type = None;
        let mut raster_type = None;
        let mut citation = None;

        let mut geographic_type = None;
        let mut geog_citation = None;
        let mut geog_geodetic_datum = None;
        let mut geog_prime_meridian = None;
        let mut geog_linear_units = None;
        let mut geog_linear_unit_size = None;
        let mut geog_angular_units = None;
        let mut geog_angular_unit_size = None;
        let mut geog_ellipsoid = None;
        let mut geog_semi_major_axis = None;
        let mut geog_semi_minor_axis = None;
        let mut geog_inv_flattening = None;
        let mut geog_azimuth_units = None;
        let mut geog_prime_meridian_long = None;

        let mut projected_type = None;

        tag_data.drain().try_for_each(|(tag, value)| {
            match tag {
                GeoKeyTag::ModelType => model_type = Some(value.into_u16()?),
                GeoKeyTag::RasterType => raster_type = Some(value.into_u16()?),
                GeoKeyTag::Citation => citation = Some(value.into_string()?),
                GeoKeyTag::GeographicType => geographic_type = Some(value.into_u16()?),
                GeoKeyTag::GeogCitation => geog_citation = Some(value.into_string()?),
                GeoKeyTag::GeogGeodeticDatum => geog_geodetic_datum = Some(value.into_u16()?),
                GeoKeyTag::GeogPrimeMeridian => geog_prime_meridian = Some(value.into_u16()?),
                GeoKeyTag::GeogLinearUnits => geog_linear_units = Some(value.into_u16()?),
                GeoKeyTag::GeogLinearUnitSize => geog_linear_unit_size = Some(value.into_f64()?),
                GeoKeyTag::GeogAngularUnits => geog_angular_units = Some(value.into_u16()?),
                GeoKeyTag::GeogAngularUnitSize => geog_angular_unit_size = Some(value.into_f64()?),
                GeoKeyTag::GeogEllipsoid => geog_ellipsoid = Some(value.into_u16()?),
                GeoKeyTag::GeogSemiMajorAxis => geog_semi_major_axis = Some(value.into_f64()?),
                GeoKeyTag::GeogSemiMinorAxis => geog_semi_minor_axis = Some(value.into_f64()?),
                GeoKeyTag::GeogInvFlattening => geog_inv_flattening = Some(value.into_f64()?),
                GeoKeyTag::GeogAzimuthUnits => geog_azimuth_units = Some(value.into_u16()?),
                GeoKeyTag::GeogPrimeMeridianLong => {
                    geog_prime_meridian_long = Some(value.into_f64()?)
                }
                GeoKeyTag::ProjectedType => projected_type = Some(value.into_u16()?),
            };
            Ok::<_, TiffError>(())
        })?;

        Ok(Self {
            model_type,
            raster_type,
            citation,

            geographic_type,
            geog_citation,
            geog_geodetic_datum,
            geog_prime_meridian,
            geog_linear_units,
            geog_linear_unit_size,
            geog_angular_units,
            geog_angular_unit_size,
            geog_ellipsoid,
            geog_semi_major_axis,
            geog_semi_minor_axis,
            geog_inv_flattening,
            geog_azimuth_units,
            geog_prime_meridian_long,

            projected_type,
        })
        // todo!()
    }

    /// Return the EPSG code representing the crs of the image
    pub fn epsg_code(&self) -> u16 {
        if let Some(projected_type) = self.projected_type {
            projected_type
        } else if let Some(geographic_type) = self.geographic_type {
            geographic_type
        } else {
            panic!("Custom projections not yet supported");
        }
    }
}
