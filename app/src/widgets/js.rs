pub fn wire(value: f32) -> f64 {
    value.to_string().parse().unwrap_or(f64::from(value))
}

pub fn to_fixed(value: f64, digits: usize) -> String {
    if value.is_nan() {
        return "NaN".to_string();
    }
    if value.is_infinite() {
        return if value > 0. { "Infinity" } else { "-Infinity" }.to_string();
    }
    let exact = format!("{:.80}", value.abs());
    let (whole, fraction) = exact.split_once('.').unwrap_or((&exact, ""));
    let mut units: u128 = format!("{whole}{}", &fraction[..digits])
        .parse()
        .unwrap_or(0);
    if fraction.as_bytes()[digits] >= b'5' {
        units += 1;
    }
    let sign = if value < 0. { "-" } else { "" };
    if digits == 0 {
        return format!("{sign}{units}");
    }
    let scale = 10u128.pow(digits as u32);
    format!(
        "{sign}{}.{:0width$}",
        units / scale,
        units % scale,
        width = digits
    )
}

#[cfg(test)]
mod tests {
    use super::{to_fixed, wire};

    #[test]
    fn to_fixed_rounds_the_exact_value_half_up_like_javascript() {
        assert_eq!(to_fixed(0.6875, 3), "0.688");
        assert_eq!(to_fixed(0.0005, 3), "0.001");
        assert_eq!(to_fixed(1.005, 2), "1.00");
        assert_eq!(to_fixed(12.345, 2), "12.35");
        assert_eq!(to_fixed(0., 3), "0.000");
        assert_eq!(to_fixed(-0.0001, 3), "-0.000");
        assert_eq!(to_fixed(-0., 3), "0.000");
        assert_eq!(to_fixed(-1.25, 1), "-1.3");
        assert_eq!(to_fixed(7., 0), "7");
    }

    #[test]
    fn a_wire_value_is_the_f32_as_its_json_reads_back() {
        assert_eq!(wire(0.7), 0.7);
        assert_ne!(f64::from(0.7f32), 0.7);
        assert_eq!(
            serde_json::from_str::<f64>(&serde_json::to_string(&0.123_45f32).unwrap()).unwrap(),
            wire(0.123_45)
        );
    }
}
