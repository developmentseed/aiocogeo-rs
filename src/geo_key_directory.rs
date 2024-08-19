use std::collections::HashMap;

use num_enum::{IntoPrimitive, TryFromPrimitive};
use tiff::decoder::ifd::Value;
use tiff::{TiffError, TiffResult};

#[derive(Clone, Copy, Debug, PartialEq, TryFromPrimitive, IntoPrimitive, Eq, Hash)]
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
    ProjCitation = 3073,
    Projection = 3074,
    ProjCoordTrans = 3075,
    ProjLinearUnits = 3076,
    ProjLinearUnitSize = 3077,
    ProjStdParallel1 = 3078,
    ProjStdParallel2 = 3079,
    ProjNatOriginLong = 3080,
    ProjNatOriginLat = 3081,
    ProjFalseEasting = 3082,
    ProjFalseNorthing = 3083,
    ProjFalseOriginLong = 3084,
    ProjFalseOriginLat = 3085,
    ProjFalseOriginEasting = 3086,
    ProjFalseOriginNorthing = 3087,
    ProjCenterLong = 3088,
    ProjCenterLat = 3089,
    ProjCenterEasting = 3090,
    ProjCenterNorthing = 3091,
    ProjScaleAtNatOrigin = 3092,
    ProjScaleAtCenter = 3093,
    ProjAzimuthAngle = 3094,
    ProjStraightVertPoleLong = 3095,

    // Vertical CRS Parameter Keys (4096-5119)
    Vertical = 4096,
    VerticalCitation = 4097,
    VerticalDatum = 4098,
    VerticalUnits = 4099,
}

/// http://docs.opengeospatial.org/is/19-008r4/19-008r4.html#_requirements_class_geokeydirectorytag
#[derive(Debug, Clone)]
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
    proj_citation: Option<String>,
    projection: Option<u16>,
    proj_coord_trans: Option<u16>,
    proj_linear_units: Option<u16>,
    proj_linear_unit_size: Option<f64>,
    proj_std_parallel1: Option<f64>,
    proj_std_parallel2: Option<f64>,
    proj_nat_origin_long: Option<f64>,
    proj_nat_origin_lat: Option<f64>,
    proj_false_easting: Option<f64>,
    proj_false_northing: Option<f64>,
    proj_false_origin_long: Option<f64>,
    proj_false_origin_lat: Option<f64>,
    proj_false_origin_easting: Option<f64>,
    proj_false_origin_northing: Option<f64>,
    proj_center_long: Option<f64>,
    proj_center_lat: Option<f64>,
    proj_center_easting: Option<f64>,
    proj_center_northing: Option<f64>,
    proj_scale_at_nat_origin: Option<f64>,
    proj_scale_at_center: Option<f64>,
    proj_azimuth_angle: Option<f64>,
    proj_straight_vert_pole_long: Option<f64>,

    vertical: Option<u16>,
    vertical_citation: Option<String>,
    vertical_datum: Option<u16>,
    vertical_units: Option<u16>,
}

