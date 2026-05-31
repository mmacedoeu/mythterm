//! Cell and line data structures for the terminal grid.

#[derive(Debug, Clone, Default)]
pub struct Cell {
    pub character: char,
    pub attrs: CellAttributes,
}

#[derive(Debug, Clone, Default)]
pub struct CellAttributes {
    pub foreground: ColorAttribute,
    pub background: ColorAttribute,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub strikethrough: bool,
    pub reverse: bool,
}

#[derive(Debug, Clone)]
pub enum ColorAttribute {
    Default,
    PaletteIndex(u8),
    TrueColor([u8; 3]),
}

impl Default for ColorAttribute {
    fn default() -> Self { Self::Default }
}
