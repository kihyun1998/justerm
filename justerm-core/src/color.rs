//! Color references — how a cell names its colour without committing to a pixel
//! value. The engine is theme-agnostic: it stores references only; resolving a
//! reference to an actual colour is the consumer/renderer's job (it owns the
//! frozen scheme). The engine never knows hex. See CONTEXT.md "Color reference".

/// A cell's foreground or background colour, stored as a reference.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Color {
    /// The scheme's default fg/bg — resolved downstream.
    #[default]
    Default,
    /// A slot in the 256-colour palette (0..=15 are the named ANSI colours,
    /// 16..=255 the cube + greyscale). The renderer maps it to hex.
    Indexed(u8),
    /// A direct 24-bit truecolour triple.
    Rgb(u8, u8, u8),
}
