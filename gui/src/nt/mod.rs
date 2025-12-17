#[cfg(not(feature = "ntable"))]
mod disabled;
#[cfg(feature = "ntable")]
mod enabled;
#[cfg(not(feature = "ntable"))]
pub use disabled::*;
#[cfg(feature = "ntable")]
pub use enabled::*;
