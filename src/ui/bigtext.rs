//! A 3×5 block font for the few numbers that deserve to be big.

pub const HEIGHT: usize = 5;

fn glyph(c: char) -> Option<[&'static str; HEIGHT]> {
    Some(match c {
        '0' => ["███", "█ █", "█ █", "█ █", "███"],
        '1' => [" █ ", "██ ", " █ ", " █ ", "███"],
        '2' => ["███", "  █", "███", "█  ", "███"],
        '3' => ["███", "  █", " ██", "  █", "███"],
        '4' => ["█ █", "█ █", "███", "  █", "  █"],
        '5' => ["███", "█  ", "███", "  █", "███"],
        '6' => ["███", "█  ", "███", "█ █", "███"],
        '7' => ["███", "  █", "  █", "  █", "  █"],
        '8' => ["███", "█ █", "███", "█ █", "███"],
        '9' => ["███", "█ █", "███", "  █", "███"],
        '-' => ["   ", "   ", "███", "   ", "   "],
        '.' => [" ", " ", " ", " ", "█"],
        _ => return None,
    })
}

/// The five rows of `text` in block letters, one space between glyphs. Characters the
/// font doesn't have are skipped.
pub fn render(text: &str) -> [String; HEIGHT] {
    let glyphs: Vec<_> = text.chars().filter_map(glyph).collect();
    std::array::from_fn(|row| glyphs.iter().map(|g| g[row]).collect::<Vec<_>>().join(" "))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_digits_side_by_side() {
        let rows = render("17");
        assert_eq!(rows[0], " █  ███");
        assert_eq!(rows[4], "███   █");
        assert!(rows.iter().all(|r| r.chars().count() == 7));
    }

    #[test]
    fn skips_unknown_characters() {
        assert_eq!(render("4°C")[0], render("4")[0]);
        assert_eq!(render("-3.5")[4], "    ███ █ ███");
    }
}
