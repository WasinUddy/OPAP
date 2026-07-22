use core::fmt;

/// A rejected analytics input or checked-arithmetic failure.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum AnalyticsError {
    /// An interval ends before it begins.
    InvalidInterval { start_ms: i64, end_ms: i64 },
    /// A therapy slice is not contained by its session window.
    SliceOutsideSession {
        session_start_ms: i64,
        session_end_ms: i64,
        slice_start_ms: i64,
        slice_end_ms: i64,
    },
    /// Two non-empty slices in one session overlap.
    OverlappingSessionSlices {
        first_start_ms: i64,
        first_end_ms: i64,
        second_start_ms: i64,
        second_end_ms: i64,
    },
    /// A signal has no samples.
    EmptySignal,
    /// A signal value was NaN or infinite.
    NonFiniteSignalValue { index: usize },
    /// A sample has no represented duration.
    ZeroSampleWeight { index: usize },
    /// A percentile was NaN, infinite, or outside the inclusive range 0..=1.
    InvalidPercentile,
    /// A regular signal has a zero sample period.
    ZeroSamplePeriod,
    /// An event index was requested without positive therapy time.
    NoTherapyTime,
    /// A civil calendar date is invalid.
    InvalidCivilDate { year: i32, month: u8, day: u8 },
    /// Checked integer or floating-point arithmetic could not represent a result.
    ArithmeticOverflow(&'static str),
}

impl fmt::Display for AnalyticsError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidInterval { start_ms, end_ms } => {
                write!(
                    formatter,
                    "interval ends before it starts: {start_ms}..{end_ms}"
                )
            }
            Self::SliceOutsideSession {
                session_start_ms,
                session_end_ms,
                slice_start_ms,
                slice_end_ms,
            } => write!(
                formatter,
                "slice {slice_start_ms}..{slice_end_ms} is outside session {session_start_ms}..{session_end_ms}"
            ),
            Self::OverlappingSessionSlices {
                first_start_ms,
                first_end_ms,
                second_start_ms,
                second_end_ms,
            } => write!(
                formatter,
                "session slices overlap: {first_start_ms}..{first_end_ms} and {second_start_ms}..{second_end_ms}"
            ),
            Self::EmptySignal => formatter.write_str("signal contains no samples"),
            Self::NonFiniteSignalValue { index } => {
                write!(formatter, "signal value at index {index} is not finite")
            }
            Self::ZeroSampleWeight { index } => {
                write!(
                    formatter,
                    "signal sample at index {index} has zero duration"
                )
            }
            Self::InvalidPercentile => {
                formatter.write_str("percentile must be finite and in the range 0..=1")
            }
            Self::ZeroSamplePeriod => {
                formatter.write_str("regular signal sample period must be positive")
            }
            Self::NoTherapyTime => {
                formatter.write_str("event index requires positive therapy time")
            }
            Self::InvalidCivilDate { year, month, day } => {
                write!(
                    formatter,
                    "invalid civil date: {year:04}-{month:02}-{day:02}"
                )
            }
            Self::ArithmeticOverflow(operation) => {
                write!(formatter, "arithmetic overflow while {operation}")
            }
        }
    }
}

impl std::error::Error for AnalyticsError {}
