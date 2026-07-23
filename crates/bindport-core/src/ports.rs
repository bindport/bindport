use super::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PortRange {
    pub start: u16,
    pub end: u16,
}

impl PortRange {
    pub const fn contains(self, port: u16) -> bool {
        self.start <= port && port <= self.end
    }

    pub const fn len(self) -> u32 {
        if self.is_empty() {
            0
        } else {
            self.end as u32 - self.start as u32 + 1
        }
    }

    pub const fn is_empty(self) -> bool {
        self.start > self.end
    }
}

pub fn is_default_skip_port(port: u16) -> bool {
    DEFAULT_SKIP_PORTS.contains(&port)
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PortRangeParseError {
    MissingSeparator,
    InvalidStart(String),
    StartBelowMinimum { start: u16, minimum: u16 },
    InvalidEnd(String),
    Empty(PortRange),
}

impl fmt::Display for PortRangeParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingSeparator => write!(f, "expected START-END"),
            Self::InvalidStart(value) => write!(f, "invalid range start `{value}`"),
            Self::StartBelowMinimum { start, minimum } => {
                write!(f, "range start {start} must be at least {minimum}")
            }
            Self::InvalidEnd(value) => write!(f, "invalid range end `{value}`"),
            Self::Empty(range) => write!(
                f,
                "range start {} must be less than or equal to end {}",
                range.start, range.end
            ),
        }
    }
}

impl std::error::Error for PortRangeParseError {}
pub fn stable_port_scan_start(seed: &str, range: PortRange) -> Option<u16> {
    if range.is_empty() {
        return None;
    }

    let offset = stable_hash(seed.as_bytes()) % u64::from(range.len());
    let port = range.start as u32 + u32::try_from(offset).expect("range length fits in u32");

    Some(u16::try_from(port).expect("port remains within configured range"))
}
pub fn parse_port_range(value: &str) -> Result<PortRange, PortRangeParseError> {
    let (start, end) = value
        .split_once('-')
        .ok_or(PortRangeParseError::MissingSeparator)?;
    let start = start
        .trim()
        .parse::<u16>()
        .map_err(|_| PortRangeParseError::InvalidStart(start.trim().to_owned()))?;
    let end = end
        .trim()
        .parse::<u16>()
        .map_err(|_| PortRangeParseError::InvalidEnd(end.trim().to_owned()))?;
    if start == 0 {
        return Err(PortRangeParseError::StartBelowMinimum { start, minimum: 1 });
    }
    let range = PortRange { start, end };

    if range.is_empty() {
        return Err(PortRangeParseError::Empty(range));
    }

    Ok(range)
}
