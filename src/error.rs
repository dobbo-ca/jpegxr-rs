use std::fmt;

/// One variant per Go error. The `Display` text is Go's `Error()` text, which the parity
/// oracle and `tests/data/pinned.txt` compare; `docs/parity.md` maps variants to error classes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    /// `ErrInternal`: a decoder bug caught as a panic. Carries the panic message.
    Internal(String),
    /// `errFormat`: the container's pixel format is not one of the five this crate decodes.
    PixelFormat,
    /// `errTooLarge`: more than 2^40 pixels.
    TooLarge,
    /// `container.ErrFormat`: a malformed or unsupported container.
    Container,
    /// `codec.ErrFormat`: a malformed codestream.
    Codestream,
    /// `codec.ErrUnsupported`: a codestream feature this crate does not decode.
    Unsupported,
    /// `errDst`: the destination or stride is too small for the image.
    DstTooSmall,
    /// `errBudgetExceeded`: the decode's estimated peak memory passed `Limits::max_alloc`.
    /// Go charged this against a process-wide budget (`SetMemoryLimit`); here the caller's
    /// `Limits` carry it.
    AllocLimit { need: u64, max: u64 },
    /// A 16-bit sample format with an odd stride (new). Go and jxrlib take `stride / 2` samples a
    /// row and so silently drop the last byte of the stride; here it is refused.
    OddStride,
    /// The image width, height or pixel count passed a `Limits` field (new; Go had no cap).
    LimitExceeded(&'static str),
    /// The allocator refused a size within the limits (new).
    OutOfMemory,
    /// The cancel flag was set (Go's `context.Canceled`).
    Cancelled,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Internal(m) => write!(f, "jpegxr: internal error: {m}"),
            Self::PixelFormat => f.write_str("jpegxr: unsupported pixel format"),
            Self::TooLarge => f.write_str("jpegxr: image too large"),
            Self::Container => f.write_str("jpegxr: bad container"),
            Self::Codestream => f.write_str("jpegxr: bad codestream"),
            Self::Unsupported => f.write_str("jpegxr: unsupported codestream feature"),
            Self::DstTooSmall => f.write_str("jpegxr: dst too small for the image"),
            Self::AllocLimit { need, max } => write!(
                f,
                "jpegxr: decode needs {need} bytes, over the {max} byte memory limit"
            ),
            Self::OddStride => f.write_str("jpegxr: odd stride for 16-bit samples"),
            Self::LimitExceeded(what) => write!(f, "jpegxr: {what} over the limit"),
            Self::OutOfMemory => f.write_str("jpegxr: out of memory"),
            Self::Cancelled => f.write_str("jpegxr: cancelled"),
        }
    }
}

impl std::error::Error for Error {}
