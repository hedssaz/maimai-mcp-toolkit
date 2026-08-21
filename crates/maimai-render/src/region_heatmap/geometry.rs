use serde::Deserialize;
use serde_json::Value;

const MAX_FEATURES: usize = 64;
const MAX_RINGS: usize = 4_096;
const MAX_POINTS: usize = 500_000;

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct GeoPoint {
    pub longitude: f64,
    pub latitude: f64,
}

#[derive(Debug)]
pub(super) struct RegionFeature {
    pub name: String,
    pub rings: Vec<Vec<GeoPoint>>,
    pub center: Option<GeoPoint>,
}

#[derive(Deserialize)]
struct FeatureCollection {
    features: Vec<Feature>,
}

#[derive(Deserialize)]
struct Feature {
    #[serde(default)]
    properties: Properties,
    #[serde(default)]
    geometry: Geometry,
}

#[derive(Default, Deserialize)]
struct Properties {
    #[serde(default)]
    name: String,
    centroid: Option<[f64; 2]>,
    center: Option<[f64; 2]>,
}

#[derive(Default, Deserialize)]
struct Geometry {
    #[serde(rename = "type", default)]
    kind: String,
    #[serde(default)]
    coordinates: Value,
}

pub(super) fn load_features(source: &[u8]) -> Result<Vec<RegionFeature>, ()> {
    let collection: FeatureCollection = serde_json::from_slice(source).map_err(|_| ())?;
    if collection.features.is_empty() || collection.features.len() > MAX_FEATURES {
        return Err(());
    }
    let mut result = Vec::new();
    let mut ring_count = 0_usize;
    let mut point_count = 0_usize;
    for feature in collection.features {
        let raw_name = feature.properties.name.trim();
        if raw_name.is_empty() || raw_name.ends_with("_JD") {
            continue;
        }
        let raw_rings = exterior_rings(&feature.geometry)?;
        let mut rings = Vec::new();
        for raw_ring in raw_rings {
            let ring = parse_ring(raw_ring)?;
            if ring.len() < 3 || outside_primary_latitudes(&ring) {
                continue;
            }
            ring_count = ring_count.checked_add(1).ok_or(())?;
            point_count = point_count.checked_add(ring.len()).ok_or(())?;
            if ring_count > MAX_RINGS || point_count > MAX_POINTS {
                return Err(());
            }
            rings.push(ring);
        }
        if rings.is_empty() {
            continue;
        }
        let center = feature
            .properties
            .centroid
            .or(feature.properties.center)
            .map(point)
            .transpose()?;
        result.push(RegionFeature {
            name: canonical_name(raw_name),
            rings,
            center,
        });
    }
    if result.is_empty() {
        Err(())
    } else {
        Ok(result)
    }
}

fn exterior_rings(geometry: &Geometry) -> Result<Vec<&Value>, ()> {
    match geometry.kind.as_str() {
        "Polygon" => {
            let polygons = geometry.coordinates.as_array().ok_or(())?;
            Ok(polygons.first().into_iter().collect())
        }
        "MultiPolygon" => geometry
            .coordinates
            .as_array()
            .ok_or(())?
            .iter()
            .map(|polygon| polygon.as_array().and_then(|rings| rings.first()).ok_or(()))
            .collect(),
        _ => Ok(Vec::new()),
    }
}

fn parse_ring(value: &Value) -> Result<Vec<GeoPoint>, ()> {
    value
        .as_array()
        .ok_or(())?
        .iter()
        .map(|point_value| {
            let pair = point_value.as_array().ok_or(())?;
            if pair.len() < 2 {
                return Err(());
            }
            point([pair[0].as_f64().ok_or(())?, pair[1].as_f64().ok_or(())?])
        })
        .collect()
}

fn point(value: [f64; 2]) -> Result<GeoPoint, ()> {
    if !value[0].is_finite()
        || !value[1].is_finite()
        || !(-180.0..=180.0).contains(&value[0])
        || !(-90.0..=90.0).contains(&value[1])
    {
        return Err(());
    }
    Ok(GeoPoint {
        longitude: value[0],
        latitude: value[1],
    })
}

fn outside_primary_latitudes(ring: &[GeoPoint]) -> bool {
    let minimum = ring
        .iter()
        .map(|point| point.latitude)
        .fold(f64::INFINITY, f64::min);
    let maximum = ring
        .iter()
        .map(|point| point.latitude)
        .fold(f64::NEG_INFINITY, f64::max);
    maximum < 17.5 || minimum > 54.5
}

fn canonical_name(value: &str) -> String {
    match value {
        "北京市" => "北京",
        "天津市" => "天津",
        "河北省" => "河北",
        "山西省" => "山西",
        "内蒙古自治区" => "内蒙古",
        "辽宁省" => "辽宁",
        "吉林省" => "吉林",
        "黑龙江省" => "黑龙江",
        "上海市" => "上海",
        "江苏省" => "江苏",
        "浙江省" => "浙江",
        "安徽省" => "安徽",
        "福建省" => "福建",
        "江西省" => "江西",
        "山东省" => "山东",
        "河南省" => "河南",
        "湖北省" => "湖北",
        "湖南省" => "湖南",
        "广东省" => "广东",
        "广西壮族自治区" => "广西",
        "海南省" => "海南",
        "重庆市" => "重庆",
        "四川省" => "四川",
        "贵州省" => "贵州",
        "云南省" => "云南",
        "西藏自治区" => "西藏",
        "陕西省" => "陕西",
        "甘肃省" => "甘肃",
        "青海省" => "青海",
        "宁夏回族自治区" => "宁夏",
        "新疆维吾尔自治区" => "新疆",
        "台湾省" => "台湾",
        "香港特别行政区" => "香港",
        "澳门特别行政区" => "澳门",
        other => other,
    }
    .to_owned()
}

#[cfg(test)]
pub(super) fn feature_names(features: &[RegionFeature]) -> Vec<&str> {
    features
        .iter()
        .map(|feature| feature.name.as_str())
        .collect()
}
