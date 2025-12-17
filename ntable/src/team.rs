use std::fmt::{self, Display, Formatter};
use std::str::FromStr;
use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TeamNumber(u16);
impl TeamNumber {
    pub const fn new(team: u16) -> Option<Self> {
        if team < 25600 { Some(Self(team)) } else { None }
    }
    pub const fn new_unchecked(team: u16) -> Self {
        Self(team)
    }
    pub const fn team(self) -> u16 {
        self.0
    }
    pub const fn upper(self) -> u8 {
        (self.0 / 100) as u8
    }
    pub const fn lower(self) -> u8 {
        (self.0 % 100) as u8
    }
    pub const fn to_ipv4(self) -> Ipv4Team {
        Ipv4Team(self)
    }
    /// Parse an IPv4 address of the form `10.te.am.x` into a team number
    pub fn parse_ipv4(ipv4: &str) -> Option<Self> {
        use atoi::FromRadix10;
        let bytes = ipv4.as_bytes();
        let rest = bytes.strip_prefix(b"10.")?;
        let (upper, used) = u16::from_radix_10(rest);
        if used == 0 {
            return None;
        }
        if upper > 255 {
            return None;
        }
        let rest = rest.get(used..)?.strip_prefix(b".")?;
        let (lower, used) = u16::from_radix_10(rest);
        if used == 0 {
            return None;
        }
        if lower > 99 {
            return None;
        }
        let rest = rest.get(used..)?.strip_prefix(b".")?;
        if rest.is_empty() {
            return None;
        }
        let (last, used) = u16::from_radix_10(rest);
        if used < rest.len() {
            return None;
        }
        if last > 255 {
            return None;
        }
        Some(Self::new_unchecked(upper * 100 + lower))
    }
}
impl serde::Serialize for TeamNumber {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_u16(self.0)
    }
}
impl<'de> serde::Deserialize<'de> for TeamNumber {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        use serde::de::{Error, Unexpected};
        u16::deserialize(deserializer).and_then(|team| {
            Self::new(team).ok_or_else(|| {
                D::Error::invalid_value(
                    Unexpected::Unsigned(team as _),
                    &"a team number from 0..=25599",
                )
            })
        })
    }
}
impl FromStr for TeamNumber {
    type Err = TeamParseError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let team = s.parse()?;
        TeamNumber::new(team).ok_or(TeamParseError::OutOfRange(team))
    }
}
impl Display for TeamNumber {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        Display::fmt(&self.0, f)
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Ipv4Team(pub TeamNumber);
impl Display for Ipv4Team {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "10.{}.{}.1", self.0.upper(), self.0.lower())
    }
}

#[derive(Debug, Clone, Error)]
pub enum TeamParseError {
    #[error(transparent)]
    Parse(#[from] std::num::ParseIntError),
    #[error("team number {0} is out of range of 0..=25599")]
    OutOfRange(u16),
}
