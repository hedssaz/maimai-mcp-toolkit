use image::Rgba;

pub(super) const EMPTY_COLOR: Rgba<u8> = Rgba([229, 236, 244, 255]);
const STOPS: [(f64, [u8; 3]); 6] = [
    (0.00, [219, 245, 255]),
    (0.22, [125, 211, 252]),
    (0.42, [52, 211, 153]),
    (0.62, [250, 204, 21]),
    (0.80, [249, 115, 22]),
    (1.00, [190, 18, 60]),
];

pub(super) fn heatmap_color(value: u64, maximum: u64, minimum_positive: u64) -> Rgba<u8> {
    if value == 0 || maximum == 0 {
        return EMPTY_COLOR;
    }
    let minimum = minimum_positive.max(1).min(maximum);
    let ratio = if maximum <= minimum {
        1.0
    } else {
        ((value.saturating_sub(minimum)) as f64 / (maximum - minimum) as f64)
            .clamp(0.0, 1.0)
            .powf(0.9)
    };
    for pair in STOPS.windows(2) {
        let (left_ratio, left) = pair[0];
        let (right_ratio, right) = pair[1];
        if ratio <= right_ratio {
            let local = (ratio - left_ratio) / (right_ratio - left_ratio).max(0.001);
            return mix(left, right, local);
        }
    }
    let last = STOPS[STOPS.len() - 1].1;
    Rgba([last[0], last[1], last[2], 255])
}

fn mix(left: [u8; 3], right: [u8; 3], ratio: f64) -> Rgba<u8> {
    let ratio = ratio.clamp(0.0, 1.0);
    Rgba([
        (f64::from(left[0]) + (f64::from(right[0]) - f64::from(left[0])) * ratio) as u8,
        (f64::from(left[1]) + (f64::from(right[1]) - f64::from(left[1])) * ratio) as u8,
        (f64::from(left[2]) + (f64::from(right[2]) - f64::from(left[2])) * ratio) as u8,
        255,
    ])
}
