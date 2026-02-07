#![no_std]
#![doc = include_str!("../README.md")]
#![cfg_attr(docsrs, feature(doc_cfg))]

extern crate alloc;

#[cfg(any(test, feature = "std"))]
extern crate std;

mod de;
mod error;
mod ser;
#[cfg(test)]
mod tests;
mod trailer;

use self::trailer::*;

pub use self::de::*;
pub use self::error::*;
pub use self::ser::*;