impl GeoKeyDirectory {
    pub(crate) fn from_tags(mut tag_data: HashMap<GeoKeyTag, Value>) -> TiffResult<Self> {
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
        let mut proj_citation = None;
        let mut projection = None;
        let mut proj_coord_trans = None;
        let mut proj_linear_units = None;
        let mut proj_linear_unit_size = None;
        let mut proj_std_parallel1 = None;
        let mut proj_std_parallel2 = None;
        let mut proj_nat_origin_long = None;
        let mut proj_nat_origin_lat = None;
        let mut proj_false_easting = None;
        let mut proj_false_northing = None;
        let mut proj_false_origin_long = None;
        let mut proj_false_origin_lat = None;
        let mut proj_false_origin_easting = None;
        let mut proj_false_origin_northing = None;
        let mut proj_center_long = None;
        let mut proj_center_lat = None;
        let mut proj_center_easting = None;
        let mut proj_center_northing = None;
        let mut proj_scale_at_nat_origin = None;
        let mut proj_scale_at_center = None;
        let mut proj_azimuth_angle = None;
        let mut proj_straight_vert_pole_long = None;

        let mut vertical = None;
        let mut vertical_citation = None;
        let mut vertical_datum = None;
        let mut vertical_units = None;

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
                GeoKeyTag::ProjCitation => proj_citation = Some(value.into_string()?),
                GeoKeyTag::Projection => projection = Some(value.into_u16()?),
                GeoKeyTag::ProjCoordTrans => proj_coord_trans = Some(value.into_u16()?),
                GeoKeyTag::ProjLinearUnits => proj_linear_units = Some(value.into_u16()?),
                GeoKeyTag::ProjLinearUnitSize => proj_linear_unit_size = Some(value.into_f64()?),
                GeoKeyTag::ProjStdParallel1 => proj_std_parallel1 = Some(value.into_f64()?),
                GeoKeyTag::ProjStdParallel2 => proj_std_parallel2 = Some(value.into_f64()?),
                GeoKeyTag::ProjNatOriginLong => proj_nat_origin_long = Some(value.into_f64()?),
                GeoKeyTag::ProjNatOriginLat => proj_nat_origin_lat = Some(value.into_f64()?),
                GeoKeyTag::ProjFalseEasting => proj_false_easting = Some(value.into_f64()?),
                GeoKeyTag::ProjFalseNorthing => proj_false_northing = Some(value.into_f64()?),
                GeoKeyTag::ProjFalseOriginLong => proj_false_origin_long = Some(value.into_f64()?),
                GeoKeyTag::ProjFalseOriginLat => proj_false_origin_lat = Some(value.into_f64()?),
                GeoKeyTag::ProjFalseOriginEasting => {
                    proj_false_origin_easting = Some(value.into_f64()?)
                }
                GeoKeyTag::ProjFalseOriginNorthing => {
                    proj_false_origin_northing = Some(value.into_f64()?)
                }
                GeoKeyTag::ProjCenterLong => proj_center_long = Some(value.into_f64()?),
                GeoKeyTag::ProjCenterLat => proj_center_lat = Some(value.into_f64()?),
                GeoKeyTag::ProjCenterEasting => proj_center_easting = Some(value.into_f64()?),
                GeoKeyTag::ProjCenterNorthing => proj_center_northing = Some(value.into_f64()?),
                GeoKeyTag::ProjScaleAtNatOrigin => {
                    proj_scale_at_nat_origin = Some(value.into_f64()?)
                }
                GeoKeyTag::ProjScaleAtCenter => proj_scale_at_center = Some(value.into_f64()?),
                GeoKeyTag::ProjAzimuthAngle => proj_azimuth_angle = Some(value.into_f64()?),
                GeoKeyTag::ProjStraightVertPoleLong => {
                    proj_straight_vert_pole_long = Some(value.into_f64()?)
                }
                GeoKeyTag::Vertical => vertical = Some(value.into_u16()?),
                GeoKeyTag::VerticalCitation => vertical_citation = Some(value.into_string()?),
                GeoKeyTag::VerticalDatum => vertical_datum = Some(value.into_u16()?),
                GeoKeyTag::VerticalUnits => vertical_units = Some(value.into_u16()?),
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
            proj_citation,
            projection,
            proj_coord_trans,
            proj_linear_units,
            proj_linear_unit_size,
            proj_std_parallel1,
            proj_std_parallel2,
            proj_nat_origin_long,
            proj_nat_origin_lat,
            proj_false_easting,
            proj_false_northing,
            proj_false_origin_long,
            proj_false_origin_lat,
            proj_false_origin_easting,
            proj_false_origin_northing,
            proj_center_long,
            proj_center_lat,
            proj_center_easting,
            proj_center_northing,
            proj_scale_at_nat_origin,
            proj_scale_at_center,
            proj_azimuth_angle,
            proj_straight_vert_pole_long,

            vertical,
            vertical_citation,
            vertical_datum,
            vertical_units,
        })
    }

    /// Return the EPSG code representing the crs of the image
    pub fn epsg_code(&self) -> Option<u16> {
        if let Some(projected_type) = self.projected_type {
            Some(projected_type)
        } else {
            self.geographic_type
        }
    }
}
