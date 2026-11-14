//! How an exited tab's program ended, as its sidebar row and hover card tell it, and the
//! contrast its status colors need against the sidebar.

/// Whether a program that ended with `code` failed: a nonzero code, or none at all (e.g.
/// killed by a signal).
pub fn exit_failed(code: Option<i32>) -> bool {
    code != Some(0)
}

/// The hover card's status for a program that ended with `code`.
pub fn exit_label(code: Option<i32>) -> String {
    match code {
        Some(0) => "Exited".to_owned(),
        Some(code) => format!("Exited ({code})"),
        None => "Exited (no exit code)".to_owned(),
    }
}

/// The WCAG 2 contrast ratio of two opaque 0xRRGGBB colors, from 1 (same) to 21.
pub fn contrast_ratio(first: u32, second: u32) -> f64 {
    let (first, second) = (relative_luminance(first), relative_luminance(second));
    let (darker, lighter) = if first < second {
        (first, second)
    } else {
        (second, first)
    };
    (lighter + 0.05) / (darker + 0.05)
}

fn relative_luminance(color: u32) -> f64 {
    let channel = |shift: u32| {
        let value = f64::from((color >> shift) & 0xFF) / 255.0;
        if value <= 0.04045 {
            value / 12.92
        } else {
            ((value + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * channel(16) + 0.7152 * channel(8) + 0.0722 * channel(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_zero_code_is_a_clean_exit() {
        assert!(!exit_failed(Some(0)));
        assert!(exit_failed(Some(3)));
        assert!(exit_failed(Some(-1)));
        assert!(exit_failed(None));
    }

    #[test]
    fn the_label_gives_a_failed_code_or_says_there_is_none() {
        assert_eq!(exit_label(Some(0)), "Exited");
        assert_eq!(exit_label(Some(3)), "Exited (3)");
        assert_eq!(exit_label(Some(-1)), "Exited (-1)");
        assert_eq!(exit_label(None), "Exited (no exit code)");
    }

    #[test]
    fn contrast_ratio_matches_wcag_reference_values() {
        assert!((contrast_ratio(0x000000, 0xFFFFFF) - 21.0).abs() < 1e-9);
        assert!((contrast_ratio(0xFFFFFF, 0x000000) - 21.0).abs() < 1e-9);
        assert!((contrast_ratio(0x777777, 0x777777) - 1.0).abs() < 1e-9);
        // #767676 on white is the usual 4.5:1 boundary grey.
        assert!((contrast_ratio(0x767676, 0xFFFFFF) - 4.54).abs() < 0.01);
    }
}
