// wirespec-layout: Layout IR
pub mod bitgroup;
pub mod ir;
pub mod lower;

pub use ir::LayoutModule;
pub use lower::lower;
