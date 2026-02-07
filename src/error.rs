/// Property list parsing error.
#[derive(Debug)]
pub struct InvalidPropertyList;

impl core::fmt::Display for InvalidPropertyList {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str("Invalid property list")
    }
}

#[cfg(feature = "std")]
#[cfg_attr(docsrs, doc(cfg(feature = "std")))]
impl std::error::Error for InvalidPropertyList {}

impl serde::de::Error for InvalidPropertyList {
    fn custom<T>(_: T) -> Self
    where
        T: core::fmt::Display,
    {
        Self
    }
}

/// Same as [`Infallible`](core::convert::Infallible) but implements `serde::ser::Error`.
#[derive(Debug)]
pub enum Infallible {}

impl core::fmt::Display for Infallible {
    fn fmt(&self, _f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match *self {}
    }
}

#[cfg(feature = "std")]
#[cfg_attr(docsrs, doc(cfg(feature = "std")))]
impl std::error::Error for Infallible {}

impl serde::ser::Error for Infallible {
    fn custom<T>(_msg: T) -> Self
    where
        T: core::fmt::Display,
    {
        unreachable!()
    }
}
