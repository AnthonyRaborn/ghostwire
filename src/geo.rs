const EARTH_RADIUS_KM: f64 = 6371.0088;

/// Great-circle distance between two points, in km.
pub fn distance_km(lat1: f64, lon1: f64, lat2: f64, lon2: f64) -> f64 {
    let (p1, p2) = (lat1.to_radians(), lat2.to_radians());
    let dp = (lat2 - lat1).to_radians();
    let dl = (lon2 - lon1).to_radians();
    let a = (dp / 2.0).sin().powi(2) + p1.cos() * p2.cos() * (dl / 2.0).sin().powi(2);
    2.0 * EARTH_RADIUS_KM * a.sqrt().asin()
}

/// Initial bearing from point 1 toward point 2, in degrees clockwise from north.
pub fn bearing_deg(lat1: f64, lon1: f64, lat2: f64, lon2: f64) -> f64 {
    let (p1, p2) = (lat1.to_radians(), lat2.to_radians());
    let dl = (lon2 - lon1).to_radians();
    let y = dl.sin() * p2.cos();
    let x = p1.cos() * p2.sin() - p1.sin() * p2.cos() * dl.cos();
    y.atan2(x).to_degrees().rem_euclid(360.0)
}

/// Elevation angle of an object `altitude_km` up, above the point `ground_km` away
/// along the surface. Positive is above the horizon, negative below.
pub fn elevation_deg(ground_km: f64, altitude_km: f64) -> f64 {
    let gamma = ground_km / EARTH_RADIUS_KM;
    let ratio = EARTH_RADIUS_KM / (EARTH_RADIUS_KM + altitude_km);
    (gamma.cos() - ratio).atan2(gamma.sin()).to_degrees()
}

fn octant(deg: f64) -> usize {
    ((deg.rem_euclid(360.0) + 22.5) / 45.0) as usize % 8
}

/// Eight-point compass direction, e.g. `"NE"`.
pub fn compass(deg: f64) -> &'static str {
    ["N", "NE", "E", "SE", "S", "SW", "W", "NW"][octant(deg)]
}

/// An arrow pointing along `deg`.
pub fn arrow(deg: f64) -> char {
    ['↑', '↗', '→', '↘', '↓', '↙', '←', '↖'][octant(deg)]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn distance_between_known_cities() {
        // San Francisco → Los Angeles is about 559 km.
        let d = distance_km(37.7749, -122.4194, 34.0522, -118.2437);
        assert!((d - 559.0).abs() < 5.0, "{d}");
    }

    #[test]
    fn elevation_from_overhead_to_below_horizon() {
        assert!((elevation_deg(0.0, 417.0) - 90.0).abs() < 1e-9);
        let ratio = EARTH_RADIUS_KM / (EARTH_RADIUS_KM + 417.0);
        let horizon_km = EARTH_RADIUS_KM * ratio.acos();
        assert!(elevation_deg(horizon_km, 417.0).abs() < 1e-6);
        assert!(elevation_deg(horizon_km - 10.0, 417.0) > 0.0);
        assert!(elevation_deg(horizon_km + 10.0, 417.0) < 0.0);
    }

    #[test]
    fn bearings_and_compass() {
        assert!((bearing_deg(0.0, 0.0, 10.0, 0.0) - 0.0).abs() < 1e-6);
        assert!((bearing_deg(0.0, 0.0, 0.0, 10.0) - 90.0).abs() < 1e-6);
        assert_eq!(compass(0.0), "N");
        assert_eq!(compass(44.0), "NE");
        assert_eq!(compass(350.0), "N");
        assert_eq!(compass(225.0), "SW");
        assert_eq!(arrow(90.0), '→');
    }
}
