//! TUI/CLI palette. Will be generated from `@gitraptor/tokens` (packages/design-tokens).

/// An RGB color.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rgb(pub u8, pub u8, pub u8);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rgb_is_comparable() {
        assert_eq!(Rgb(0, 0, 0), Rgb(0, 0, 0));
    }
}
